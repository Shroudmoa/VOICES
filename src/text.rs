//! Turning a file (or a pipe) into something a reader can follow.
//!
//! The document is split into blocks (paragraphs, headings, quotes, list items,
//! code) and then into sentences. Every word keeps a stable global index, which
//! is the same index the speaker uses when it reports "this word is being
//! spoken right now". The screen and the audio therefore agree without either
//! side having to search the text again.

use unicode_width::UnicodeWidthStr;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockKind {
    Paragraph,
    Heading,
    Quote,
    List,
    Code,
}

#[derive(Clone, Debug)]
pub struct Word {
    /// Stable index across the whole document.
    pub index: usize,
    /// The token exactly as it should be shown, punctuation included.
    pub text: String,
    pub sentence: usize,
    pub block: usize,
    /// Rough syllable-like weight, used to distribute an audio file's known
    /// duration across its words for highlighting.
    pub weight: f32,
}

#[derive(Clone, Debug)]
pub struct Sentence {
    pub index: usize,
    pub block: usize,
    pub text: String,
    pub words: Vec<Word>,
    pub weight: f32,
    pub char_count: usize,
}

impl Sentence {
    pub fn first_word(&self) -> Option<usize> {
        self.words.first().map(|w| w.index)
    }

    pub fn last_word(&self) -> Option<usize> {
        self.words.last().map(|w| w.index)
    }
}

#[derive(Clone, Debug)]
pub struct Block {
    pub kind: BlockKind,
    /// Indices into `Document::sentences`.
    pub sentences: Vec<usize>,
}

#[derive(Clone, Debug)]
pub struct Document {
    pub title: String,
    pub source: Option<String>,
    pub blocks: Vec<Block>,
    pub sentences: Vec<Sentence>,
    pub word_count: usize,
    pub char_count: usize,
    /// Sentences joined by newlines: what Piper reads for an export.
    pub speech_text: String,
}

