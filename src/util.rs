//! Small filesystem and environment helpers shared by the rest of the app.
//!
//! VOICES deliberately avoids a directory crate and a shell-words crate: the
//! handful of things it needs are short enough to keep visible and dependency
//! free.

use std::path::{Path, PathBuf};

/// The user's home directory, if the environment names one.
pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .filter(|h| !h.is_empty())
        .map(PathBuf::from)
}

/// `$XDG_CONFIG_HOME`, or `~/.config`.
pub fn config_home() -> Option<PathBuf> {
    if let Some(x) = std::env::var_os("XDG_CONFIG_HOME") {
        if !x.is_empty() {
            return Some(PathBuf::from(x));
        }
    }
    home_dir().map(|h| h.join(".config"))
}

/// `$XDG_DATA_HOME`, or `~/.local/share`.
pub fn data_home() -> Option<PathBuf> {
    if let Some(x) = std::env::var_os("XDG_DATA_HOME") {
        if !x.is_empty() {
            return Some(PathBuf::from(x));
        }
    }
    home_dir().map(|h| h.join(".local").join("share"))
}

/// Expand a leading `~` so config files can use the familiar shorthand.
pub fn expand_tilde(path: &Path) -> PathBuf {
    let Some(s) = path.to_str() else {
        return path.to_path_buf();
    };
    if s == "~" {
        return home_dir().unwrap_or_else(|| path.to_path_buf());
    }
    if let Some(rest) = s.strip_prefix("~/") {
        if let Some(home) = home_dir() {
            return home.join(rest);
        }
    }
    path.to_path_buf()
}

/// Look a program up on `PATH`. An explicit path is accepted too.
pub fn find_executable(name: &str) -> Option<PathBuf> {
    let p = Path::new(name);
    if p.components().count() > 1 || p.is_absolute() {
        return is_executable(p).then(|| p.to_path_buf());
    }
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(name))
        .find(|candidate| is_executable(candidate))
}

pub fn is_executable(path: &Path) -> bool {
    let Ok(md) = std::fs::metadata(path) else {
        return false;
    };
    if !md.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        md.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

/// Split a command line from the config into a program and its arguments.
///
/// This is intentionally simple: whitespace separates words, and single or
/// double quotes group a word containing spaces. There is no expansion and no
/// escaping beyond a backslash inside a double-quoted word.
pub fn split_command(input: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut word = String::new();
    let mut quote: Option<char> = None;
    let mut escaped = false;
    let mut started = false;

    for ch in input.chars() {
        if escaped {
            word.push(ch);
            escaped = false;
            started = true;
            continue;
        }
        match (quote, ch) {
            (Some('"'), '\\') => {
                escaped = true;
                started = true;
            }
            (Some(q), c) if c == q => {
                quote = None;
                started = true;
            }
            (None, '\'') | (None, '"') => {
                quote = Some(ch);
                started = true;
            }
            (None, c) if c.is_whitespace() => {
                if started {
                    out.push(std::mem::take(&mut word));
                    started = false;
                }
            }
            (_, c) => {
                word.push(c);
                started = true;
            }
        }
    }
    if started {
        out.push(word);
    }
    out
}

/// `foo.txt` -> `FOO`; used for document titles.
pub fn file_label(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().trim().to_string())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_uppercase())
        .unwrap_or_else(|| "UNTITLED".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_split_with_quotes() {
        assert_eq!(
            split_command("ffplay -nodisp -autoexit"),
            ["ffplay", "-nodisp", "-autoexit"]
        );
        assert_eq!(
            split_command("my player --title \"a b\""),
            ["my", "player", "--title", "a b"]
        );
    }
}
