//! Importing colour schemes written for other terminals.
//!
//! Three formats, one output. Every one of them describes a terminal palette
//! and nothing else, so the chrome tokens Harbour also needs - panel, border,
//! muted text - are derived from the two colours all three do define. The
//! alternative, asking the user to pick eight more colours after importing a
//! scheme, is not an import.
//!
//! | Source | What is read |
//! | --- | --- |
//! | VS Code theme (`.json`, with comments) | the `colors` map: `terminal.ansi*`, falling back to `editor.*` |
//! | Windows Terminal `settings.json` | every entry of `schemes`, or a bare scheme object |
//! | iTerm2 (`.itermcolors`) | the `Ansi N Color` dicts of the XML plist |
//!
//! Nothing here writes: the caller reviews what was found and saves the ones
//! it wants, the same way the vault importers work.

use std::path::Path;

use serde::Serialize;
use serde_json::Value;

use crate::error::{AppError, AppResult};
use crate::settings::color::Rgb;
use crate::settings::{ThemeKind, ThemeSpec, UiColors, XtermColors};

/// What one import found. `notes` carries anything that was not a scheme, so
/// a directory of forty files does not silently yield thirty-eight themes.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SchemeImport {
    pub source: String,
    pub themes: Vec<ThemeSpec>,
    pub notes: Vec<String>,
}

/// Imported ids are namespaced so an imported "Nord" cannot shadow the
/// built-in one.
pub const IMPORTED_PREFIX: &str = "imported.";

/// Reads a file, or every scheme file in a directory.
pub fn import(path: &Path) -> AppResult<SchemeImport> {
    let meta = std::fs::metadata(path).map_err(|err| AppError::SchemeImport {
        path: path.display().to_string(),
        reason: err.to_string(),
    })?;

    let mut themes = Vec::new();
    let mut notes = Vec::new();

    if meta.is_dir() {
        let mut entries: Vec<_> = std::fs::read_dir(path)
            .map_err(|err| AppError::SchemeImport {
                path: path.display().to_string(),
                reason: err.to_string(),
            })?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|entry| entry.is_file() && has_scheme_extension(entry))
            .collect();
        entries.sort();

        if entries.is_empty() {
            notes.push("No .itermcolors or .json files in that directory.".into());
        }
        for entry in entries {
            match parse_file(&entry) {
                Ok(found) if found.is_empty() => {
                    notes.push(format!("{}: no colour scheme in it.", file_label(&entry)));
                }
                Ok(found) => themes.extend(found),
                Err(reason) => notes.push(format!("{}: {reason}", file_label(&entry))),
            }
        }
    } else {
        themes = parse_file(path).map_err(|reason| AppError::SchemeImport {
            path: path.display().to_string(),
            reason,
        })?;
        if themes.is_empty() {
            return Err(AppError::SchemeImport {
                path: path.display().to_string(),
                reason: "no colour scheme in it".into(),
            });
        }
    }

    dedupe_ids(&mut themes);
    Ok(SchemeImport {
        source: path.display().to_string(),
        themes,
        notes,
    })
}

fn has_scheme_extension(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|ext| ext.to_str()),
        Some("itermcolors" | "json" | "jsonc")
    )
}

fn file_label(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("(unnamed)")
        .to_string()
}

/// Two schemes with the same name - "Solarized Dark" from two files - would
/// otherwise both claim one id, and the second would be dropped on save.
fn dedupe_ids(themes: &mut [ThemeSpec]) {
    let mut seen: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
    for theme in themes.iter_mut() {
        let count = seen.entry(theme.id.clone()).or_insert(0);
        *count += 1;
        if *count > 1 {
            theme.id = format!("{}-{}", theme.id, count);
        }
    }
}