impl Document {
    /// Parse plain text or Markdown. `default_title` is used when the document
    /// has no heading of its own (usually the file's stem).
    pub fn parse(input: &str, source: Option<String>, default_title: &str) -> Document {
        let raw = split_blocks(input);
        let mut doc = Document {
            title: default_title.to_string(),
            source,
            blocks: Vec::new(),
            sentences: Vec::new(),
            word_count: 0,
            char_count: 0,
            speech_text: String::new(),
        };

        // A document with no usable prose is still a document: the UI shows an
        // empty reader rather than refusing to open.
        for (kind, text) in raw {
            let text = text.trim();
            if text.is_empty() {
                continue;
            }
            let block_index = doc.blocks.len();
            let mut sentence_indices = Vec::new();
            for tokens in split_sentences(text) {
                if tokens.is_empty() {
                    continue;
                }
                let mut words = Vec::with_capacity(tokens.len());
                for token in tokens {
                    let index = doc.word_count;
                    let weight = speech_weight(&token);
                    words.push(Word {
                        index,
                        text: token,
                        sentence: doc.sentences.len(),
                        block: block_index,
                        weight,
                    });
                    doc.word_count += 1;
                }
                let sentence_text = words
                    .iter()
                    .map(|w| w.text.as_str())
                    .collect::<Vec<_>>()
                    .join(" ");
                let weight = words.iter().map(|w| w.weight).sum::<f32>().max(0.5);
                let char_count = sentence_text.chars().count();
                doc.char_count += char_count;
                let sentence_index = doc.sentences.len();
                doc.sentences.push(Sentence {
                    index: sentence_index,
                    block: block_index,
                    text: sentence_text,
                    words,
                    weight,
                    char_count,
                });
                sentence_indices.push(sentence_index);
            }
            if !sentence_indices.is_empty() {
                doc.blocks.push(Block {
                    kind,
                    sentences: sentence_indices,
                });
            }
        }

        // The first heading names the document. Without one, the caller's
        // label (a file stem, usually) is the honest answer.
        if let Some(first) = doc.blocks.iter().find(|b| b.kind == BlockKind::Heading) {
            if let Some(s) = first.sentences.first() {
                if let Some(sentence) = doc.sentences.get(*s) {
                    let title = sentence
                        .words
                        .iter()
                        .map(|w| w.text.as_str())
                        .collect::<Vec<_>>()
                        .join(" ");
                    if !title.is_empty() {
                        doc.title = title;
                    }
                }
            }
        }

        doc.speech_text = doc
            .sentences
            .iter()
            .map(|s| s.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        doc
    }

    pub fn sentence_first_word(&self, sentence: usize) -> Option<usize> {
        self.sentences.get(sentence).and_then(|s| s.first_word())
    }

    pub fn sentence_of_word(&self, word: usize) -> Option<usize> {
        self.sentences
            .iter()
            .find(|s| s.words.iter().any(|w| w.index == word))
            .map(|s| s.index)
    }
}

// -- blocks -----------------------------------------------------------------

/// Split input into block-level chunks, stripping the Markdown that should not
/// be read aloud (`#`, `**`, link targets, fences) while keeping the prose.
fn split_blocks(input: &str) -> Vec<(BlockKind, String)> {
    let normalized = input.replace("\r\n", "\n").replace('\r', "\n");
    let mut blocks = Vec::new();
    let mut para: Vec<String> = Vec::new();
    let mut para_kind = BlockKind::Paragraph;
    let mut in_fence = false;
    let mut fence: Vec<String> = Vec::new();

    let flush = |blocks: &mut Vec<(BlockKind, String)>,
                 para: &mut Vec<String>,
                 kind: BlockKind| {
        if para.is_empty() {
            return;
        }
        let joined = para.join(" ");
        let cleaned = strip_markup(&joined);
        let cleaned = collapse_spaces(&cleaned);
        if !cleaned.is_empty() {
            blocks.push((kind, cleaned));
        }
        para.clear();
    };

    for line in normalized.lines() {
        let line = line.trim_end();
        let trimmed = line.trim_start();
        let indent = line.len() - trimmed.len();

        // Fenced code is its own block. Fence markers themselves are not prose.
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            if in_fence {
                let text = fence.join(" ");
                let text = collapse_spaces(&text);
                if !text.is_empty() {
                    blocks.push((BlockKind::Code, text));
                }
                fence.clear();
                in_fence = false;
            } else {
                flush(&mut blocks, &mut para, para_kind);
                para_kind = BlockKind::Paragraph;
                in_fence = true;
            }
            continue;
        }
        if in_fence {
            fence.push(line.to_string());
            continue;
        }

        if trimmed.is_empty() {
            flush(&mut blocks, &mut para, para_kind);
            para_kind = BlockKind::Paragraph;
            continue;
        }

        // A horizontal rule is punctuation for the eye, not a sentence.
        let rule_chars: String = trimmed
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();
        if rule_chars.len() >= 3
            && rule_chars
                .chars()
                .all(|c| matches!(c, '-' | '*' | '_' | '='))
        {
            flush(&mut blocks, &mut para, para_kind);
            para_kind = BlockKind::Paragraph;
            continue;
        }

        let heading = markdown_heading(trimmed);
        if let Some(h) = heading {
            flush(&mut blocks, &mut para, para_kind);
            para_kind = BlockKind::Paragraph;
            let cleaned = collapse_spaces(&strip_markup(h));
            if !cleaned.is_empty() {
                blocks.push((BlockKind::Heading, cleaned));
            }
            continue;
        }

        if let Some(q) = trimmed.strip_prefix('>') {
            if para_kind != BlockKind::Quote {
                flush(&mut blocks, &mut para, para_kind);
                para_kind = BlockKind::Quote;
            }
            para.push(q.trim_start().to_string());
            continue;
        }

        if let Some(item) = list_item(trimmed, indent) {
            flush(&mut blocks, &mut para, para_kind);
            para_kind = BlockKind::Paragraph;
            let cleaned = collapse_spaces(&strip_markup(item));
            if !cleaned.is_empty() {
                blocks.push((BlockKind::List, cleaned));
            }
            continue;
        }

        if para_kind != BlockKind::Paragraph {
            flush(&mut blocks, &mut para, para_kind);
            para_kind = BlockKind::Paragraph;
        }
        para.push(trimmed.to_string());
    }

    if in_fence {
        let text = collapse_spaces(&fence.join(" "));
        if !text.is_empty() {
            blocks.push((BlockKind::Code, text));
        }
    }
    flush(&mut blocks, &mut para, para_kind);
    blocks
}

fn markdown_heading(line: &str) -> Option<&str> {
    let hashes = line.chars().take_while(|c| *c == '#').count();
    if (1..=6).contains(&hashes) {
        let rest = &line[hashes..];
        if rest.is_empty() || rest.starts_with(' ') || rest.starts_with('\t') {
            return Some(rest.trim());
        }
    }
    None
}

