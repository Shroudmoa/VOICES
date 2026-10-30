//! Export the whole reading as a WAV or MP3.

use std::path::Path;

use crate::config::Config;
use crate::lang::Language;
use crate::text::Document;
use crate::tts::piper::{PiperConfig, synthesise};

pub fn run(
    doc: &Document,
    cfg: PiperConfig,
    output: &Path,
    speed: f32,
) -> anyhow::Result<()> {
    let ext = output
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();
    if ext != "wav" && ext != "mp3" {
        anyhow::bail!("output file must end in .wav or .mp3");
    }

    eprintln!("voices: synthesising {} words\u{2026}", doc.word_count);

    let mut cfg = cfg;
    cfg.length_scale = 1.0 / speed.clamp(0.4, 4.0);

    let tmp = tempfile::tempdir()?;
    let wav = tmp.path().join("export.wav");
    let duration = synthesise(&cfg, &doc.speech_text, &wav)?;
    eprintln!("voices: wrote {:.1}s of audio", duration);

    match ext.as_str() {
        "wav" => {
            std::fs::copy(&wav, output)?;
        }
        "mp3" => {
            let ffmpeg = crate::util::find_executable("ffmpeg")
                .ok_or_else(|| anyhow::anyhow!("ffmpeg not found on PATH (needed for .mp3 export)"))?;
            let status = std::process::Command::new(ffmpeg)
                .arg("-y")
                .arg("-i")
                .arg(&wav)
                .arg("-codec:a")
                .arg("libmp3lame")
                .arg("-qscale:a")
                .arg("2")
                .arg(output)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()?;
            if !status.success() {
                anyhow::bail!("ffmpeg failed to encode the mp3");
            }
        }
        _ => unreachable!(),
    }

    let size = std::fs::metadata(output)?.len();
    eprintln!("voices: wrote {} ({} bytes)", output.display(), size);
    Ok(())
}

pub fn resolve_config(
    language: Language,
    voice: Option<&str>,
    model_dir: Option<&Path>,
    piper: Option<&Path>,
    config: &Config,
) -> anyhow::Result<PiperConfig> {
    PiperConfig::resolve(language, voice, model_dir, piper, config)
}