fn parse_file(path: &Path) -> Result<Vec<ThemeSpec>, String> {
    let text = std::fs::read_to_string(path).map_err(|err| err.to_string())?;
    let source = file_label(path);
    let fallback_label = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("Imported")
        .to_string();

    if text.trim_start().starts_with("<?xml") || text.contains("<plist") {
        let colors = parse_itermcolors(&text)?;
        return Ok(vec![theme_from(&fallback_label, &source, colors)]);
    }

    // VS Code themes are JSON with comments, and Windows Terminal tolerates
    // them too; a strict parser would reject most real files.
    let value: Value = serde_json::from_str(&strip_json_comments(&text))
        .map_err(|err| format!("not JSON ({err})"))?;

    if let Some(schemes) = value.get("schemes").and_then(Value::as_array) {
        let themes: Vec<_> = schemes
            .iter()
            .filter_map(|scheme| {
                let label = scheme.get("name").and_then(Value::as_str)?.to_string();
                Some(theme_from(&label, &source, windows_terminal_colors(scheme)))
            })
            .collect();
        return Ok(themes);
    }

    if let Some(colors) = value.get("colors").and_then(Value::as_object) {
        let label = value
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or(&fallback_label)
            .to_string();
        let palette = vscode_colors(&Value::Object(colors.clone()));
        if palette.background.is_none() && palette.black.is_none() {
            return Err("a VS Code theme with no terminal colours in it".into());
        }
        return Ok(vec![theme_from(&label, &source, palette)]);
    }

    // A bare Windows Terminal scheme, as pasted from windowsterminalthemes.dev.
    if value.get("background").is_some() && value.get("black").is_some() {
        let label = value
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or(&fallback_label)
            .to_string();
        return Ok(vec![theme_from(
            &label,
            &source,
            windows_terminal_colors(&value),
        )]);
    }

    Err("not a VS Code, Windows Terminal or iTerm colour scheme".into())
}

// ---------------------------------------------------------------------------
// Format readers
// ---------------------------------------------------------------------------

fn hex_at(value: &Value, key: &str) -> Option<String> {
    let raw = value.get(key)?.as_str()?;
    Rgb::parse(raw).map(Rgb::to_hex)
}

fn first_hex(value: &Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| hex_at(value, key))
}

fn windows_terminal_colors(scheme: &Value) -> XtermColors {
    XtermColors {
        background: hex_at(scheme, "background"),
        foreground: hex_at(scheme, "foreground"),
        cursor: hex_at(scheme, "cursorColor"),
        cursor_accent: None,
        selection_background: hex_at(scheme, "selectionBackground"),
        selection_foreground: None,
        black: hex_at(scheme, "black"),
        red: hex_at(scheme, "red"),
        green: hex_at(scheme, "green"),
        yellow: hex_at(scheme, "yellow"),
        blue: hex_at(scheme, "blue"),
        // Windows Terminal calls magenta "purple"; accept both spellings so a
        // scheme converted from another tool still reads.
        magenta: first_hex(scheme, &["purple", "magenta"]),
        cyan: hex_at(scheme, "cyan"),
        white: hex_at(scheme, "white"),
        bright_black: hex_at(scheme, "brightBlack"),
        bright_red: hex_at(scheme, "brightRed"),
        bright_green: hex_at(scheme, "brightGreen"),
        bright_yellow: hex_at(scheme, "brightYellow"),
        bright_blue: hex_at(scheme, "brightBlue"),
        bright_magenta: first_hex(scheme, &["brightPurple", "brightMagenta"]),
        bright_cyan: hex_at(scheme, "brightCyan"),
        bright_white: hex_at(scheme, "brightWhite"),
    }
}

fn vscode_colors(colors: &Value) -> XtermColors {
    XtermColors {
        // A theme that colours only the editor still tells us enough to build
        // a usable terminal palette from.
        background: first_hex(colors, &["terminal.background", "editor.background"]),
        foreground: first_hex(colors, &["terminal.foreground", "editor.foreground"]),
        cursor: first_hex(
            colors,
            &["terminalCursor.foreground", "editorCursor.foreground"],
        ),
        cursor_accent: hex_at(colors, "terminalCursor.background"),
        selection_background: first_hex(
            colors,
            &["terminal.selectionBackground", "editor.selectionBackground"],
        ),
        selection_foreground: hex_at(colors, "terminal.selectionForeground"),
        black: hex_at(colors, "terminal.ansiBlack"),
        red: hex_at(colors, "terminal.ansiRed"),
        green: hex_at(colors, "terminal.ansiGreen"),
        yellow: hex_at(colors, "terminal.ansiYellow"),
        blue: hex_at(colors, "terminal.ansiBlue"),
        magenta: hex_at(colors, "terminal.ansiMagenta"),
        cyan: hex_at(colors, "terminal.ansiCyan"),
        white: hex_at(colors, "terminal.ansiWhite"),
        bright_black: hex_at(colors, "terminal.ansiBrightBlack"),
        bright_red: hex_at(colors, "terminal.ansiBrightRed"),
        bright_green: hex_at(colors, "terminal.ansiBrightGreen"),
        bright_yellow: hex_at(colors, "terminal.ansiBrightYellow"),
        bright_blue: hex_at(colors, "terminal.ansiBrightBlue"),
        bright_magenta: hex_at(colors, "terminal.ansiBrightMagenta"),
        bright_cyan: hex_at(colors, "terminal.ansiBrightCyan"),
        bright_white: hex_at(colors, "terminal.ansiBrightWhite"),
    }
}