fn list_item(line: &str, indent: usize) -> Option<&str> {
    if indent > 8 {
        return None;
    }
    for marker in ["- ", "* ", "+ "] {
        if let Some(rest) = line.strip_prefix(marker) {
            return Some(rest.trim());
        }
    }
    // Ordered list: digits, a dot, then a space. Keep the number out of the
    // spoken sentence, but leave the item's own words alone.
    let digits = line.chars().take_while(|c| c.is_ascii_digit()).count();
    if digits > 0 && digits <= 4 {
        let rest = &line[digits..];
        if let Some(item) = rest.strip_prefix(". ") {
            return Some(item.trim());
        }
        if let Some(item) = rest.strip_prefix(") ") {
            return Some(item.trim());
        }
    }
    None
}

/// Remove the small set of Markdown marks that would otherwise be spoken as
/// noise. The goal is readable prose, not a perfect CommonMark parser.
fn strip_markup(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        let c = chars[i];

        // Images and links: keep the visible label, drop the target.
        if c == '!' && i + 1 < chars.len() && chars[i + 1] == '[' {
            if let Some((label, next)) = parse_link(&chars, i + 1) {
                out.push_str(&label);
                i = next;
                continue;
            }
        }
        if c == '[' {
            if let Some((label, next)) = parse_link(&chars, i) {
                out.push_str(&label);
                i = next;
                continue;
            }
        }

        // HTML-ish tags. A bare URL in angle brackets is kept as a URL.
        if c == '<' {
            if let Some(close) = chars[i + 1..].iter().position(|c| *c == '>') {
                let inner: String = chars[i + 1..i + 1 + close].iter().collect();
                if inner.starts_with("http://")
                    || inner.starts_with("https://")
                    || inner.starts_with("mailto:")
                {
                    out.push_str(&inner);
                }
                i += close + 2;
                continue;
            }
        }

        // Emphasis and inline-code marks vanish; the words stay.
        if matches!(c, '*' | '`') {
            i += 1;
            continue;
        }
        if c == '_' {
            let prev_word = i > 0 && chars[i - 1].is_alphanumeric();
            let next_word = i + 1 < chars.len() && chars[i + 1].is_alphanumeric();
            if !(prev_word && next_word) {
                i += 1;
                continue;
            }
        }

        // A backslash-escaped Markdown character is just that character.
        if c == '\\' && i + 1 < chars.len() {
            let next = chars[i + 1];
            if next.is_ascii_punctuation() {
                out.push(next);
                i += 2;
                continue;
            }
        }

        out.push(c);
        i += 1;
    }

    decode_entities(&collapse_spaces(&out))
}

/// Parse `[label](target)` starting at the `[`. Returns the label and the
/// index just past the closing `)`.
fn parse_link(chars: &[char], open: usize) -> Option<(String, usize)> {
    let close = chars[open..].iter().position(|c| *c == ']')? + open;
    if close + 1 >= chars.len() || chars[close + 1] != '(' {
        return None;
    }
    let end = chars[close + 2..].iter().position(|c| *c == ')')? + close + 2;
    let label: String = chars[open + 1..close].iter().collect();
    Some((label, end + 1))
}

fn decode_entities(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
}

fn collapse_spaces(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut space = false;
    for c in s.chars() {
        if c.is_whitespace() {
            space = true;
        } else {
            if space && !out.is_empty() {
                out.push(' ');
            }
            out.push(c);
            space = false;
        }
    }
    out
}

// -- sentences --------------------------------------------------------------

