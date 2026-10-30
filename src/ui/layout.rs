//! Word-wrapped screen geometry for the reader.
//!
//! The layout is built once per width change. It records, for every word, the
//! visual line it landed on, and for every sentence the first and last of those
//! lines. That is all the viewport needs to follow the voice.

use crate::text::{token_width, BlockKind, Document};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TokenKind {
    Word(usize),
    /// A bullet, quote rail, or continuation indent.
    Prefix,
    /// The gap between two words.
    Space,
}

#[derive(Clone, Debug)]
pub struct Token {
    pub text: String,
    pub kind: TokenKind,
}

#[derive(Clone, Debug)]
pub struct VisualLine {
    pub tokens: Vec<Token>,
    /// The sentence the line starts in. Words can belong to later sentences on
    /// the same line; `word_sentence` is the authority for styling.
    pub sentence: Option<usize>,
    pub block: usize,
    pub kind: BlockKind,
}

impl VisualLine {
    fn blank(block: usize, kind: BlockKind) -> VisualLine {
        VisualLine {
            tokens: Vec::new(),
            sentence: None,
            block,
            kind,
        }
    }

    pub fn is_blank(&self) -> bool {
        self.tokens.is_empty()
    }
}

#[derive(Clone, Debug, Default)]
pub struct ReaderLayout {
    pub lines: Vec<VisualLine>,
    /// For each global word index, the visual line it starts on.
    pub word_line: Vec<usize>,
    /// For each global word index, the sentence it belongs to.
    pub word_sentence: Vec<usize>,
    /// For each sentence, the first and last visual line it touches.
    pub sentence_first_line: Vec<usize>,
    pub sentence_last_line: Vec<usize>,
    pub width: usize,
}

impl ReaderLayout {
    pub fn build(doc: &Document, width: usize) -> ReaderLayout {
        let width = width.max(16);
        let mut layout = ReaderLayout {
            lines: Vec::new(),
            word_line: vec![0; doc.word_count],
            word_sentence: vec![0; doc.word_count],
            sentence_first_line: vec![0; doc.sentences.len()],
            sentence_last_line: vec![0; doc.sentences.len()],
            width,
        };

        for (bi, block) in doc.blocks.iter().enumerate() {
            if bi > 0 {
                layout.lines.push(VisualLine::blank(bi, BlockKind::Paragraph));
            }

            // One line buffer per block, so short sentences in a paragraph run
            // together instead of each taking a line of its own.
            let mut line = VisualLine {
                tokens: Vec::new(),
                sentence: None,
                block: bi,
                kind: block.kind,
            };
            let mut line_width = 0usize;
            let mut has_word = false;

            let opening = match block.kind {
                BlockKind::List => "\u{2022} ",
                BlockKind::Quote => "\u{2502} ",
                BlockKind::Code => "  ",
                BlockKind::Paragraph | BlockKind::Heading => "",
            };
            if !opening.is_empty() {
                line.tokens.push(Token {
                    text: opening.to_string(),
                    kind: TokenKind::Prefix,
                });
                line_width += token_width(opening);
            }

            for sentence_index in &block.sentences {
                let Some(sentence) = doc.sentences.get(*sentence_index) else {
                    continue;
                };
                for word in &sentence.words {
                    let w = token_width(&word.text);
                    let need = if has_word { 1 + w } else { w };

                    if has_word && line_width + need > width {
                        layout.lines.push(line);
                        line = VisualLine {
                            tokens: Vec::new(),
                            sentence: Some(sentence.index),
                            block: bi,
                            kind: block.kind,
                        };
                        let cont = match block.kind {
                            BlockKind::List | BlockKind::Quote | BlockKind::Code => "  ",
                            BlockKind::Paragraph | BlockKind::Heading => "",
                        };
                        line_width = 0;
                        has_word = false;
                        if !cont.is_empty() {
                            line.tokens.push(Token {
                                text: cont.to_string(),
                                kind: TokenKind::Prefix,
                            });
                            line_width += token_width(cont);
                        }
                    }

                    if has_word {
                        line.tokens.push(Token {
                            text: " ".to_string(),
                            kind: TokenKind::Space,
                        });
                        line_width += 1;
                    }

                    // A single token wider than the column is rare but not
                    // impossible (URLs, German compounds, CJK). Split it into
                    // chunks that keep the same word index so highlighting
                    // still covers the whole token.
                    let start_line = layout.lines.len();
                    let chunks = split_wide(&word.text, width.saturating_sub(line_width).max(1));
                    for (ci, chunk) in chunks.iter().enumerate() {
                        if ci > 0 {
                            layout.lines.push(line);
                            line = VisualLine {
                                tokens: Vec::new(),
                                sentence: Some(sentence.index),
                                block: bi,
                                kind: block.kind,
                            };
                            line_width = 0;
                            has_word = false;
                        }
                        line.tokens.push(Token {
                            text: chunk.clone(),
                            kind: TokenKind::Word(word.index),
                        });
                        line_width += token_width(chunk);
                        has_word = true;
                    }

                    if line.sentence.is_none() {
                        line.sentence = Some(sentence.index);
                    }
                    let end_line = layout.lines.len();
                    layout.word_line[word.index] = start_line;
                    layout.word_sentence[word.index] = sentence.index;
                    layout.sentence_first_line[sentence.index] = layout.sentence_first_line
                        [sentence.index]
                        .min(start_line);
                    layout.sentence_last_line[sentence.index] =
                        layout.sentence_last_line[sentence.index].max(end_line);
                }
            }

            layout.lines.push(line);
        }

        layout
    }

