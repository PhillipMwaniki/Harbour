//! User settings: what the app remembers between runs, and what the user may
//! reasonably want to edit in a text editor.
//!
//! The vault holds hosts; this holds preferences. They are separate files for
//! the same reason the vault and the keychain are separate: a settings file is
//! something people copy between machines, paste into an issue, and hand-edit.
//! It must therefore contain nothing that matters if it leaks and nothing that
//! breaks the app if it is malformed - a settings file that will not parse is
//! logged and replaced by defaults, never fatal.

pub mod color;
pub mod highlight;
pub mod scheme;
pub mod store;

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Bumped when a field changes shape rather than merely appearing. Additions
/// are handled by `serde(default)` and need no bump.
pub const CURRENT_VERSION: u32 = 1;

pub const DEFAULT_THEME_ID: &str = "harbour-dark";
pub const DEFAULT_FONT_SIZE: u16 = 13;
/// `{title}`, `{date}` and `{time}` are substituted when a log is started.
pub const DEFAULT_LOG_NAME: &str = "{title}-{date}.log";

fn default_version() -> u32 {
    CURRENT_VERSION
}

fn default_scrollback() -> u32 {
    10_000
}

/// The whole settings document.
///
/// Every field carries a default so that a file written by an older Harbour -
/// or hand-edited down to `{}` - still loads.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    #[serde(default = "default_version")]
    pub version: u32,
    /// Id of a built-in or imported theme. An id that no longer resolves falls
    /// back to the default in the frontend rather than failing here.
    pub theme_id: String,
    /// `null` means the platform monospace stack the frontend ships with.
    pub font_family: Option<String>,
    pub font_size: u16,
    pub scrollback: u32,
    /// Colour schemes imported from VS Code, iTerm or Windows Terminal.
    pub custom_themes: Vec<ThemeSpec>,
    /// Host id -> theme id, for hosts that should not look like everything
    /// else. Production being unmistakable is a safety feature.
    pub host_themes: BTreeMap<String, String>,
    /// Action id -> chords. An action absent here uses its built-in binding;
    /// an action mapped to an empty list is unbound on purpose.
    pub keymap: BTreeMap<String, Vec<String>>,
    pub highlights: Vec<HighlightRule>,
    /// Saved commands, inserted into a terminal from the snippet palette.
    pub snippets: Vec<Snippet>,
    pub logging: LoggingSettings,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            version: CURRENT_VERSION,
            theme_id: DEFAULT_THEME_ID.to_string(),
            font_family: None,
            font_size: DEFAULT_FONT_SIZE,
            scrollback: default_scrollback(),
            custom_themes: Vec::new(),
            host_themes: BTreeMap::new(),
            keymap: BTreeMap::new(),
            highlights: Vec::new(),
            snippets: Vec::new(),
            logging: LoggingSettings::default(),
        }
    }
}

impl Settings {
    /// Drops what a hand-edited file could contain that the rest of the app
    /// would rather not deal with: duplicate ids, absurd font sizes, rules
    /// with nothing to match.
    pub fn sanitise(&mut self) {
        self.version = CURRENT_VERSION;
        self.font_size = self.font_size.clamp(6, 72);
        self.scrollback = self.scrollback.clamp(100, 1_000_000);

        let mut theme_ids = std::collections::HashSet::new();
        self.custom_themes
            .retain(|theme| !theme.id.trim().is_empty() && theme_ids.insert(theme.id.clone()));

        let mut rule_ids = std::collections::HashSet::new();
        self.highlights
            .retain(|rule| !rule.pattern.is_empty() && rule_ids.insert(rule.id.clone()));

        let mut snippet_ids = std::collections::HashSet::new();
        self.snippets
            .retain(|snippet| !snippet.text.is_empty() && snippet_ids.insert(snippet.id.clone()));
    }
}

/// A theme, in the same shape the frontend catalogue uses.
///
/// Built-in themes live in `src/lib/themes.ts` and never cross the boundary;
/// only imported ones are stored here, because only they have to survive a
/// restart.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThemeSpec {
    pub id: String,
    pub label: String,
    pub kind: ThemeKind,
    pub ui: UiColors,
    pub xterm: XtermColors,
    /// Where it was imported from, so the settings dialog can say.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemeKind {
    Dark,
    Light,
}

/// The chrome tokens, published by the frontend as `--hb-*` custom properties.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UiColors {
    pub bg: String,
    pub panel: String,
    pub hover: String,
    pub border: String,
    pub fg: String,
    pub fg_muted: String,
    pub accent: String,
    pub danger: String,
}