/// Split a block into sentences, returning each sentence's words. Words keep
/// their punctuation; the sentence is the unit Piper synthesises.
fn split_sentences(text: &str) -> Vec<Vec<String>> {
    let words: Vec<&str> = text.split_whitespace().collect();
    let mut sentences: Vec<Vec<String>> = Vec::new();
    let mut current: Vec<String> = Vec::new();
    let mut current_chars = 0usize;

    for (i, raw) in words.iter().enumerate() {
        let word = (*raw).to_string();
        current_chars += word.chars().count() + usize::from(!current.is_empty());
        current.push(word);

        let (strong, period) = sentence_punctuation(raw);
        let mut end = strong;

        if period {
            let core: String = raw
                .chars()
                .filter(|c| c.is_alphanumeric() || *c == '.')
                .collect::<String>()
                .trim_matches('.')
                .to_ascii_lowercase();
            const TITLES: &[&str] = &["dr", "mr", "mrs", "ms", "prof", "st", "jr", "sr", "rev"];
            let is_title = TITLES.contains(&core.as_str());
            if is_title {
                // A title stays attached to the name that follows it.
            } else if is_abbreviation(raw) {
                let next = words.get(i + 1).copied();
                let next_upper = next
                    .and_then(|w| w.chars().find(|c| c.is_alphabetic()))
                    .is_some_and(|c| c.is_uppercase());
                if next_upper || current_chars > 90 {
                    end = true;
                }
            } else {
                let next = words.get(i + 1).copied();
                let next_lower = next
                    .and_then(|w| w.chars().find(|c| c.is_alphabetic()))
                    .is_some_and(|c| c.is_lowercase());
                if !next_lower || current_chars > 90 {
                    end = true;
                }
            }
        }

        // A very long run-on still needs to be chunked so it can be synthesised
        // and followed. Commas are the least destructive place to break.
        if !end
            && (current_chars > 240 || current.len() > 110)
            && raw.ends_with([',', ';', ':'])
        {
            end = true;
        }

        if end {
            sentences.push(std::mem::take(&mut current));
            current_chars = 0;
        }
    }

    if !current.is_empty() {
        sentences.push(current);
    }
    sentences
}

/// `(strong, period)`: strong means `!`, `?`, or an ellipsis; period means the
/// token ends in a full stop. Closing quotes and brackets are ignored.
fn sentence_punctuation(token: &str) -> (bool, bool) {
    let mut t = token.trim_end();
    while let Some(last) = t.chars().last() {
        if matches!(last, '"' | '\'' | ')' | ']' | '}' | '»' | '«' | '“' | '”' | '‘' | '’')
        {
            t = &t[..t.len() - last.len_utf8()];
        } else {
            break;
        }
    }
    let last = match t.chars().last() {
        Some(c) => c,
        None => return (false, false),
    };
    match last {
        '!' | '?' | '。' | '！' | '？' => (true, false),
        '…' => (true, false),
        '.' => {
            // Three or more dots is an ellipsis, which is a stronger pause than
            // a single full stop.
            let dots = t.chars().rev().take_while(|c| *c == '.').count();
            (dots >= 3, true)
        }
        _ => (false, false),
    }
}

fn is_abbreviation(token: &str) -> bool {
    let core: String = token
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '.')
        .collect();
    let core = core.trim_matches('.').to_ascii_lowercase();
    if core.is_empty() {
        return false;
    }

    // Initials and dotted abbreviations: A., z.B., i.e., u.s.a.
    let parts: Vec<&str> = core.split('.').collect();
    if parts.len() > 1 && parts.iter().all(|p| !p.is_empty() && p.chars().count() <= 3) {
        return true;
    }
    if core.chars().count() == 1 {
        return true;
    }

    const ABBREVIATIONS: &[&str] = &[
        "mr", "mrs", "ms", "dr", "prof", "sr", "jr", "st", "vs", "etc", "no", "fig", "figs",
        "vol", "pp", "p", "approx", "dept", "inc", "ltd", "co", "corp", "jan", "feb", "mar",
        "apr", "jun", "jul", "aug", "sep", "sept", "oct", "nov", "dec", "mon", "tue", "wed",
        "thu", "fri", "sat", "sun", "hr", "fr", "nr", "ca", "bzw", "usw", "z.b", "u.a",
        "d.h", "vgl", "ggf", "evtl", "inkl", "sog", "abs", "art", "bd", "bspw", "max", "min",
        "mio", "mrd", "zzt", "geb", "gest", "str", "gmbh", "ag", "kg",
    ];
    ABBREVIATIONS.contains(&core.as_str())
}

