//! Command-line interface. Kept deliberately close to WORDS's own syntax.

use clap::{Parser, ValueEnum};
use std::path::PathBuf;

use crate::config::Config;
use crate::lang::Language;
use crate::theme::ThemeName;

#[derive(Parser, Debug)]
#[command(
    name = "voices",
    version,
    about = "A focused terminal reader that speaks: offline neural TTS with live word highlighting",
    long_about = "VOICES reads a text or Markdown document aloud with Piper, a high-quality \
offline neural voice, and highlights the word being spoken as it goes. \
Everything runs in the terminal with ratatui, and the controls are a single \
keystroke away.",
    after_help = "KEYS\n  space     pause / resume\n  s         stop (rewind the current sentence)\n  left/right previous / next sentence\n  up/down   faster / slower\n  t         toggle dark / light\n  ?  h      controls\n  q  esc    quit\n\nEXAMPLES\n  voices essay.md\n  voices --lang de --voice de_DE-thorsten-medium essay.md\n  voices --no-play --theme light essay.md\n  voices --output essay.mp3 essay.md\n  echo \"Hello world.\" | voices\n  voices --list-voices"
)]
pub struct Cli {
    /// A file to read, or raw text when the path does not exist.
    #[arg(value_name = "INPUT")]
    pub input: Option<String>,

    /// Read raw text from this option instead of a file.
    #[arg(short = 't', long = "text")]
    pub text: Option<String>,

    /// Write the whole reading to a file (.wav or .mp3) and exit.
    #[arg(short = 'o', long = "output")]
    pub output: Option<PathBuf>,

    /// Language: english or german (default: auto-detect).
    #[arg(short = 'l', long = "lang", value_enum)]
    pub lang: Option<Language>,

    /// A Piper voice: model name or path to an .onnx file.
    #[arg(short = 'v', long = "voice")]
    pub voice: Option<String>,

    /// Playback speed (0.4 .. 4.0, 1.0 is natural).
    #[arg(short = 's', long = "speed")]
    pub speed: Option<f32>,

    /// Extra directory to search for .onnx voices.
    #[arg(long = "model-dir")]
    pub model_dir: Option<PathBuf>,

    /// The Piper executable (default: "piper" on PATH).
    #[arg(long = "piper")]
    pub piper: Option<PathBuf>,

    /// The audio player (default: auto-detect).
    #[arg(long = "player")]
    pub player: Option<String>,

    /// Theme: dark or light.
    #[arg(long = "theme", value_enum)]
    pub theme: Option<ThemeName>,

    /// Start in the light theme.
    #[arg(long = "light")]
    pub light: bool,

    /// Open the reader without speaking (handy for previewing).
    #[arg(long = "no-play")]
    pub no_play: bool,

    /// List the Piper voices found in the usual places and exit.
    #[arg(long = "list-voices")]
    pub list_voices: bool,

    /// Print a sample config file and exit.
    #[arg(long = "sample-config")]
    pub sample_config: bool,

    /// Path to a config file (default: ~/.config/voices/config.toml).
    #[arg(long = "config")]
    pub config: Option<PathBuf>,

    /// Synthesise ahead by this many sentences.
    #[arg(long = "lookahead")]
    pub lookahead: Option<usize>,

    /// Silence between sentences, in seconds.
    #[arg(long = "sentence-silence")]
    pub sentence_silence: Option<f32>,

    /// Read phrases from a pipe in addition to the input (WORDS-style).
    #[arg(short = 'i', long = "stdin")]
    pub stdin: bool,

    /// Seed for the reader's idle animation (unused currently, kept for WORDS parity).
    #[arg(long = "seed")]
    pub seed: Option<u64>,
}

impl Cli {
    pub fn config_path(&self) -> Option<PathBuf> {
        self.config.clone()
    }
}

pub fn load_config(cli: &Cli) -> anyhow::Result<(Config, Option<PathBuf>)> {
    Config::load(cli.config_path().as_deref())
}