/// Mirrors xterm.js's `ITheme`. Every field is optional because a source
/// scheme may define any subset, and xterm fills the rest itself.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct XtermColors {
    pub background: Option<String>,
    pub foreground: Option<String>,
    pub cursor: Option<String>,
    pub cursor_accent: Option<String>,
    pub selection_background: Option<String>,
    pub selection_foreground: Option<String>,
    pub black: Option<String>,
    pub red: Option<String>,
    pub green: Option<String>,
    pub yellow: Option<String>,
    pub blue: Option<String>,
    pub magenta: Option<String>,
    pub cyan: Option<String>,
    pub white: Option<String>,
    pub bright_black: Option<String>,
    pub bright_red: Option<String>,
    pub bright_green: Option<String>,
    pub bright_yellow: Option<String>,
    pub bright_blue: Option<String>,
    pub bright_magenta: Option<String>,
    pub bright_cyan: Option<String>,
    pub bright_white: Option<String>,
}

/// One output highlight rule. Matching happens in the frontend, against the
/// terminal buffer; this is only how a rule is stored.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct HighlightRule {
    pub id: String,
    pub label: String,
    /// A JavaScript regular expression source, without delimiters or flags.
    pub pattern: String,
    pub case_sensitive: bool,
    pub foreground: Option<String>,
    pub background: Option<String>,
    pub enabled: bool,
}

impl Default for HighlightRule {
    fn default() -> Self {
        Self {
            id: String::new(),
            label: String::new(),
            pattern: String::new(),
            case_sensitive: false,
            foreground: None,
            background: None,
            enabled: true,
        }
    }
}

/// One saved command. `text` is inserted verbatim - trailing newline and all,
/// so a snippet can run on insert or wait to be edited, exactly as written.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Snippet {
    pub id: String,
    pub label: String,
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogFormat {
    /// Every byte the session produced, escape sequences included.
    Raw,
    /// Escape sequences removed and carriage returns resolved, so the file
    /// reads the way the screen did.
    #[default]
    Plain,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LoggingSettings {
    /// Where logs are written when the user does not name a file. `null`
    /// means the platform app-log directory.
    pub directory: Option<String>,
    pub format: LogFormat,
    /// Start logging every new session automatically.
    pub auto_start: bool,
    pub name_template: String,
}

impl Default for LoggingSettings {
    fn default() -> Self {
        Self {
            directory: None,
            format: LogFormat::Plain,
            auto_start: false,
            name_template: DEFAULT_LOG_NAME.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_document_loads_as_defaults() {
        let settings: Settings = serde_json::from_str("{}").unwrap();
        assert_eq!(settings, Settings::default());
        assert_eq!(settings.theme_id, DEFAULT_THEME_ID);
    }

    #[test]
    fn unknown_fields_are_tolerated() {
        // A file written by a newer Harbour must still load in an older one.
        let settings: Settings =
            serde_json::from_str(r#"{"themeId":"nord","somethingNew":{"a":1}}"#).unwrap();
        assert_eq!(settings.theme_id, "nord");
    }

    #[test]
    fn round_trips_through_json() {
        let mut settings = Settings::default();
        settings.host_themes.insert("host-1".into(), "nord".into());
        settings
            .keymap
            .insert("terminal.new".into(), vec!["Ctrl+Shift+T".into()]);
        settings.highlights.push(HighlightRule {
            id: "r1".into(),
            label: "Errors".into(),
            pattern: "error".into(),
            foreground: Some("#ff0000".into()),
            ..Default::default()
        });

        let json = serde_json::to_string(&settings).unwrap();
        assert_eq!(serde_json::from_str::<Settings>(&json).unwrap(), settings);
        // camelCase on the wire, matching every other IPC payload.
        assert!(json.contains("\"hostThemes\""));
        assert!(json.contains("\"caseSensitive\""));
    }

    #[test]
    fn sanitise_clamps_and_deduplicates() {
        let mut settings = Settings {
            font_size: 900,
            highlights: vec![
                HighlightRule {
                    id: "a".into(),
                    pattern: "x".into(),
                    ..Default::default()
                },
                // Same id twice: the second is dropped rather than shadowing.
                HighlightRule {
                    id: "a".into(),
                    pattern: "y".into(),
                    ..Default::default()
                },
                // No pattern: nothing to match, so nothing to keep.
                HighlightRule {
                    id: "b".into(),
                    pattern: String::new(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };

        settings.sanitise();

        assert_eq!(settings.font_size, 72);
        assert_eq!(settings.highlights.len(), 1);
        assert_eq!(settings.highlights[0].pattern, "x");
    }
}
