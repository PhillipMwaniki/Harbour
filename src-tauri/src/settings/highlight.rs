//! Importing Xshell highlight sets (`.hls`) as highlight rules.
//!
//! A highlight set is an INI file: one `[Keyword_N]` section per rule and a
//! `[Colors]` palette of sixteen entries the rules index into. Two things about
//! it are not obvious and are worth knowing before touching the code:
//!
//! - Colour indices are Windows resource ids, not palette positions. `281`
//!   means palette entry 1; the offset is 280.
//! - The palette is written as `COLORREF` values, which are `BBGGRR` - the
//!   reverse of the `RRGGBB` every other file in the backup uses. The default
//!   set's bright red is `5555FF`, which only reads as red the other way
//!   round.

use std::path::Path;

use serde::Serialize;

use crate::error::{AppError, AppResult};
use crate::settings::color::Rgb;
use crate::settings::HighlightRule;
use crate::text;
use crate::xts;

/// What an import found. Nothing is saved until the caller says so.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HighlightImport {
    pub source: String,
    pub rules: Vec<HighlightRule>,
    /// Rules or files that could not be brought across, and why.
    pub notes: Vec<String>,
}

/// Xshell numbers its colour indices from here.
const INDEX_BASE: i64 = 280;

/// Reads a `.hls` file, a directory of them, or the highlight sets inside a
/// `.xts` backup.
pub fn import(path: &Path) -> AppResult<HighlightImport> {
    let fail = |reason: String| AppError::HighlightImport {
        path: path.display().to_string(),
        reason,
    };

    let mut rules = Vec::new();
    let mut notes = Vec::new();

    if xts::Archive::is_archive(path) {
        let mut archive = xts::Archive::open(path).map_err(|err| fail(err.to_string()))?;
        let sets = archive
            .highlight_sets()
            .map_err(|err| fail(err.to_string()))?;
        if sets.is_empty() {
            return Err(fail("no highlight sets in the backup".into()));
        }
        for set in sets {
            collect(&set.name, &set.text, &mut rules, &mut notes);
        }
    } else if path.is_dir() {
        let mut entries: Vec<_> = std::fs::read_dir(path)
            .map_err(|err| fail(err.to_string()))?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|entry| {
                entry.is_file()
                    && entry
                        .extension()
                        .is_some_and(|ext| ext.eq_ignore_ascii_case("hls"))
            })
            .collect();
        entries.sort();
        if entries.is_empty() {
            notes.push("No .hls files in that directory.".into());
        }
        for entry in entries {
            let name = stem(&entry);
            match std::fs::read(&entry) {
                Ok(bytes) => collect(&name, &text::decode(&bytes), &mut rules, &mut notes),
                Err(err) => notes.push(format!("{name}.hls: {err}")),
            }
        }
    } else {
        let bytes = std::fs::read(path).map_err(|err| fail(err.to_string()))?;
        let name = stem(path);
        let found = parse_hls(&name, &text::decode(&bytes)).map_err(fail)?;
        if found.0.is_empty() {
            return Err(fail("no rules in it".into()));
        }
        rules = found.0;
        notes = found.1;
    }

    Ok(HighlightImport {
        source: path.display().to_string(),
        rules,
        notes,
    })
}

fn stem(path: &Path) -> String {
    path.file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Imported".into())
}

fn collect(name: &str, contents: &str, rules: &mut Vec<HighlightRule>, notes: &mut Vec<String>) {
    match parse_hls(name, contents) {
        Ok((found, mut found_notes)) => {
            if found.is_empty() {
                notes.push(format!("{name}: no rules in it"));
            }
            rules.extend(found);
            notes.append(&mut found_notes);
        }
        Err(reason) => notes.push(format!("{name}: {reason}")),
    }
}