/// An `.itermcolors` file is an XML plist whose top-level dict maps a colour
/// name to a dict of 0..1 components. Colour dicts never nest, which is what
/// makes this scanner - rather than a plist library - enough.
fn parse_itermcolors(text: &str) -> Result<XtermColors, String> {
    let mut colors = XtermColors::default();
    let mut found = 0usize;
    let mut cursor = 0usize;

    while let Some(offset) = text[cursor..].find("<key>") {
        let key_start = cursor + offset + "<key>".len();
        let Some(key_len) = text[key_start..].find("</key>") else {
            break;
        };
        let key = text[key_start..key_start + key_len].trim().to_string();
        cursor = key_start + key_len + "</key>".len();

        let Some(dict_offset) = text[cursor..].find("<dict>") else {
            break;
        };
        let dict_start = cursor + dict_offset + "<dict>".len();
        let Some(dict_len) = text[dict_start..].find("</dict>") else {
            break;
        };
        let body = &text[dict_start..dict_start + dict_len];
        cursor = dict_start + dict_len + "</dict>".len();

        let Some(rgb) = components(body) else {
            continue;
        };
        let hex = Some(rgb.to_hex());
        found += 1;
        match key.as_str() {
            "Ansi 0 Color" => colors.black = hex,
            "Ansi 1 Color" => colors.red = hex,
            "Ansi 2 Color" => colors.green = hex,
            "Ansi 3 Color" => colors.yellow = hex,
            "Ansi 4 Color" => colors.blue = hex,
            "Ansi 5 Color" => colors.magenta = hex,
            "Ansi 6 Color" => colors.cyan = hex,
            "Ansi 7 Color" => colors.white = hex,
            "Ansi 8 Color" => colors.bright_black = hex,
            "Ansi 9 Color" => colors.bright_red = hex,
            "Ansi 10 Color" => colors.bright_green = hex,
            "Ansi 11 Color" => colors.bright_yellow = hex,
            "Ansi 12 Color" => colors.bright_blue = hex,
            "Ansi 13 Color" => colors.bright_magenta = hex,
            "Ansi 14 Color" => colors.bright_cyan = hex,
            "Ansi 15 Color" => colors.bright_white = hex,
            "Background Color" => colors.background = hex,
            "Foreground Color" => colors.foreground = hex,
            "Cursor Color" => colors.cursor = hex,
            "Cursor Text Color" => colors.cursor_accent = hex,
            "Selection Color" => colors.selection_background = hex,
            "Selected Text Color" => colors.selection_foreground = hex,
            _ => found -= 1,
        }
    }

    if found == 0 {
        return Err("a plist with no colours Harbour recognises".into());
    }
    Ok(colors)
}

/// Pulls the three components out of one colour dict. iTerm writes them as
/// `<real>`, but hand-edited files sometimes use `<integer>0</integer>`.
fn components(body: &str) -> Option<Rgb> {
    fn component(body: &str, name: &str) -> Option<f32> {
        let key = format!("<key>{name} Component</key>");
        let start = body.find(&key)? + key.len();
        let rest = &body[start..];
        let open = rest.find('<')?;
        let tag_end = rest[open..].find('>')? + open + 1;
        let close = rest[tag_end..].find('<')? + tag_end;
        rest[tag_end..close].trim().parse::<f32>().ok()
    }

    Some(Rgb::from_unit(
        component(body, "Red")?,
        component(body, "Green")?,
        component(body, "Blue")?,
    ))
}

/// Removes `//` and `/* */` comments, and trailing commas, from JSON with
/// comments. Strings are honoured, so a `//` inside a colour name survives.
pub fn strip_json_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    let mut in_string = false;
    let mut escaped = false;

    while let Some(c) = chars.next() {
        if in_string {
            out.push(c);
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_string = false;
            }
            continue;
        }

        match c {
            '"' => {
                in_string = true;
                out.push(c);
            }
            '/' if chars.peek() == Some(&'/') => {
                for next in chars.by_ref() {
                    if next == '\n' {
                        out.push('\n');
                        break;
                    }
                }
            }
            '/' if chars.peek() == Some(&'*') => {
                chars.next();
                let mut previous = '\0';
                for next in chars.by_ref() {
                    if previous == '*' && next == '/' {
                        break;
                    }
                    previous = next;
                }
                // Keep the comment's width off the output but not its lines,
                // so a parse error still points at the right line.
                out.push(' ');
            }
            ',' => {
                // A trailing comma is legal in a VS Code theme and not in JSON.
                let mut lookahead = chars.clone();
                let next = loop {
                    match lookahead.next() {
                        Some(c) if c.is_whitespace() => continue,
                        other => break other,
                    }
                };
                if !matches!(next, Some('}') | Some(']')) {
                    out.push(',');
                }
            }
            _ => out.push(c),
        }
    }

    out
}

