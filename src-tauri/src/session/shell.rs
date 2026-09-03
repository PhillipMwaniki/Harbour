//! Enumeration of the local shells Harbour can attach a pty to.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ShellFamily {
    Windows,
    Wsl,
    Unix,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShellSpec {
    /// Stable identifier, e.g. `pwsh`, `cmd`, `wsl:Ubuntu-24.04`, `bash`.
    pub id: String,
    pub label: String,
    pub program: String,
    pub args: Vec<String>,
    pub family: ShellFamily,
    /// True for the shell offered when the user just hits "new local tab".
    pub default: bool,
}

/// Lists the shells available on this machine, best default first.
pub fn detect() -> Vec<ShellSpec> {
    let mut shells = if cfg!(windows) {
        detect_windows()
    } else {
        detect_unix()
    };
    if let Some(first) = shells.first_mut() {
        first.default = true;
    }
    shells
}

pub fn find(id: &str) -> Option<ShellSpec> {
    detect().into_iter().find(|s| s.id == id)
}

// ---------------------------------------------------------------------------
// Windows
// ---------------------------------------------------------------------------

fn detect_windows() -> Vec<ShellSpec> {
    let mut out = Vec::new();

    if let Some(path) = which("pwsh.exe") {
        out.push(ShellSpec {
            id: "pwsh".into(),
            label: "PowerShell 7".into(),
            program: path.to_string_lossy().into_owned(),
            args: vec!["-NoLogo".into()],
            family: ShellFamily::Windows,
            default: false,
        });
    }
    if let Some(path) = which("powershell.exe") {
        out.push(ShellSpec {
            id: "powershell".into(),
            label: "Windows PowerShell".into(),
            program: path.to_string_lossy().into_owned(),
            args: vec!["-NoLogo".into()],
            family: ShellFamily::Windows,
            default: false,
        });
    }
    if let Some(path) = which("cmd.exe") {
        out.push(ShellSpec {
            id: "cmd".into(),
            label: "Command Prompt".into(),
            program: path.to_string_lossy().into_owned(),
            args: vec![],
            family: ShellFamily::Windows,
            default: false,
        });
    }

    let git_bash = PathBuf::from(r"C:\Program Files\Git\bin\bash.exe");
    if git_bash.is_file() {
        out.push(ShellSpec {
            id: "git-bash".into(),
            label: "Git Bash".into(),
            program: git_bash.to_string_lossy().into_owned(),
            args: vec!["-i".into(), "-l".into()],
            family: ShellFamily::Unix,
            default: false,
        });
    }

    out.extend(detect_wsl());
    out
}

fn detect_wsl() -> Vec<ShellSpec> {
    let Some(wsl) = which("wsl.exe") else {
        return Vec::new();
    };
    let Ok(output) = std::process::Command::new(&wsl)
        .args(["--list", "--quiet"])
        .output()
    else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }

    parse_wsl_distros(&output.stdout)
        .into_iter()
        .map(|distro| ShellSpec {
            id: format!("wsl:{distro}"),
            label: format!("WSL - {distro}"),
            program: wsl.to_string_lossy().into_owned(),
            // No command after the distro: wsl.exe then starts that distro's
            // default login shell as the distro's default user.
            args: vec!["--distribution".into(), distro],
            family: ShellFamily::Wsl,
            default: false,
        })
        .collect()
}

/// `wsl.exe --list --quiet` writes UTF-16LE, one distro per line.
pub fn parse_wsl_distros(raw: &[u8]) -> Vec<String> {
    let text = decode_console_output(raw);
    text.lines()
        .map(|line| line.trim_matches(|c: char| c.is_whitespace() || c == '\u{0}'))
        .filter(|line| !line.is_empty())
        .map(|line| line.to_string())
        .collect()
}

