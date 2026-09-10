//! The font families installed on this machine, for the settings dialog to
//! offer instead of asking people to spell a family name from memory.
//!
//! Enumeration reads every font file the platform knows about, so it is a
//! filesystem walk rather than a lookup: callers run it off the UI thread and
//! do it once per dialog, not once per keystroke.

use std::collections::BTreeMap;

use serde::Serialize;

/// One installed family, as it would be written in a CSS `font-family`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FontFamily {
    pub name: String,
    /// Any face in the family declares itself fixed-pitch. A hint for the
    /// picker to lead with terminal-shaped fonts, not a filter: plenty of
    /// monospace fonts forget to set the flag, so nothing is hidden on it.
    pub monospace: bool,
}

/// Every family the system knows about, monospace ones first, then A-Z.
pub fn installed() -> Vec<FontFamily> {
    let mut db = fontdb::Database::new();
    db.load_system_fonts();
    collect(db.faces().filter_map(|face| {
        face.families
            .first()
            .map(|(name, _)| (name.as_str(), face.monospaced))
    }))
}

/// Folds faces into families. A family with six weights is six faces with
/// the same name; it is one entry here, monospace if any face says so.
fn collect<'a>(faces: impl IntoIterator<Item = (&'a str, bool)>) -> Vec<FontFamily> {
    let mut families: BTreeMap<String, bool> = BTreeMap::new();
    for (name, monospaced) in faces {
        let name = name.trim();
        if name.is_empty() {
            continue;
        }
        let entry = families.entry(name.to_string()).or_default();
        *entry |= monospaced;
    }
    let mut out: Vec<FontFamily> = families
        .into_iter()
        .map(|(name, monospace)| FontFamily { name, monospace })
        .collect();
    out.sort_by(|a, b| {
        b.monospace
            .cmp(&a.monospace)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dedups_faces_into_families_and_leads_with_monospace() {
        let out = collect([
            ("Segoe UI", false),
            ("Segoe UI", false),
            ("cascadia Mono", true),
            ("Arial", false),
            ("JetBrains Mono", false),
            ("JetBrains Mono", true),
            ("  ", true),
        ]);
        let names: Vec<(&str, bool)> = out.iter().map(|f| (f.name.as_str(), f.monospace)).collect();
        assert_eq!(
            names,
            [
                ("cascadia Mono", true),
                ("JetBrains Mono", true),
                ("Arial", false),
                ("Segoe UI", false),
            ]
        );
    }

    #[test]
    fn system_enumeration_does_not_panic() {
        // What is installed varies by machine; the contract is only that
        // it returns, and that whatever it returns is well-formed.
        for family in installed() {
            assert!(!family.name.trim().is_empty());
        }
    }
}