// ---------------------------------------------------------------------------
// Palette -> theme
// ---------------------------------------------------------------------------

fn slug(label: &str) -> String {
    let mut out = String::new();
    let mut pending_dash = false;
    for c in label.chars() {
        if c.is_ascii_alphanumeric() {
            if pending_dash && !out.is_empty() {
                out.push('-');
            }
            pending_dash = false;
            out.extend(c.to_lowercase());
        } else {
            pending_dash = true;
        }
    }
    if out.is_empty() {
        out.push_str("scheme");
    }
    out
}

fn contrast(a: Rgb, b: Rgb) -> f32 {
    let (high, low) = if a.luminance() >= b.luminance() {
        (a.luminance(), b.luminance())
    } else {
        (b.luminance(), a.luminance())
    };
    (high + 0.05) / (low + 0.05)
}

/// Builds the chrome tokens an imported scheme does not carry.
///
/// Everything is mixed between the scheme's own background and foreground, so
/// the result belongs to the scheme rather than to some fixed grey ramp: a
/// warm scheme gets warm borders.
pub fn derive_ui(colors: &XtermColors, bg: Rgb, fg: Rgb) -> UiColors {
    let pick = |value: &Option<String>| value.as_deref().and_then(Rgb::parse);

    // The cursor is the scheme's own idea of "look here", so it makes the best
    // accent - unless it is nearly the background, which cursors often are.
    let accent = pick(&colors.cursor)
        .filter(|cursor| contrast(*cursor, bg) >= 2.0)
        .or_else(|| pick(&colors.bright_blue))
        .or_else(|| pick(&colors.blue))
        .or_else(|| pick(&colors.cyan))
        .unwrap_or_else(|| bg.mix(fg, 0.8));

    let danger = pick(&colors.red)
        .or_else(|| pick(&colors.bright_red))
        .unwrap_or_else(|| Rgb::new(0xf8, 0x71, 0x71));

    UiColors {
        bg: bg.to_hex(),
        panel: bg.mix(fg, 0.07).to_hex(),
        hover: bg.mix(fg, 0.14).to_hex(),
        border: bg.mix(fg, 0.22).to_hex(),
        fg: fg.to_hex(),
        fg_muted: bg.mix(fg, 0.55).to_hex(),
        accent: accent.to_hex(),
        danger: danger.to_hex(),
    }
}

