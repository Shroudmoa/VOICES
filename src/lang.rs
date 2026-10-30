//! Languages VOICES knows how to speak.
//!
//! Piper voices are named by a locale prefix (`en_US-…`, `de_DE-…`), so the
//! language is mostly a hint for choosing a voice. It is also used by the
//! lightweight auto-detector when no explicit language is given.

use clap::ValueEnum;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    #[value(alias = "en", alias = "eng", alias = "en-us", alias = "en-gb")]
    #[serde(alias = "en")]
    English,
    #[value(alias = "de", alias = "deu", alias = "ger", alias = "de-de")]
    #[serde(alias = "de")]
    German,
}

impl Language {
    pub fn code(self) -> &'static str {
        match self {
            Language::English => "en",
            Language::German => "de",
        }
    }

    pub fn short(self) -> &'static str {
        match self {
            Language::English => "EN",
            Language::German => "DE",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Language::English => "ENGLISH",
            Language::German => "GERMAN",
        }
    }

    /// The prefix Piper uses in a voice's file name.
    pub fn prefix(self) -> &'static str {
        match self {
            Language::English => "en_",
            Language::German => "de_",
        }
    }

    /// The voice VOICES reaches for first, in preference order.
    pub fn preferred_voices(self) -> &'static [&'static str] {
        match self {
            Language::English => &[
                "en_US-lessac-medium",
                "en_US-amy-medium",
                "en_US-ryan-medium",
                "en_GB-alba-medium",
                "en_GB-alan-medium",
            ],
            Language::German => &[
                "de_DE-thorsten-medium",
                "de_DE-eva_k-x_low",
                "de_DE-kerstin-low",
                "de_DE-ramona-low",
            ],
        }
    }

    /// Infer a language from a Piper model name (`de_DE-thorsten-medium.onnx`).
    pub fn from_model_name(name: &str) -> Option<Language> {
        let lower = name.to_ascii_lowercase();
        if lower.starts_with("de_") || lower.starts_with("de-") {
            Some(Language::German)
        } else if lower.starts_with("en_") || lower.starts_with("en-") {
            Some(Language::English)
        } else {
            None
        }
    }

    /// A small, deliberately conservative language guess.
    ///
    /// It is not a linguistic model. It looks for characters and function words
    /// that are strong evidence of German, then compares that against common
    /// English words. Short or ambiguous prose falls back to English, which is
    /// the least surprising default for an English-language tool.
    pub fn detect(text: &str) -> Language {
        let lower = text.to_lowercase();
        let mut de = 0.0f32;
        let mut en = 0.0f32;

        // Umlauts and the sharp s are near-certain German in normal prose.
        for ch in lower.chars() {
            match ch {
                'ä' | 'ö' | 'ü' | 'ß' => de += 1.5,
                _ => {}
            }
        }

        const DE_WORDS: &[&str] = &[
            "der", "die", "das", "den", "dem", "des", "und", "ist", "sind", "nicht", "ein",
            "eine", "einen", "einem", "einer", "ich", "du", "er", "sie", "es", "wir", "ihr",
            "mit", "sich", "auf", "für", "von", "zu", "zum", "zur", "im", "hat", "haben",
            "wird", "werden", "war", "auch", "aber", "oder", "noch", "nur", "wie", "wenn",
            "dann", "dass", "diese", "dieser", "dieses", "mein", "dein", "kein", "keine",
            "über", "unter", "durch", "gegen", "ohne", "weil", "schon", "sehr", "kann",
        ];
        const EN_WORDS: &[&str] = &[
            "the", "a", "an", "and", "is", "are", "was", "were", "be", "been", "of", "to",
            "in", "on", "at", "for", "with", "from", "by", "as", "that", "this", "these",
            "those", "it", "its", "he", "she", "they", "we", "you", "i", "my", "your",
            "his", "her", "their", "our", "not", "but", "or", "if", "when", "then", "there",
            "here", "what", "which", "who", "how", "why", "can", "could", "would", "should",
            "will", "have", "has", "had", "do", "does", "did", "about", "into", "over",
        ];

        for word in lower
            .split(|c: char| !c.is_alphabetic() && c != 'ä' && c != 'ö' && c != 'ü' && c != 'ß')
            .filter(|w| !w.is_empty())
        {
            if DE_WORDS.contains(&word) {
                de += 1.0;
            }
            if EN_WORDS.contains(&word) {
                en += 1.0;
            }
        }

        if de > en + 1.0 {
            Language::German
        } else {
            Language::English
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn german_prose_is_recognised() {
        let text = "Der Morgen ist still. Die Stadt schläft noch, und über den Dächern liegt Nebel.";
        assert_eq!(Language::detect(text), Language::German);
    }

    #[test]
    fn english_prose_is_recognised() {
        let text = "The morning is quiet. The city is still asleep, and there is fog over the roofs.";
        assert_eq!(Language::detect(text), Language::English);
    }

    #[test]
    fn model_names_carry_their_language() {
        assert_eq!(
            Language::from_model_name("de_DE-thorsten-medium.onnx"),
            Some(Language::German)
        );
        assert_eq!(
            Language::from_model_name("en_US-lessac-medium.onnx"),
            Some(Language::English)
        );
        assert_eq!(Language::from_model_name("voice.onnx"), None);
    }
}
