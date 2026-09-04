//! Portable mode: everything Harbour writes lives beside the executable.
//!
//! A normal install keeps its vault, settings, logs and known_hosts in the
//! OS's per-user config and log directories. That is the right default, but it
//! ties the data to the machine. Portable mode is the other choice: drop a
//! marker next to the executable and Harbour keeps everything in a `data`
//! folder beside itself instead - a copy on a USB stick that leaves no trace on
//! the host, carries its own hosts, and, because there is no per-user keychain
//! to reach for, keeps its secrets in the master-password file.
//!
//! The trigger is deliberately a file, not a setting: a setting would have to
//! live somewhere, and where it lived is the very question. A `portable` (or
//! `portable.txt`) file next to the executable is unambiguous and travels with
//! the copy. `HARBOUR_PORTABLE` in the environment forces it on for a dev build
//! or a test, where the executable is buried in `target/`.

use std::path::{Path, PathBuf};

/// The names that, next to the executable, turn portable mode on. `.txt` is
/// there because Windows hides extensions and a user making the file by hand
/// tends to get a `.txt` whether they meant to or not.
const MARKERS: [&str; 2] = ["portable", "portable.txt"];

/// Where portable data lives relative to the marker: a `data` folder beside the
/// executable, so the copy is self-contained and the folder is obvious.
const DATA_DIR: &str = "data";

/// Decides whether Harbour is running portable, and if so where its data goes.
///
/// Portable when `HARBOUR_PORTABLE` is set to something truthy, or when a marker
/// file sits next to the executable. Returns the base directory everything else
/// hangs off - `<exe dir>/data` - or `None` for a normal install.
pub fn base_dir() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let exe_dir = exe.parent()?;
    resolve(exe_dir, std::env::var("HARBOUR_PORTABLE").ok().as_deref())
}

/// The decision, factored out so it can be tested without a real executable.
fn resolve(exe_dir: &Path, env: Option<&str>) -> Option<PathBuf> {
    if is_truthy(env) || MARKERS.iter().any(|m| exe_dir.join(m).exists()) {
        Some(exe_dir.join(DATA_DIR))
    } else {
        None
    }
}

/// An environment variable counts as "on" unless it is absent, empty, or an
/// explicit off value, so `HARBOUR_PORTABLE=0` does not accidentally enable it.
fn is_truthy(value: Option<&str>) -> bool {
    match value.map(str::trim) {
        None | Some("") | Some("0") | Some("false") | Some("no") => false,
        Some(_) => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("harbour-portable-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_plain_directory_is_not_portable() {
        let dir = temp_dir();
        assert_eq!(resolve(&dir, None), None);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_marker_file_beside_the_executable_turns_it_on() {
        let dir = temp_dir();
        std::fs::write(dir.join("portable"), b"").unwrap();
        assert_eq!(resolve(&dir, None), Some(dir.join("data")));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_txt_variant_of_the_marker_also_counts() {
        let dir = temp_dir();
        std::fs::write(dir.join("portable.txt"), b"").unwrap();
        assert_eq!(resolve(&dir, None), Some(dir.join("data")));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_environment_forces_it_on_without_a_marker() {
        let dir = temp_dir();
        assert_eq!(resolve(&dir, Some("1")), Some(dir.join("data")));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn an_explicit_off_value_does_not_enable_it() {
        let dir = temp_dir();
        for off in ["", "0", "false", "no", "  "] {
            assert_eq!(resolve(&dir, Some(off)), None, "{off:?} should be off");
        }
        std::fs::remove_dir_all(&dir).ok();
    }
}
