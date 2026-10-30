//! Calling the standalone Piper binary.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use crate::util;

#[derive(Clone, Debug)]
pub struct PiperConfig {
    pub binary: PathBuf,
    pub model: PathBuf,
    pub length_scale: f32,
    pub sentence_silence: f32,
}

impl PiperConfig {
    /// Find the Piper binary and a voice for `language`. `voice_arg` may be a
    /// path, a model name, or `None` to pick a preferred voice automatically.
    pub fn resolve(
        language: crate::lang::Language,
        voice_arg: Option<&str>,
        model_dir: Option<&Path>,
        piper_override: Option<&Path>,
        config: &crate::config::Config,
    ) -> anyhow::Result<PiperConfig> {
        let binary = resolve_binary(piper_override.or(config.piper.as_deref()))?;

        let dirs = model_dirs(model_dir, config);
        let model = resolve_model(language, voice_arg, config.voice.as_deref(), &dirs)?;

        let length_scale = config.speed.unwrap_or(1.0).clamp(0.25, 4.0);
        let length_scale = 1.0 / length_scale.clamp(0.25, 4.0);
        let sentence_silence = config.sentence_silence.unwrap_or(0.12).clamp(0.0, 1.0);

        Ok(PiperConfig {
            binary,
            model,
            length_scale,
            sentence_silence,
        })
    }

    pub fn voice_name(&self) -> String {
        self.model
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "piper".to_string())
    }

    /// The config file Piper expects next to the model, if present.
    pub fn config_file(&self) -> Option<PathBuf> {
        let p = PathBuf::from(format!("{}.json", self.model.display()));
        if p.exists() {
            Some(p)
        } else {
            None
        }
    }
}

fn resolve_binary(override_path: Option<&Path>) -> anyhow::Result<PathBuf> {
    if let Some(p) = override_path {
        let p = util::expand_tilde(p);
        if !util::is_executable(&p) {
            anyhow::bail!("Piper binary not found or not executable: {}", p.display());
        }
        return Ok(p);
    }
    if let Some(p) = util::find_executable("piper") {
        return Ok(p);
    }
    if let Some(p) = util::find_executable("piper-tts") {
        return Ok(p);
    }
    anyhow::bail!(
        "Piper binary not found on PATH. Install Piper (https://github.com/rhasspy/piper) \
         or set `piper` in the config / --piper."
    )
}

fn model_dirs(extra: Option<&Path>, config: &crate::config::Config) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(d) = extra {
        dirs.push(util::expand_tilde(d));
    }
    if let Some(d) = &config.model_dir {
        dirs.push(util::expand_tilde(d));
    }
    if let Some(d) = std::env::var_os("PIPER_MODEL_DIR") {
        dirs.push(PathBuf::from(d));
    }
    if let Some(d) = std::env::var_os("VOICES_MODEL_DIR") {
        dirs.push(PathBuf::from(d));
    }
    if let Some(d) = crate::util::data_home() {
        dirs.push(d.join("piper"));
        dirs.push(d.join("piper").join("voices"));
        dirs.push(d.join("voices"));
    }
    dirs.push(PathBuf::from("models"));
    dirs.push(PathBuf::from("voices"));
    // De-duplicate, keep order.
    let mut seen = std::collections::HashSet::new();
    dirs.retain(|d| seen.insert(d.clone()));
    dirs
}

fn resolve_model(
    language: crate::lang::Language,
    voice_arg: Option<&str>,
    config_voice: Option<&str>,
    dirs: &[PathBuf],
) -> anyhow::Result<PathBuf> {
    // An explicit path wins outright.
    if let Some(v) = voice_arg.or(config_voice) {
        let p = util::expand_tilde(Path::new(v));
        if p.exists() {
            return Ok(p);
        }
        // Treat it as a name and search the voice directories.
        if let Some(found) = find_by_name(v, dirs) {
            return Ok(found);
        }
        anyhow::bail!(
            "Piper voice not found: {v}. Looked in:\n  {}",
            dirs.iter().map(|d| d.display().to_string()).collect::<Vec<_>>().join("\n  ")
        );
    }

    // No voice named: pick a preferred voice for the language, then any voice
    // whose file name starts with the language prefix.
    for preferred in language.preferred_voices() {
        if let Some(found) = find_by_name(preferred, dirs) {
            return Ok(found);
        }
    }
    for dir in dirs {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("onnx") {
                    continue;
                }
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    if name.to_ascii_lowercase().starts_with(language.prefix()) {
                        return Ok(path);
                    }
                }
            }
        }
    }
    anyhow::bail!(
        "No Piper voice found for {} ({}). Download one into ~/.local/share/piper, \
         for example ~/.local/share/piper/{}-…-medium.onnx, or pass --voice <path>.\n\
         Searched:\n  {}",
        language.label(),
        language.code(),
        language.code().replace("en", "en_US").replace("de", "de_DE"),
        dirs.iter().map(|d| d.display().to_string()).collect::<Vec<_>>().join("\n  ")
    )
}

fn find_by_name(name: &str, dirs: &[PathBuf]) -> Option<PathBuf> {
    let wanted = if name.ends_with(".onnx") {
        name.to_string()
    } else {
        format!("{name}.onnx")
    };
    for dir in dirs {
        let direct = dir.join(&wanted);
        if direct.exists() {
            return Some(direct);
        }
        // One level of nesting, for archives that unpack into a subdirectory.
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let p = entry.path().join(&wanted);
                if p.exists() {
                    return Some(p);
                }
            }
        }
    }
    None
}

/// Scan a handful of directories for Piper voices, for --list-voices.
pub fn list_voices(dirs: &[PathBuf]) -> Vec<(crate::lang::Language, PathBuf)> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for dir in dirs {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("onnx") {
                    continue;
                }
                if !seen.insert(path.clone()) {
                    continue;
                }
                let name = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("");
                let language = crate::lang::Language::from_model_name(name)
                    .unwrap_or(crate::lang::Language::English);
                out.push((language, path));
            }
        }
    }
    out.sort_by(|a, b| a.1.cmp(&b.1));
    out
}

/// Synthesise `text` to `output`. Returns the WAV's duration.
pub fn synthesise(cfg: &PiperConfig, text: &str, output: &Path) -> anyhow::Result<f64> {
    let mut cmd = Command::new(&cfg.binary);
    cmd.arg("--model").arg(&cfg.model);
    if let Some(config) = cfg.config_file() {
        cmd.arg("--config").arg(config);
    }
    cmd.arg("--output_file").arg(output);
    cmd.arg("--length_scale").arg(format!("{:.3}", cfg.length_scale));
    cmd.arg("--sentence_silence").arg(format!("{:.3}", cfg.sentence_silence));
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());

    let mut child = cmd.spawn()?;
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(text.as_bytes());
        let _ = stdin.write_all(b"\n");
        let _ = stdin.flush();
        drop(stdin);
    }

    let result = child.wait_with_output()?;
    if !result.status.success() {
        let err = String::from_utf8_lossy(&result.stderr);
        let snippet = err.trim().chars().take(600).collect::<String>();
        anyhow::bail!(
            "Piper failed ({}): {}",
            result.status,
            if snippet.is_empty() { "no error output" } else { &snippet }
        );
    }
    crate::tts::wav::duration(output)
}