/// Parses one highlight set. `set_name` labels rules that carry no
/// description of their own.
pub fn parse_hls(
    set_name: &str,
    contents: &str,
) -> Result<(Vec<HighlightRule>, Vec<String>), String> {
    let sections = text::ini(contents);

    let palette: Vec<Option<String>> = text::ini_get(&sections, "COLORS", "COLORS")
        .map(|list| {
            list.split(',')
                .map(|entry| colorref(entry.trim()))
                .collect()
        })
        .unwrap_or_default();

    let mut keywords: Vec<(u32, &str)> = sections
        .keys()
        .filter_map(|section| {
            let index = section.strip_prefix("KEYWORD_")?.parse::<u32>().ok()?;
            Some((index, section.as_str()))
        })
        .collect();
    if keywords.is_empty() {
        return Err("not an Xshell highlight set: no [Keyword_N] sections".into());
    }
    keywords.sort();

    let get = |section: &str, key: &str| text::ini_get(&sections, section, key);
    let flag = |section: &str, key: &str, default: bool| match get(section, key) {
        Some(value) => value.trim() != "0",
        None => default,
    };
    let colour = |section: &str, key: &str| -> Option<String> {
        let index = get(section, key)?.trim().parse::<i64>().ok()? - INDEX_BASE;
        usize::try_from(index)
            .ok()
            .and_then(|i| palette.get(i).cloned().flatten())
    };

    let mut rules = Vec::new();
    let mut notes = Vec::new();

    for (index, section) in keywords {
        let Some(keyword) = get(section, "KEYWORD") else {
            notes.push(format!("{set_name}: rule {index} has no keyword"));
            continue;
        };
        let label = get(section, "DESCRIPTION")
            .map(str::to_string)
            .unwrap_or_else(|| format!("{set_name} {index}"));

        // A plain keyword has to match itself, not act as a pattern.
        let pattern = if flag(section, "USEREGEX", false) {
            keyword.to_string()
        } else {
            regex_escape(keyword)
        };

        let foreground = colour(section, "TEXTCOLORINDEX");
        // `TermBackColor=1` means "keep the terminal's background".
        let background = if flag(section, "TERMBACKCOLOR", false) {
            None
        } else {
            colour(section, "BACKCOLORINDEX")
        };

        // A rule with no colour would match and show nothing. Give it one
        // and say so, rather than importing a rule that appears broken.
        let (foreground, background) = if foreground.is_none() && background.is_none() {
            notes.push(format!(
                "{set_name}: \"{label}\" had no readable colour and was given yellow"
            ));
            (Some("#ffff55".to_string()), None)
        } else {
            (foreground, background)
        };

        rules.push(HighlightRule {
            id: uuid::Uuid::new_v4().to_string(),
            label,
            pattern,
            case_sensitive: flag(section, "CASESENS", false),
            foreground,
            background,
            enabled: flag(section, "ENABLE", true),
        });
    }

    Ok((rules, notes))
}

/// `BBGGRR` hex, as Windows writes a `COLORREF`, to `#rrggbb`.
fn colorref(hex: &str) -> Option<String> {
    let value = u32::from_str_radix(hex.trim_start_matches('#'), 16).ok()?;
    if hex.trim_start_matches('#').len() != 6 {
        return None;
    }
    let r = (value & 0xff) as u8;
    let g = ((value >> 8) & 0xff) as u8;
    let b = ((value >> 16) & 0xff) as u8;
    Some(Rgb::new(r, g, b).to_hex())
}

