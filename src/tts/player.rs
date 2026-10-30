//! Small wrapper around the external audio player Piper hands its WAV to.
//!
//! Piper writes files; it does not play them. Rather than link an audio
//! backend that may not be installed, VOICES hands the file to one of the
//! players a modern Linux box already has. Pausing is done with SIGSTOP/SIGCONT,
//! which every Unix player tolerates.

use std::path::Path;
use std::process::{Child, Command, Stdio};

use crate::util;

#[derive(Clone, Debug)]
pub struct Player {
    pub program: String,
    pub args: Vec<String>,
}

impl Player {
    /// Resolve a player from a config string or from the environment.
    pub fn resolve(custom: Option<&str>) -> anyhow::Result<Player> {
        if let Some(custom) = custom {
            let mut parts = util::split_command(custom);
            if parts.is_empty() {
                anyhow::bail!("empty player command");
            }
            let program = parts.remove(0);
            if util::find_executable(&program).is_none() && Path::new(&program).is_file() == false {
                anyhow::bail!("player not found: {program}");
            }
            return Ok(Player {
                program,
                args: parts,
            });
        }

        const CANDIDATES: &[(&str, &[&str])] = &[
            ("paplay", &[]),
            ("pw-play", &[]),
            ("aplay", &["-q"]),
            ("ffplay", &["-nodisp", "-autoexit", "-loglevel", "quiet"]),
            ("play", &["-q"]),
            ("mpv", &["--no-video", "--really-quiet"]),
            ("afplay", &[]),
        ];
        for (name, args) in CANDIDATES {
            if util::find_executable(name).is_some() {
                return Ok(Player {
                    program: (*name).to_string(),
                    args: args.iter().map(|s| s.to_string()).collect(),
                });
            }
        }
        anyhow::bail!(
            "no audio player found on PATH. Install one (paplay, pw-play, aplay, \
             ffplay, play, or afplay) or set `player` in the config."
        )
    }

    pub fn name(&self) -> &str {
        &self.program
    }

    /// Spawn a player for `wav`. The `{file}` placeholder, if present, is
    /// replaced with the path; otherwise the path is appended.
    pub fn spawn(&self, wav: &Path) -> anyhow::Result<Child> {
        let mut cmd = Command::new(&self.program);
        for arg in &self.args {
            if arg == "{file}" {
                cmd.arg(wav);
            } else {
                cmd.arg(arg);
            }
        }
        if !self.args.iter().any(|a| a == "{file}") {
            cmd.arg(wav);
        }
        cmd.stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        Ok(cmd.spawn()?)
    }

    pub fn suspend(child: &Child) {
        #[cfg(unix)]
        {
            unsafe {
                libc::kill(child.id() as i32, libc::SIGSTOP);
            }
        }
        let _ = child;
    }

    pub fn resume(child: &Child) {
        #[cfg(unix)]
        {
            unsafe {
                libc::kill(child.id() as i32, libc::SIGCONT);
            }
        }
        let _ = child;
    }

    pub fn kill(child: &mut Child) {
        let _ = child.kill();
        let _ = child.wait();
    }
}
