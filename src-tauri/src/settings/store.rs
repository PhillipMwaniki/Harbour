//! Reads and writes `settings.json`, and keeps the parsed copy in memory.
//!
//! Two rules drive the whole file. A settings file that will not parse must
//! never stop Harbour from starting - the app is a terminal, and a terminal
//! that refuses to open because of a stray comma is useless exactly when it is
//! needed. And a file the user has hand-edited must never be silently
//! destroyed, so an unreadable one is moved aside rather than overwritten.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use parking_lot::RwLock;

use crate::error::{AppError, AppResult};
use crate::settings::Settings;

pub struct SettingsStore {
    path: PathBuf,
    current: RwLock<Settings>,
}

impl SettingsStore {
    /// Opens the store at `path`, reading it if it is there. Never fails:
    /// anything unreadable becomes defaults plus a log line.
    pub fn open(path: PathBuf) -> Self {
        let current = RwLock::new(read(&path));
        Self { path, current }
    }

    /// An in-memory store, for tests and for the case where there is nowhere
    /// on disk to put one.
    pub fn ephemeral() -> Self {
        Self {
            path: PathBuf::new(),
            current: RwLock::new(Settings::default()),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn get(&self) -> Settings {
        self.current.read().clone()
    }

    /// Replaces the whole document. The caller sends what it wants the file to
    /// contain; there is no partial update, because a settings dialog that
    /// merges field by field ends up with two sources of truth.
    pub fn replace(&self, mut settings: Settings) -> AppResult<Settings> {
        settings.sanitise();
        if !self.path.as_os_str().is_empty() {
            write(&self.path, &settings)?;
        }
        *self.current.write() = settings.clone();
        Ok(settings)
    }

    /// Re-reads the file, for when the user has edited it by hand.
    pub fn reload(&self) -> Settings {
        let settings = read(&self.path);
        *self.current.write() = settings.clone();
        settings
    }
}

fn read(path: &Path) -> Settings {
    if path.as_os_str().is_empty() {
        return Settings::default();
    }
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Settings::default(),
        Err(err) => {
            tracing::warn!(path = %path.display(), error = %err, "could not read settings; using defaults");
            return Settings::default();
        }
    };

    match serde_json::from_str::<Settings>(&text) {
        Ok(mut settings) => {
            settings.sanitise();
            settings
        }
        Err(err) => {
            // Keep what the user wrote. Overwriting it on the next save would
            // destroy an edit whose only crime is a typo.
            let aside = path.with_extension("invalid.json");
            if let Err(move_err) = fs::rename(path, &aside) {
                tracing::warn!(error = %move_err, "could not set the bad settings file aside");
            }
            tracing::error!(
                path = %path.display(),
                moved_to = %aside.display(),
                error = %err,
                "settings file could not be parsed; continuing with defaults"
            );
            Settings::default()
        }
    }
}

/// Writes through a temporary file in the same directory, so an interrupted
/// save leaves the old settings intact rather than a truncated document.
fn write(path: &Path, settings: &Settings) -> AppResult<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| {
            AppError::Settings(format!("could not create {}: {err}", parent.display()))
        })?;
    }

    let json = serde_json::to_string_pretty(settings)
        .map_err(|err| AppError::Settings(format!("could not serialise settings: {err}")))?;

    let temp = path.with_extension("json.tmp");
    {
        let mut file = fs::File::create(&temp).map_err(|err| {
            AppError::Settings(format!("could not write {}: {err}", temp.display()))
        })?;
        file.write_all(json.as_bytes())
            .and_then(|()| file.write_all(b"\n"))
            .and_then(|()| file.sync_all())
            .map_err(|err| {
                AppError::Settings(format!("could not write {}: {err}", temp.display()))
            })?;
    }

    fs::rename(&temp, path).map_err(|err| {
        let _ = fs::remove_file(&temp);
        AppError::Settings(format!("could not replace {}: {err}", path.display()))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{HighlightRule, LogFormat, LoggingSettings};

    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("harbour-settings-{name}-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_missing_file_reads_as_defaults_and_writes_on_save() {
        let dir = temp_dir("missing");
        let path = dir.join("settings.json");
        let store = SettingsStore::open(path.clone());

        assert_eq!(store.get(), Settings::default());
        assert!(!path.exists(), "reading must not create the file");

        store
            .replace(Settings {
                theme_id: "nord".into(),
                ..Default::default()
            })
            .unwrap();

        assert!(path.exists());
        assert_eq!(SettingsStore::open(path).get().theme_id, "nord");
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_saved_document_survives_a_round_trip() {
        let dir = temp_dir("roundtrip");
        let path = dir.join("settings.json");
        let store = SettingsStore::open(path.clone());

        let mut wanted = Settings {
            font_size: 15,
            logging: LoggingSettings {
                format: LogFormat::Raw,
                auto_start: true,
                ..Default::default()
            },
            ..Default::default()
        };
        wanted.host_themes.insert("h1".into(), "dracula".into());
        wanted
            .keymap
            .insert("pane.split".into(), vec!["Ctrl+\\".into()]);
        wanted.highlights.push(HighlightRule {
            id: "err".into(),
            label: "Errors".into(),
            pattern: "(?i)error".into(),
            background: Some("#450a0a".into()),
            ..Default::default()
        });
        store.replace(wanted.clone()).unwrap();

        assert_eq!(SettingsStore::open(path).get(), wanted);
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_malformed_file_is_moved_aside_rather_than_lost() {
        let dir = temp_dir("malformed");
        let path = dir.join("settings.json");
        fs::write(&path, "{ not json at all").unwrap();

        let store = SettingsStore::open(path.clone());

        assert_eq!(store.get(), Settings::default());
        assert!(!path.exists(), "the bad file should have been moved");
        let aside = dir.join("settings.invalid.json");
        assert_eq!(fs::read_to_string(aside).unwrap(), "{ not json at all");
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn saving_sanitises_before_writing() {
        let dir = temp_dir("sanitise");
        let path = dir.join("settings.json");
        let store = SettingsStore::open(path.clone());

        store
            .replace(Settings {
                font_size: 0,
                ..Default::default()
            })
            .unwrap();

        assert_eq!(store.get().font_size, 6);
        assert_eq!(SettingsStore::open(path).get().font_size, 6);
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn reload_picks_up_a_hand_edit() {
        let dir = temp_dir("reload");
        let path = dir.join("settings.json");
        let store = SettingsStore::open(path.clone());
        store.replace(Settings::default()).unwrap();

        fs::write(&path, r#"{"themeId":"gruvbox-dark"}"#).unwrap();

        assert_eq!(store.get().theme_id, "harbour-dark");
        assert_eq!(store.reload().theme_id, "gruvbox-dark");
        assert_eq!(store.get().theme_id, "gruvbox-dark");
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn an_ephemeral_store_keeps_settings_without_a_file() {
        let store = SettingsStore::ephemeral();
        store
            .replace(Settings {
                theme_id: "monokai".into(),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(store.get().theme_id, "monokai");
    }
}
