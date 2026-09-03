//! Decoding text files that were not written for us.
//!
//! Xshell writes every one of its files - sessions, colour schemes, highlight
//! sets - as UTF-16LE with a byte order mark, which is what a Windows program
//! saving "Unicode" produces. Reading those as UTF-8 does not fail; it yields
//! `[\0C\0O\0N\0...`, and every section header quietly stops matching. That
//! is how the milestone 3 Xshell import came to skip every real session file.

use std::collections::BTreeMap;

/// Turns the bytes of a text file into a string, honouring a byte order mark
/// and falling back to lossy UTF-8.
///
/// UTF-16 without a mark is recognised too, by the tell-tale zero high bytes
/// of Latin text - cheap, and the only way a hand-edited file that lost its
/// BOM still reads.
pub fn decode(bytes: &[u8]) -> String {
    match bytes {
        [0xef, 0xbb, 0xbf, rest @ ..] => String::from_utf8_lossy(rest).into_owned(),
        [0xff, 0xfe, rest @ ..] => utf16(rest, u16::from_le_bytes),
        [0xfe, 0xff, rest @ ..] => utf16(rest, u16::from_be_bytes),
        _ if looks_like_utf16le(bytes) => utf16(bytes, u16::from_le_bytes),
        _ => String::from_utf8_lossy(bytes).into_owned(),
    }
}

fn utf16(bytes: &[u8], unit: fn([u8; 2]) -> u16) -> String {
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|pair| unit([pair[0], pair[1]]))
        .collect();
    // A stray trailing byte is a truncated file, not a reason to drop the
    // rest of it.
    String::from_utf16_lossy(&units)
}

/// Text that is mostly ASCII encoded as UTF-16LE has a zero in every second
/// byte. UTF-8 text essentially never does.
fn looks_like_utf16le(bytes: &[u8]) -> bool {
    let sample = &bytes[..bytes.len().min(64)];
    if sample.len() < 4 {
        return false;
    }
    let odd_zeroes = sample
        .iter()
        .skip(1)
        .step_by(2)
        .filter(|b| **b == 0)
        .count();
    let even_zeroes = sample.iter().step_by(2).filter(|b| **b == 0).count();
    odd_zeroes * 2 >= sample.len() / 2 && even_zeroes == 0
}

/// Case-insensitive INI, tolerant of the quirks real Xshell files show: a
/// byte order mark, CRLF endings, `;`/`#` comments, blank lines, and values
/// that themselves contain `=`. Section and key names come back upper-cased.
pub fn ini(contents: &str) -> BTreeMap<String, BTreeMap<String, String>> {
    let mut sections: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    let mut current = String::new();

    for raw_line in contents.trim_start_matches('\u{feff}').lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
            continue;
        }
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            current = name.trim().to_ascii_uppercase();
            sections.entry(current.clone()).or_default();
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            sections
                .entry(current.clone())
                .or_default()
                .insert(key.trim().to_ascii_uppercase(), value.trim().to_string());
        }
    }
    sections
}

/// One value out of an [`ini`] result, treating an empty value as absent.
pub fn ini_get<'a>(
    sections: &'a BTreeMap<String, BTreeMap<String, String>>,
    section: &str,
    key: &str,
) -> Option<&'a str> {
    sections
        .get(section)?
        .get(key)
        .map(|value| value.as_str())
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ini_upper_cases_names_and_keeps_values_verbatim() {
        let sections = ini(
            "\u{feff}; comment\r\n[Color Scheme]\r\ntext=cdcdcd\r\nDescription=a=b\r\n\r\n[empty]\r\n",
        );
        assert_eq!(ini_get(&sections, "COLOR SCHEME", "TEXT"), Some("cdcdcd"));
        assert_eq!(
            ini_get(&sections, "COLOR SCHEME", "DESCRIPTION"),
            Some("a=b")
        );
        assert!(sections.contains_key("EMPTY"));
        assert_eq!(ini_get(&sections, "COLOR SCHEME", "MISSING"), None);
    }

    #[test]
    fn ini_treats_an_empty_value_as_absent() {
        let sections = ini("[A]\nk=\n");
        assert_eq!(ini_get(&sections, "A", "K"), None);
    }

    fn utf16le(text: &str, bom: bool) -> Vec<u8> {
        let mut bytes = if bom { vec![0xff, 0xfe] } else { Vec::new() };
        for unit in text.encode_utf16() {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        bytes
    }

    #[test]
    fn reads_what_xshell_actually_writes() {
        let bytes = utf16le("[CONNECTION]\r\nHost=web\r\n", true);
        assert_eq!(decode(&bytes), "[CONNECTION]\r\nHost=web\r\n");
    }

    #[test]
    fn reads_big_endian_too() {
        let mut bytes = vec![0xfe, 0xff];
        for unit in "[A]\nk=v".encode_utf16() {
            bytes.extend_from_slice(&unit.to_be_bytes());
        }
        assert_eq!(decode(&bytes), "[A]\nk=v");
    }

    #[test]
    fn strips_a_utf8_bom_and_leaves_plain_utf8_alone() {
        assert_eq!(decode(b"\xef\xbb\xbf[A]\nk=v"), "[A]\nk=v");
        assert_eq!(decode(b"[A]\nk=v"), "[A]\nk=v");
        assert_eq!(decode("[A]\nname=Zoë".as_bytes()), "[A]\nname=Zoë");
    }

    #[test]
    fn recognises_utf16le_that_lost_its_mark() {
        let bytes = utf16le("[CONNECTION]\r\nHost=web.example.com\r\n", false);
        assert_eq!(decode(&bytes), "[CONNECTION]\r\nHost=web.example.com\r\n");
    }

    #[test]
    fn does_not_mistake_short_or_binary_input_for_utf16() {
        assert_eq!(decode(b"ab"), "ab");
        // Zero bytes in even positions rule UTF-16LE out.
        let mixed = b"\0a\0b\0c\0d\0e\0f";
        assert!(!looks_like_utf16le(mixed));
    }

    #[test]
    fn a_truncated_trailing_byte_does_not_lose_the_file() {
        let mut bytes = utf16le("Host=web", true);
        bytes.push(0x41);
        assert_eq!(decode(&bytes), "Host=web");
    }

    #[test]
    fn non_latin_text_survives() {
        let bytes = utf16le("Description=服务器 – prod", true);
        assert_eq!(decode(&bytes), "Description=服务器 – prod");
    }
}