fn theme_from(label: &str, source: &str, colors: XtermColors) -> ThemeSpec {
    let bg = colors
        .background
        .as_deref()
        .and_then(Rgb::parse)
        .or_else(|| colors.black.as_deref().and_then(Rgb::parse))
        .unwrap_or(Rgb::new(0x10, 0x10, 0x10));
    let fg = colors
        .foreground
        .as_deref()
        .and_then(Rgb::parse)
        .or_else(|| colors.white.as_deref().and_then(Rgb::parse))
        .unwrap_or_else(|| {
            if bg.is_dark() {
                Rgb::new(0xd0, 0xd0, 0xd0)
            } else {
                Rgb::new(0x20, 0x20, 0x20)
            }
        });

    let mut colors = colors;
    colors.background.get_or_insert_with(|| bg.to_hex());
    colors.foreground.get_or_insert_with(|| fg.to_hex());

    ThemeSpec {
        id: format!("{IMPORTED_PREFIX}{}", slug(label)),
        label: label.trim().to_string(),
        kind: if bg.is_dark() {
            ThemeKind::Dark
        } else {
            ThemeKind::Light
        },
        ui: derive_ui(&colors, bg, fg),
        xterm: colors,
        source: Some(source.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("harbour-scheme-{name}-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    const ITERM: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>Ansi 0 Color</key>
	<dict>
		<key>Color Space</key>
		<string>sRGB</string>
		<key>Blue Component</key>
		<real>0.0</real>
		<key>Green Component</key>
		<real>0.0</real>
		<key>Red Component</key>
		<real>0.0</real>
	</dict>
	<key>Ansi 1 Color</key>
	<dict>
		<key>Blue Component</key>
		<real>0.0</real>
		<key>Green Component</key>
		<real>0.0</real>
		<key>Red Component</key>
		<real>1.0</real>
	</dict>
	<key>Background Color</key>
	<dict>
		<key>Blue Component</key>
		<real>0.11764705882352941</real>
		<key>Green Component</key>
		<real>0.11764705882352941</real>
		<key>Red Component</key>
		<real>0.11764705882352941</real>
	</dict>
	<key>Foreground Color</key>
	<dict>
		<key>Blue Component</key>
		<real>0.8</real>
		<key>Green Component</key>
		<real>0.8</real>
		<key>Red Component</key>
		<real>0.8</real>
	</dict>
</dict>
</plist>
"#;

    #[test]
    fn reads_an_itermcolors_palette() {
        let colors = parse_itermcolors(ITERM).unwrap();
        assert_eq!(colors.black.as_deref(), Some("#000000"));
        assert_eq!(colors.red.as_deref(), Some("#ff0000"));
        assert_eq!(colors.background.as_deref(), Some("#1e1e1e"));
        assert_eq!(colors.foreground.as_deref(), Some("#cccccc"));
        // Absent colours stay absent; xterm fills its own defaults.
        assert_eq!(colors.cyan, None);
    }

    #[test]
    fn a_plist_with_nothing_recognisable_is_an_error() {
        let plist = "<?xml version=\"1.0\"?><plist><dict><key>Draw Bold</key><dict><key>Red Component</key><real>1</real><key>Green Component</key><real>1</real><key>Blue Component</key><real>1</real></dict></dict></plist>";
        assert!(parse_itermcolors(plist).is_err());
    }

    #[test]
    fn imports_an_itermcolors_file_as_a_dark_theme() {
        let dir = temp_dir("iterm");
        let path = dir.join("Ayu Mirage.itermcolors");
        fs::write(&path, ITERM).unwrap();

        let found = import(&path).unwrap();

        assert_eq!(found.themes.len(), 1);
        let theme = &found.themes[0];
        assert_eq!(theme.id, "imported.ayu-mirage");
        assert_eq!(theme.label, "Ayu Mirage");
        assert_eq!(theme.kind, ThemeKind::Dark);
        assert_eq!(theme.ui.bg, "#1e1e1e");
        assert_eq!(theme.ui.danger, "#ff0000");
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn reads_every_scheme_in_a_windows_terminal_settings_file() {
        let dir = temp_dir("wt");
        let path = dir.join("settings.json");
        fs::write(
            &path,
            r##"{
              // Windows Terminal writes comments into its own settings file.
              "profiles": { "defaults": {} },
              "schemes": [
                {
                  "name": "Campbell",
                  "background": "#0C0C0C", "foreground": "#CCCCCC",
                  "black": "#0C0C0C", "red": "#C50F1F", "green": "#13A10E",
                  "yellow": "#C19C00", "blue": "#0037DA", "purple": "#881798",
                  "cyan": "#3A96DD", "white": "#CCCCCC",
                  "brightBlack": "#767676", "brightRed": "#E74856",
                  "brightGreen": "#16C60C", "brightYellow": "#F9F1A5",
                  "brightBlue": "#3B78FF", "brightPurple": "#B4009E",
                  "brightCyan": "#61D6D6", "brightWhite": "#F2F2F2",
                  "cursorColor": "#FFFFFF", "selectionBackground": "#FFFFFF",
                },
                {
                  "name": "Solarized Light",
                  "background": "#FDF6E3", "foreground": "#657B83",
                  "black": "#073642", "red": "#DC322F", "green": "#859900",
                  "yellow": "#B58900", "blue": "#268BD2", "purple": "#D33682",
                  "cyan": "#2AA198", "white": "#EEE8D5",
                  "brightBlack": "#002B36", "brightRed": "#CB4B16",
                  "brightGreen": "#586E75", "brightYellow": "#657B83",
                  "brightBlue": "#839496", "brightPurple": "#6C71C4",
                  "brightCyan": "#93A1A1", "brightWhite": "#FDF6E3"
                }
              ]
            }"##,
        )
        .unwrap();

        let found = import(&path).unwrap();

        assert_eq!(found.themes.len(), 2);
        assert_eq!(found.themes[0].label, "Campbell");
        assert_eq!(found.themes[0].kind, ThemeKind::Dark);
        // "purple" is Windows Terminal's name for magenta.
        assert_eq!(found.themes[0].xterm.magenta.as_deref(), Some("#881798"));
        assert_eq!(found.themes[1].kind, ThemeKind::Light);
        assert_eq!(found.themes[1].ui.bg, "#fdf6e3");
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn reads_a_vs_code_theme_with_comments_and_trailing_commas() {
        let dir = temp_dir("vscode");
        let path = dir.join("night-owl.json");
        fs::write(
            &path,
            r##"{
              "name": "Night Owl",
              "type": "dark",
              /* block comment */
              "colors": {
                "editor.background": "#011627",
                "terminal.background": "#011627",
                "terminal.foreground": "#d6deeb",
                "terminal.ansiRed": "#EF5350", // inline comment
                "terminal.ansiBrightCyan": "#7fdbca",
                "terminalCursor.foreground": "#80a4c2",
              },
              "tokenColors": []
            }"##,
        )
        .unwrap();

        let found = import(&path).unwrap();

        assert_eq!(found.themes.len(), 1);
        let theme = &found.themes[0];
        assert_eq!(theme.label, "Night Owl");
        assert_eq!(theme.xterm.red.as_deref(), Some("#ef5350"));
        assert_eq!(theme.xterm.bright_cyan.as_deref(), Some("#7fdbca"));
        // The cursor contrasts with the background, so it becomes the accent.
        assert_eq!(theme.ui.accent, "#80a4c2");
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_theme_with_no_terminal_colours_falls_back_to_the_editor() {
        let dir = temp_dir("editor-only");
        let path = dir.join("plain.json");
        fs::write(
            &path,
            r##"{"name":"Plain","colors":{"editor.background":"#ffffff","editor.foreground":"#333333"}}"##,
        )
        .unwrap();

        let theme = import(&path).unwrap().themes.remove(0);

        assert_eq!(theme.kind, ThemeKind::Light);
        assert_eq!(theme.xterm.background.as_deref(), Some("#ffffff"));
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_directory_imports_every_scheme_and_reports_the_rest() {
        let dir = temp_dir("dir");
        fs::write(dir.join("one.itermcolors"), ITERM).unwrap();
        fs::write(dir.join("two.itermcolors"), ITERM).unwrap();
        fs::write(dir.join("notes.json"), r#"{"hello":"world"}"#).unwrap();
        fs::write(dir.join("readme.txt"), "ignored entirely").unwrap();

        let found = import(&dir).unwrap();

        assert_eq!(found.themes.len(), 2);
        // Same name in two files must not collapse to one id.
        assert_ne!(found.themes[0].id, found.themes[1].id);
        assert_eq!(found.notes.len(), 1);
        assert!(found.notes[0].starts_with("notes.json:"));
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_file_that_is_not_a_scheme_says_so() {
        let dir = temp_dir("junk");
        let path = dir.join("junk.json");
        fs::write(&path, r#"{"anything":"else"}"#).unwrap();

        let err = import(&path).unwrap_err();

        assert_eq!(err.code(), "SCHEME_IMPORT_FAILED");
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_missing_path_is_an_error_rather_than_an_empty_import() {
        let err = import(Path::new("no-such-directory-anywhere")).unwrap_err();
        assert_eq!(err.code(), "SCHEME_IMPORT_FAILED");
    }

    #[test]
    fn comment_stripping_leaves_strings_alone() {
        let stripped = strip_json_comments(r#"{"url":"https://example.com//x","a":1,}"#);
        assert_eq!(stripped, r#"{"url":"https://example.com//x","a":1}"#);
    }

    #[test]
    fn derived_chrome_sits_between_background_and_foreground() {
        let colors = XtermColors {
            background: Some("#000000".into()),
            foreground: Some("#ffffff".into()),
            ..Default::default()
        };
        let ui = derive_ui(&colors, Rgb::new(0, 0, 0), Rgb::new(255, 255, 255));

        assert_eq!(ui.bg, "#000000");
        assert_eq!(ui.fg, "#ffffff");
        let panel = Rgb::parse(&ui.panel).unwrap();
        let border = Rgb::parse(&ui.border).unwrap();
        assert!(panel.luminance() > 0.0 && panel.luminance() < border.luminance());
    }

    #[test]
    fn slugs_are_stable_and_url_shaped() {
        assert_eq!(slug("Solarized Dark"), "solarized-dark");
        assert_eq!(slug("  Ayu  Mirage  "), "ayu-mirage");
        assert_eq!(slug("!!!"), "scheme");
    }
}