/// Decodes console output that may be either UTF-16LE (most `wsl.exe` builds)
/// or plain UTF-8 (some newer ones).
pub fn decode_console_output(raw: &[u8]) -> String {
    // ASCII text encoded as UTF-16LE always has NUL in every second byte.
    let looks_utf16 = raw.len() >= 2 && raw.iter().skip(1).step_by(2).take(8).any(|b| *b == 0);
    if !looks_utf16 {
        return String::from_utf8_lossy(raw).into_owned();
    }
    let units: Vec<u16> = raw
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect();
    String::from_utf16_lossy(&units)
}

// ---------------------------------------------------------------------------
// Unix
// ---------------------------------------------------------------------------

fn detect_unix() -> Vec<ShellSpec> {
    let mut out: Vec<ShellSpec> = Vec::new();
    let mut seen: Vec<String> = Vec::new();

    let push = |path: PathBuf, out: &mut Vec<ShellSpec>, seen: &mut Vec<String>| {
        let program = path.to_string_lossy().into_owned();
        if seen.contains(&program) || !path.is_file() {
            return;
        }
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| program.clone());
        seen.push(program.clone());
        out.push(ShellSpec {
            id: name.clone(),
            label: name,
            program,
            args: vec!["-l".into()],
            family: ShellFamily::Unix,
            default: false,
        });
    };

    if let Ok(shell) = std::env::var("SHELL") {
        push(PathBuf::from(shell), &mut out, &mut seen);
    }
    for candidate in ["/bin/zsh", "/bin/bash", "/usr/bin/fish", "/bin/sh"] {
        push(PathBuf::from(candidate), &mut out, &mut seen);
    }
    out
}

// ---------------------------------------------------------------------------
// PATH lookup
// ---------------------------------------------------------------------------

/// Minimal `which`: resolves `name` against PATH. On Windows the name is
/// expected to already carry its extension.
pub fn which(name: &str) -> Option<PathBuf> {
    let direct = Path::new(name);
    if direct.is_absolute() {
        return direct.is_file().then(|| direct.to_path_buf());
    }
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utf16le(text: &str) -> Vec<u8> {
        text.encode_utf16()
            .flat_map(|ch| ch.to_le_bytes())
            .collect()
    }

    #[test]
    fn parses_utf16le_wsl_output() {
        let raw = utf16le("Ubuntu\r\nDebian\r\n");
        assert_eq!(parse_wsl_distros(&raw), vec!["Ubuntu", "Debian"]);
    }

    #[test]
    fn parses_utf8_wsl_output() {
        assert_eq!(
            parse_wsl_distros(b"Ubuntu\nDebian\n"),
            vec!["Ubuntu", "Debian"]
        );
    }

    #[test]
    fn ignores_blank_and_whitespace_only_lines() {
        assert_eq!(
            parse_wsl_distros(b"Ubuntu\n\n  \nDebian\n"),
            vec!["Ubuntu", "Debian"]
        );
    }

    #[test]
    fn keeps_distro_names_containing_dots_and_dashes() {
        let raw = utf16le("Ubuntu-24.04\r\n");
        assert_eq!(parse_wsl_distros(&raw), vec!["Ubuntu-24.04"]);
    }

    #[test]
    fn detect_returns_at_least_one_shell_with_exactly_one_default() {
        let shells = detect();
        assert!(!shells.is_empty(), "every supported platform has a shell");
        assert_eq!(shells.iter().filter(|s| s.default).count(), 1);
    }

    #[test]
    fn shell_ids_are_unique() {
        let shells = detect();
        let mut ids: Vec<&str> = shells.iter().map(|s| s.id.as_str()).collect();
        ids.sort_unstable();
        let before = ids.len();
        ids.dedup();
        assert_eq!(before, ids.len(), "duplicate shell id in {shells:?}");
    }

    #[test]
    fn find_resolves_a_detected_shell_and_rejects_unknown_ones() {
        let first = detect().first().cloned().expect("a shell exists");
        assert_eq!(find(&first.id).map(|s| s.program), Some(first.program));
        assert!(find("definitely-not-a-shell").is_none());
    }
}
