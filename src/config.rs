//! The optional `voices` config file.
//!
//! `$XDG_CONFIG_HOME/voices/config.toml` (or `~/.config/voices/config.toml`)
//! holds the defaults that are awkward to repeat on every invocation: which
//! Piper voice belongs to which language, where the model lives, and what the
//! reader should look and sound like. Every field is optional, and the command
//! line always wins.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::lang::Language;
use crate::theme::ThemeName;
use crate::util;

pub const APP_DIR: &str = "voices";
pub const CONFIG_FILE: &str = "config.toml";

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Config {
    /// `english`, `german`, or omitted to auto-detect.
    pub language: Option<Language>,
    /// A Piper model path, or a model name to look up in the voice directories.
    pub voice: Option<String>,
    /// Playback speed. 1.0 is the voice's natural pace.
    pub speed: Option<f32>,
    /// `dark` or `light`.
    pub theme: Option<ThemeName>,
    /// The Piper executable, or a name looked up on `PATH`.
    pub piper: Option<PathBuf>,
    /// Extra directory to search for `.onnx` voices.
    pub model_dir: Option<PathBuf>,
    /// The audio player command, e.g. `paplay` or `ffplay -nodisp`.
    pub player: Option<String>,
    /// How many sentences to synthesise ahead of playback.
    pub lookahead: Option<usize>,
    /// Silence Piper inserts between its own sentences, in seconds.
    pub sentence_silence: Option<f32>,
}

impl Config {
    /// Load the config named by `explicit`, or the default one if it exists.
    ///
    /// A missing default config is normal and silent. A missing *explicit*
    /// config is an error, because the user asked for that exact file.
    pub fn load(explicit: Option<&Path>) -> Result<(Config, Option<PathBuf>)> {
        let path = match explicit {
            Some(p) => Some(util::expand_tilde(p)),
            None => default_path(),
        };
        let Some(path) = path else {
            return Ok((Config::default(), None));
        };
        if !path.exists() {
            if explicit.is_some() {
                anyhow::bail!("config file not found: {}", path.display());
            }
            return Ok((Config::default(), None));
        }
        let body = std::fs::read_to_string(&path)
            .with_context(|| format!("could not read config {}", path.display()))?;
        let cfg: Config = toml::from_str(&body)
            .with_context(|| format!("could not parse config {}", path.display()))?;
        Ok((cfg, Some(path)))
    }
}

pub fn default_path() -> Option<PathBuf> {
    util::config_home().map(|d| d.join(APP_DIR).join(CONFIG_FILE))
}

pub fn ensure_dir() -> Option<PathBuf> {
    let d = util::config_home()?.join(APP_DIR);
    let _ = std::fs::create_dir_all(&d);
    Some(d)
}

/// A ready-to-edit config file, printed by `voices --sample-config`.
pub fn sample() -> String {
    r#"# voices — configuration
#
# Everything here is optional. Command-line flags override these values.

# "english" or "german". Omit to auto-detect from the text.
# language = "english"

# A Piper voice: either a model name found in a voice directory, or a path.
# voice = "en_US-lessac-medium"

# Playback speed. 0.5 is slow, 1.0 is natural, 2.0 is brisk.
# speed = 1.0

# "dark" or "light".
# theme = "dark"

# The Piper executable (default: "piper" on PATH).
# piper = "piper"

# Extra directory to search for *.onnx voices.
# On Linux, ~/.local/share/piper is searched automatically.
# model_dir = "~/.local/share/piper"

# The audio player. VOICES auto-detects paplay, pw-play, aplay, ffplay,
# play, or afplay. A custom command may contain {file}.
# player = "paplay"
# player = "ffplay -nodisp -autoexit -loglevel quiet {file}"

# Sentences to synthesise ahead of the one being spoken (default: 2).
# lookahead = 2

# Silence inserted between sentences by Piper, in seconds (default: 0.12).
# sentence_silence = 0.12
"#
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_config_has_no_overrides() {
        let c: Config = toml::from_str("").unwrap();
        assert!(c.language.is_none());
        assert!(c.voice.is_none());
    }

    #[test]
    fn aliases_work_for_language_and_theme() {
        let c: Config = toml::from_str("language = \"de\"\ntheme = \"light\"").unwrap();
        assert_eq!(c.language, Some(Language::German));
        assert_eq!(c.theme, Some(ThemeName::Light));
    }

    #[test]
    fn sample_config_is_valid_toml() {
        let c: Config = toml::from_str(&sample()).expect("sample parses");
        assert!(c.language.is_none());
    }
}