    pub fn line_of_word(&self, word: usize) -> Option<usize> {
        self.word_line.get(word).copied()
    }

    pub fn sentence_of_word(&self, word: usize) -> Option<usize> {
        self.word_sentence.get(word).copied()
    }

    pub fn sentence_lines(&self, sentence: usize) -> Option<(usize, usize)> {
        let a = *self.sentence_first_line.get(sentence)?;
        let b = *self.sentence_last_line.get(sentence)?;
        Some((a.min(b), a.max(b)))
    }

    /// A line to keep in view: the current word if there is one, otherwise the
    /// start of the current sentence.
    pub fn focus_line(&self, sentence: usize, word: Option<usize>) -> usize {
        if let Some(line) = word.and_then(|w| self.line_of_word(w)) {
            return line;
        }
        self.sentence_lines(sentence)
            .map(|(a, _)| a)
            .unwrap_or(0)
    }
}

/// Split a token whose display width exceeds `room` into pieces. The split
/// respects character boundaries, not grapheme clusters; that is enough for
/// prose and URLs.
fn split_wide(text: &str, room: usize) -> Vec<String> {
    if token_width(text) <= room || room == 0 {
        return vec![text.to_string()];
    }
    let mut out = Vec::new();
    let mut chunk = String::new();
    let mut width = 0;
    for c in text.chars() {
        let w = token_width(&c.to_string());
        if width + w > room && !chunk.is_empty() {
            out.push(std::mem::take(&mut chunk));
            width = 0;
        }
        chunk.push(c);
        width += w;
    }
    if !chunk.is_empty() {
        out.push(chunk);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc() -> Document {
        Document::parse(
            "One two three four five six seven eight nine ten.\n\nA new paragraph.",
            None,
            "T",
        )
    }

    #[test]
    fn every_word_has_a_line_and_lines_are_ordered() {
        let d = doc();
        let l = ReaderLayout::build(&d, 20);
        assert_eq!(l.word_line.len(), d.word_count);
        for pair in l.word_line.windows(2) {
            assert!(pair[0] <= pair[1], "word lines go backwards: {pair:?}");
        }
        assert!(l.lines.len() > 2);
    }

    #[test]
    fn a_sentence_spanning_lines_has_a_range() {
        let d = doc();
        let l = ReaderLayout::build(&d, 20);
        let (a, b) = l.sentence_lines(0).unwrap();
        assert!(b >= a);
        assert!(b > a, "the first sentence should wrap at width 20");
    }

    #[test]
    fn list_items_keep_a_bullet_and_continuation_indent() {
        let d = Document::parse(
            "- a list item long enough to wrap onto another line for sure",
            None,
            "T",
        );
        let l = ReaderLayout::build(&d, 18);
        assert!(l.lines[0].tokens[0].text.starts_with('\u{2022}'));
        assert!(l.lines.len() > 1);
    }

    #[test]
    fn short_sentences_share_a_paragraph_line() {
        let d = Document::parse("One. Two. Three.", None, "T");
        let l = ReaderLayout::build(&d, 60);
        assert_eq!(l.lines.len(), 1);
        assert_eq!(l.sentence_first_line, [0, 0, 0]);
    }

    #[test]
    fn an_overlong_word_is_chunked_without_losing_its_index() {
        let long = "a".repeat(50);
        let d = Document::parse(&format!("{long} tail"), None, "T");
        let l = ReaderLayout::build(&d, 20);
        let chunks: usize = l
            .lines
            .iter()
            .flat_map(|line| &line.tokens)
            .filter(|t| matches!(t.kind, TokenKind::Word(0)))
            .count();
        assert!(chunks > 1);
        assert_eq!(l.word_line[0], 0);
    }
}