/// Escapes a literal so a JavaScript `RegExp` matches it verbatim.
fn regex_escape(literal: &str) -> String {
    let mut out = String::with_capacity(literal.len() + 8);
    for c in literal.chars() {
        if matches!(
            c,
            '\\' | '^' | '$' | '.' | '|' | '?' | '*' | '+' | '(' | ')' | '[' | ']' | '{' | '}'
        ) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xts::test_support::{archive, cleanup, utf16};

    /// The sample set Xshell ships, trimmed.
    const SAMPLE: &str = "\
[Keyword_0]
TermBackColor=0
Bold=1
Keyword=\\d{3}-\\d{3,4}-\\d{4}
Description=Phone number
BackColorIndex=282
UseRegex=1
Enable=1
TextColorIndex=281
CaseSens=0
[Keyword_1]
TermBackColor=1
Keyword=ERROR
Description=Errors
BackColorIndex=284
UseRegex=0
Enable=1
TextColorIndex=289
CaseSens=1
[Keyword_2]
TermBackColor=0
Keyword=\\s
Description=Space
BackColorIndex=999
UseRegex=1
Enable=0
TextColorIndex=281
[Colors]
Colors=000000,00E4FF,000040,0080FF,400000,C08080,8080FF,C0C0C0,555555,5555FF,55FF55,55FFFF,FF5555,FF55FF,FFFF55,FFFFFF
[info]
Version=1.1
Count=3
";

    #[test]
    fn reads_the_sample_set() {
        let (rules, notes) = parse_hls("Sample", SAMPLE).unwrap();

        assert_eq!(rules.len(), 3);
        let phone = &rules[0];
        assert_eq!(phone.label, "Phone number");
        assert_eq!(phone.pattern, "\\d{3}-\\d{3,4}-\\d{4}");
        assert!(!phone.case_sensitive);
        assert!(phone.enabled);
        // Index 281 is palette entry 1, written as a COLORREF: 00E4FF is
        // R=FF, G=E4, B=00 - yellow, not sky blue.
        assert_eq!(phone.foreground.as_deref(), Some("#ffe400"));
        assert_eq!(phone.background.as_deref(), Some("#400000"));
        assert!(notes.is_empty(), "{notes:?}");
    }

    #[test]
    fn a_plain_keyword_is_escaped_and_keeps_the_terminal_background() {
        let (rules, _) = parse_hls("Sample", SAMPLE).unwrap();
        let errors = &rules[1];

        assert_eq!(errors.pattern, "ERROR");
        assert!(errors.case_sensitive);
        assert_eq!(
            errors.background, None,
            "TermBackColor=1 means no background"
        );
        // 289 is palette entry 9: 5555FF as COLORREF is red.
        assert_eq!(errors.foreground.as_deref(), Some("#ff5555"));
    }

    #[test]
    fn disabled_rules_come_across_disabled_and_bad_indices_are_ignored() {
        let (rules, _) = parse_hls("Sample", SAMPLE).unwrap();
        let space = &rules[2];

        assert!(!space.enabled);
        assert_eq!(space.background, None, "index 999 is outside the palette");
        assert_eq!(space.foreground.as_deref(), Some("#ffe400"));
    }

    #[test]
    fn literals_are_escaped_so_they_match_themselves() {
        assert_eq!(
            regex_escape("1.2.3.4 (prod) [x]"),
            "1\\.2\\.3\\.4 \\(prod\\) \\[x\\]"
        );
        assert_eq!(regex_escape("plain"), "plain");
        let (rules, _) = parse_hls(
            "s",
            "[Keyword_0]\nKeyword=a.b*c\nUseRegex=0\nTextColorIndex=280\n[Colors]\nColors=FFFFFF\n",
        )
        .unwrap();
        assert_eq!(rules[0].pattern, "a\\.b\\*c");
    }

    #[test]
    fn a_rule_with_no_colour_is_given_one_and_noted() {
        let (rules, notes) = parse_hls("s", "[Keyword_0]\nKeyword=x\nDescription=Bare\n").unwrap();

        assert_eq!(rules[0].foreground.as_deref(), Some("#ffff55"));
        assert_eq!(notes.len(), 1);
        assert!(notes[0].contains("Bare"));
    }

    #[test]
    fn a_rule_with_no_keyword_is_skipped_with_a_note() {
        let (rules, notes) = parse_hls(
            "s",
            "[Keyword_0]\nDescription=Nothing\n[Keyword_1]\nKeyword=y\n",
        )
        .unwrap();

        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].pattern, "y");
        assert!(notes[0].contains("rule 0"));
    }

    #[test]
    fn rules_come_out_in_numeric_order() {
        let (rules, _) = parse_hls(
            "s",
            "[Keyword_10]\nKeyword=ten\n[Keyword_2]\nKeyword=two\n[Keyword_1]\nKeyword=one\n",
        )
        .unwrap();
        let patterns: Vec<_> = rules.iter().map(|rule| rule.pattern.as_str()).collect();
        assert_eq!(patterns, ["one", "two", "ten"]);
    }

    #[test]
    fn something_that_is_not_a_highlight_set_is_an_error() {
        assert!(parse_hls("s", "[Color Scheme]\ntext=cdcdcd\n").is_err());
    }

    #[test]
    fn colorref_is_bgr() {
        assert_eq!(colorref("5555FF").as_deref(), Some("#ff5555"));
        assert_eq!(colorref("0080FF").as_deref(), Some("#ff8000"));
        assert_eq!(colorref("FFFFFF").as_deref(), Some("#ffffff"));
        assert_eq!(colorref("12345"), None);
        assert_eq!(colorref("nope"), None);
    }

    #[test]
    fn imports_every_set_out_of_a_backup() {
        let path = archive(
            "backup.xts",
            &[
                ("xsl/HighlightSet Files/Sample.hls", utf16(SAMPLE)),
                ("xsl/HighlightSet Files/Mine.hls", utf16("[Keyword_0]\r\nKeyword=WARN\r\nTextColorIndex=280\r\n[Colors]\r\nColors=00FFFF\r\n")),
                ("xsl/HighlightSet Files/readme.txt", b"ignored".to_vec()),
            ],
        );

        let found = import(&path).unwrap();
        cleanup(&path);

        assert_eq!(found.rules.len(), 4);
        // Sets sort by name: Mine before Sample.
        assert_eq!(found.rules[0].pattern, "WARN");
        assert_eq!(found.rules[0].foreground.as_deref(), Some("#ffff00"));
        assert!(found.notes.is_empty(), "{:?}", found.notes);
    }

    #[test]
    fn imports_a_single_file_and_a_directory() {
        let dir = std::env::temp_dir().join(format!("harbour-hls-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.hls"), utf16(SAMPLE)).unwrap();
        std::fs::write(dir.join("b.hls"), utf16("[Keyword_0]\r\nKeyword=x\r\n")).unwrap();
        std::fs::write(dir.join("junk.hls"), utf16("[Color Scheme]\r\n")).unwrap();

        let one = import(&dir.join("a.hls")).unwrap();
        assert_eq!(one.rules.len(), 3);

        let all = import(&dir).unwrap();
        assert_eq!(all.rules.len(), 4);
        // The file that was not a set is reported, not dropped.
        assert!(all.notes.iter().any(|note| note.starts_with("junk:")));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_file_that_is_not_a_set_is_an_error() {
        let dir = std::env::temp_dir().join(format!("harbour-hls-bad-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("scheme.hls");
        std::fs::write(&path, utf16("[Color Scheme]\r\ntext=cdcdcd\r\n")).unwrap();

        let err = import(&path).unwrap_err();
        std::fs::remove_dir_all(dir).ok();

        assert_eq!(err.code(), "HIGHLIGHT_IMPORT_FAILED");
    }
}