/// Distribute a sentence's audio duration using a rough speaking-weight model.
/// Vowel groups are the closest cheap approximation to syllables; punctuation
/// adds a little air. This keeps highlighting in step with the actual WAV, not
/// with a words-per-minute guess.
pub fn speech_weight(token: &str) -> f32 {
    let letters: String = token
        .chars()
        .filter(|c| c.is_alphanumeric() || matches!(c, 'ä' | 'ö' | 'ü' | 'ß'))
        .collect();
    if letters.is_empty() {
        return 0.12;
    }

    let lower = letters.to_lowercase();
    let vowels = "aeiouyàâäáéèêëîïóôöúùû";
    let mut groups = 0usize;
    let mut in_vowel = false;
    for c in lower.chars() {
        let v = vowels.contains(c);
        if v && !in_vowel {
            groups += 1;
        }
        in_vowel = v;
    }
    if groups == 0 {
        // Numbers, acronyms, and consonant-heavy tokens still take time.
        groups = (lower.chars().count() / 3).max(1);
    }

    let mut w = 0.55 + groups as f32 * 0.9 + (lower.chars().count() as f32 * 0.025).min(0.4);
    if token.ends_with([',', ';', ':']) {
        w += 0.18;
    }
    if token.ends_with(['.', '!', '?', '…']) {
        w += 0.32;
    }
    w
}

/// The width a word occupies on screen, honouring double-width glyphs.
pub fn token_width(s: &str) -> usize {
    UnicodeWidthStr::width(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paragraphs_and_headings_become_blocks() {
        let doc = Document::parse(
            "# The Sea\n\nIt was calm. The gulls were loud.\n\n## Night\n\nAnd then dark.",
            None,
            "UNTITLED",
        );
        assert_eq!(doc.title, "The Sea");
        assert_eq!(doc.blocks.len(), 4);
        assert_eq!(doc.blocks[0].kind, BlockKind::Heading);
        assert_eq!(doc.blocks[1].kind, BlockKind::Paragraph);
        assert_eq!(doc.sentences.len(), 5);
    }

    #[test]
    fn abbreviations_do_not_split_sentences() {
        let doc = Document::parse(
            "Dr. Smith arrived at 9 a.m. He sat down. The room was quiet.",
            None,
            "T",
        );
        // "Dr." and "a.m." are abbreviations, so there are three sentences.
        assert_eq!(doc.sentences.len(), 3, "{:?}", doc.sentences);
    }

    #[test]
    fn markdown_marks_are_not_spoken() {
        let doc = Document::parse(
            "A **bold** idea and a [link](https://example.com) walk into a `bar`.",
            None,
            "T",
        );
        let text = &doc.sentences[0].text;
        assert!(text.contains("bold idea"));
        assert!(text.contains("link"));
        assert!(!text.contains("https://"));
        assert!(!text.contains('*'));
        assert!(!text.contains('`'));
    }

    #[test]
    fn word_indices_are_global_and_contiguous() {
        let doc = Document::parse(
            "One two. Three four.\n\nFive six.",
            None,
            "T",
        );
        let mut seen = Vec::new();
        for s in &doc.sentences {
            for w in &s.words {
                seen.push(w.index);
            }
        }
        assert_eq!(seen, (0..doc.word_count).collect::<Vec<_>>());
    }

    #[test]
    fn lists_and_quotes_keep_their_kind() {
        let doc = Document::parse(
            "- first thing\n- second thing\n\n> a quoted line\n> continued",
            None,
            "T",
        );
        assert_eq!(doc.blocks[0].kind, BlockKind::List);
        assert_eq!(doc.blocks[1].kind, BlockKind::List);
        assert_eq!(doc.blocks[2].kind, BlockKind::Quote);
        assert_eq!(doc.blocks[2].sentences.len(), 1);
    }

    #[test]
    fn ellipsis_ends_a_sentence() {
        let doc = Document::parse("Wait… then go. Fine.", None, "T");
        assert_eq!(doc.sentences.len(), 3);
    }

    #[test]
    fn weights_are_positive_and_long_words_weigh_more() {
        assert!(speech_weight("a") > 0.0);
        assert!(speech_weight("extraordinary") > speech_weight("the"));
        assert!(speech_weight("stop.") > speech_weight("stop"));
    }

    #[test]
    fn source_labels_become_titles_without_a_heading() {
        let doc = Document::parse("Just words.", Some("/tmp/notes.md".into()), "NOTES");
        assert_eq!(doc.title, "NOTES");
        assert_eq!(doc.source.as_deref(), Some("/tmp/notes.md"));
    }

    #[test]
    fn code_fences_are_kept_as_one_block() {
        let doc = Document::parse(
            "Before.\n\n```rust\nfn main() {}\n```\n\nAfter.",
            None,
            "T",
        );
        assert_eq!(doc.blocks.len(), 3);
        assert_eq!(doc.blocks[1].kind, BlockKind::Code);
    }
}
