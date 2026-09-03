//! Reading `~/.ssh/config` into importable hosts.
//!
//! This is not a full `ssh_config` implementation and does not try to be. It
//! answers one question - "what hosts does this person already have names
//! for?" - and answers it the way `ssh` would for the keywords that matter to
//! a session list: `HostName`, `Port`, `User` and `IdentityFile`.
//!
//! Two rules of the real format are worth keeping in mind, because getting
//! them wrong would silently import the wrong values:
//!
//! * **First value wins.** Unlike most config formats, `ssh_config` keeps the
//!   *earliest* setting for a keyword, so a later block cannot override an
//!   earlier one.
//! * **`Host` takes patterns.** A block whose patterns contain wildcards - the
//!   near-universal `Host *` - configures other hosts rather than being one,
//!   so it contributes defaults but is not itself importable.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::glob;

/// A host worth offering to import.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigHost {
    /// The `Host` alias, which is what the user actually types.
    pub alias: String,
    /// `HostName`, falling back to the alias, as `ssh` does.
    pub hostname: String,
    pub port: u16,
    /// `User`, or `None` when the config leaves it to the local username.
    pub user: Option<String>,
    /// The first `IdentityFile`, if the block names one.
    pub identity_file: Option<String>,
}

/// What a parse produced, and what it could not do.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigImport {
    pub hosts: Vec<ConfigHost>,
    /// Files that were named by an `Include` but could not be read, and other
    /// things worth telling the user rather than swallowing.
    pub notes: Vec<String>,
}

/// The user's own config, if they have one.
pub fn default_path() -> Option<PathBuf> {
    dirs::home_dir().map(|home| home.join(".ssh").join("config"))
}

/// Parses `path`, following `Include` directives.
pub fn read(path: &Path) -> ConfigImport {
    let mut import = ConfigImport::default();
    let mut blocks = Vec::new();
    // `Include` can nest, and a config that includes itself would otherwise
    // spin forever.
    let mut visited = Vec::new();

    collect(path, &mut blocks, &mut visited, &mut import.notes, 0);
    import.hosts = resolve(&blocks);
    import
}

/// Parses config text directly, without touching the filesystem. `Include` is
/// reported rather than followed.
pub fn parse(contents: &str) -> ConfigImport {
    let mut import = ConfigImport::default();
    let mut blocks = Vec::new();
    parse_into(contents, &mut blocks, &mut |note| import.notes.push(note));
    import.hosts = resolve(&blocks);
    import
}

/// One `Host` stanza: the patterns it applies to and the keywords under it.
#[derive(Debug, Clone, Default)]
struct Block {
    patterns: Vec<String>,
    /// Lower-cased keyword to first value seen, since first value wins.
    settings: BTreeMap<String, String>,
}

impl Block {
    /// A block is a host to import only if it names exactly one thing. `Host *`
    /// and `Host web-*` configure other hosts; they are not hosts.
    fn concrete_alias(&self) -> Option<&str> {
        match self.patterns.as_slice() {
            [only] if !only.contains(['*', '?', '!']) => Some(only),
            _ => None,
        }
    }
}

const MAX_INCLUDE_DEPTH: usize = 16;

fn collect(
    path: &Path,
    blocks: &mut Vec<Block>,
    visited: &mut Vec<PathBuf>,
    notes: &mut Vec<String>,
    depth: usize,
) {
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    if visited.contains(&canonical) {
        notes.push(format!("{} was already included; skipped", path.display()));
        return;
    }
    if depth > MAX_INCLUDE_DEPTH {
        notes.push(format!(
            "{} is nested more than {MAX_INCLUDE_DEPTH} includes deep; skipped",
            path.display()
        ));
        return;
    }
    visited.push(canonical);

    let contents = match std::fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(err) => {
            notes.push(format!("could not read {}: {err}", path.display()));
            return;
        }
    };

    let mut includes = Vec::new();
    parse_into(&contents, blocks, &mut |note| includes.push(note));

    for include in includes {
        for included in expand_include(&include, path) {
            collect(&included, blocks, visited, notes, depth + 1);
        }
    }
}

/// Splits config text into blocks. `on_include` receives the raw argument of
/// each `Include` line, so callers decide whether to follow them.
fn parse_into(contents: &str, blocks: &mut Vec<Block>, on_include: &mut dyn FnMut(String)) {
    // Settings before the first `Host` line apply to everything.
    let mut current = Block {
        patterns: vec!["*".to_string()],
        settings: BTreeMap::new(),
    };

    for line in contents.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        // Keyword and value may be separated by whitespace or by `=`.
        let (keyword, value) = match line.split_once(['=', ' ', '\t']) {
            Some((keyword, value)) => (keyword.trim(), value.trim_start_matches('=').trim()),
            None => continue,
        };
        let keyword = keyword.to_ascii_lowercase();

        match keyword.as_str() {
            "host" => {
                blocks.push(std::mem::take(&mut current));
                current.patterns = split_patterns(value);
            }
            // A `Match` block's conditions are not modelled, so anything under
            // one is skipped rather than mis-attributed to the host above it.
            "match" => {
                blocks.push(std::mem::take(&mut current));
                current.patterns = Vec::new();
            }
            "include" => on_include(value.to_string()),
            _ => {
                current
                    .settings
                    .entry(keyword)
                    .or_insert_with(|| unquote(value));
            }
        }
    }
    blocks.push(current);
}

/// Turns each concrete `Host` block into a host, with wildcard blocks
/// contributing defaults the way `ssh` applies them.
fn resolve(blocks: &[Block]) -> Vec<ConfigHost> {
    blocks
        .iter()
        .filter_map(|block| {
            let alias = block.concrete_alias()?;
            let setting = |keyword: &str| lookup(blocks, alias, keyword);

            Some(ConfigHost {
                hostname: setting("hostname").unwrap_or_else(|| alias.to_string()),
                port: setting("port")
                    .and_then(|port| port.parse().ok())
                    .filter(|port| *port != 0)
                    .unwrap_or(22),
                user: setting("user"),
                identity_file: setting("identityfile"),
                alias: alias.to_string(),
            })
        })
        .collect()
}

/// The value `ssh` would use for `keyword` when connecting to `alias`: the
/// first one set by any block whose patterns match.
fn lookup(blocks: &[Block], alias: &str, keyword: &str) -> Option<String> {
    blocks
        .iter()
        .filter(|block| {
            !block.patterns.is_empty()
                && block
                    .patterns
                    .iter()
                    .any(|pattern| glob::matches_list(pattern, alias))
        })
        .find_map(|block| block.settings.get(keyword).cloned())
}

fn split_patterns(value: &str) -> Vec<String> {
    value
        .split_whitespace()
        .map(unquote)
        .filter(|pattern| !pattern.is_empty())
        .collect()
}

fn unquote(value: &str) -> String {
    let trimmed = value.trim();
    trimmed
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
        .unwrap_or(trimmed)
        .to_string()
}

/// Resolves an `Include` argument to the files it names.
///
/// Relative paths are taken against `~/.ssh`, as `ssh` does for a user config,
/// and a trailing wildcard in the file name is expanded by listing the
/// directory. Directory components are not globbed - `ssh` allows it, but it
/// is vanishingly rare and matching it wrongly would be worse than not
/// matching it.
fn expand_include(argument: &str, parent: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();

    for token in argument.split_whitespace() {
        let token = unquote(token);
        let path = if let Some(rest) = token.strip_prefix("~/") {
            match dirs::home_dir() {
                Some(home) => home.join(rest),
                None => continue,
            }
        } else {
            let candidate = Path::new(&token);
            if candidate.is_absolute() {
                candidate.to_path_buf()
            } else {
                // Relative to the including file's directory, which for the
                // usual `~/.ssh/config` is `~/.ssh`.
                parent
                    .parent()
                    .unwrap_or_else(|| Path::new("."))
                    .join(candidate)
            }
        };

        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !name.contains(['*', '?']) {
            out.push(path);
            continue;
        }

        let Some(directory) = path.parent() else {
            continue;
        };
        let Ok(entries) = std::fs::read_dir(directory) else {
            continue;
        };
        let mut matched: Vec<PathBuf> = entries
            .flatten()
            .filter(|entry| entry.path().is_file())
            .filter(|entry| {
                entry
                    .file_name()
                    .to_str()
                    .is_some_and(|candidate| glob::matches(name, candidate))
            })
            .map(|entry| entry.path())
            .collect();
        // Directory order is not defined; sorting makes an import reproducible.
        matched.sort();
        out.extend(matched);
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host<'a>(import: &'a ConfigImport, alias: &str) -> &'a ConfigHost {
        import
            .hosts
            .iter()
            .find(|host| host.alias == alias)
            .unwrap_or_else(|| panic!("expected a host called {alias}: {:?}", import.hosts))
    }

    #[test]
    fn reads_a_plain_block() {
        let import = parse("Host web\n  HostName web.example.com\n  User deploy\n  Port 2222\n");

        assert_eq!(import.hosts.len(), 1);
        let web = host(&import, "web");
        assert_eq!(web.hostname, "web.example.com");
        assert_eq!(web.user.as_deref(), Some("deploy"));
        assert_eq!(web.port, 2222);
    }

    #[test]
    fn an_alias_with_no_hostname_is_its_own_hostname() {
        let import = parse("Host example.com\n  User deploy\n");
        assert_eq!(host(&import, "example.com").hostname, "example.com");
    }

    #[test]
    fn the_default_port_is_22() {
        let import = parse("Host web\n  HostName web.example.com\n");
        assert_eq!(host(&import, "web").port, 22);
    }

    /// `Host *` is how nearly every real config sets defaults. Importing it as
    /// a host called `*` would be nonsense.
    #[test]
    fn wildcard_blocks_configure_hosts_but_are_not_hosts() {
        let import = parse("Host *\n  User default\n\nHost web\n  HostName web.example.com\n");

        assert_eq!(import.hosts.len(), 1);
        assert_eq!(host(&import, "web").user.as_deref(), Some("default"));
    }

    #[test]
    fn a_pattern_block_supplies_defaults_to_the_hosts_it_matches() {
        let import = parse(
            "Host *.example.com\n  User deploy\n  Port 2222\n\n\
             Host web.example.com\n\n\
             Host db.internal\n",
        );

        assert_eq!(
            host(&import, "web.example.com").user.as_deref(),
            Some("deploy")
        );
        assert_eq!(host(&import, "web.example.com").port, 2222);
        assert_eq!(host(&import, "db.internal").user, None);
        assert_eq!(host(&import, "db.internal").port, 22);
    }

    /// The rule that catches people out: `ssh` keeps the *first* value, not
    /// the last. Getting this backwards would import the wrong user.
    #[test]
    fn the_first_value_for_a_keyword_wins() {
        let import = parse("Host web\n  User first\n  User second\n  HostName web.example.com\n");
        assert_eq!(host(&import, "web").user.as_deref(), Some("first"));
    }

    #[test]
    fn a_host_block_beats_a_later_wildcard_block() {
        let import = parse("Host web\n  User specific\n\nHost *\n  User general\n");
        assert_eq!(host(&import, "web").user.as_deref(), Some("specific"));
    }

    /// A wildcard block placed first genuinely does win in `ssh`, however
    /// surprising that is, so the import has to agree with it.
    #[test]
    fn a_wildcard_block_placed_first_wins() {
        let import = parse("Host *\n  User general\n\nHost web\n  User specific\n");
        assert_eq!(host(&import, "web").user.as_deref(), Some("general"));
    }

    #[test]
    fn settings_above_the_first_host_line_apply_to_everything() {
        let import = parse("User global\n\nHost web\n  HostName web.example.com\n");
        assert_eq!(host(&import, "web").user.as_deref(), Some("global"));
    }

    #[test]
    fn keywords_are_case_insensitive_and_may_use_equals() {
        let import = parse("Host web\n  hostname=web.example.com\n  PORT = 2200\n");
        let web = host(&import, "web");
        assert_eq!(web.hostname, "web.example.com");
        assert_eq!(web.port, 2200);
    }

    #[test]
    fn quoted_values_lose_their_quotes() {
        let import = parse("Host web\n  HostName \"web.example.com\"\n");
        assert_eq!(host(&import, "web").hostname, "web.example.com");
    }

    #[test]
    fn comments_and_blank_lines_are_ignored() {
        let import = parse("# a comment\n\nHost web\n  # another\n  HostName web.example.com\n");
        assert_eq!(import.hosts.len(), 1);
    }

    #[test]
    fn one_host_line_can_name_several_patterns() {
        // Several aliases for one config is common, but there is no single
        // name to import it under, so it contributes settings only.
        let import = parse("Host web web1 web2\n  HostName web.example.com\n");
        assert!(import.hosts.is_empty());
    }

    #[test]
    fn the_first_identity_file_is_taken() {
        let import =
            parse("Host web\n  IdentityFile ~/.ssh/id_ed25519\n  IdentityFile ~/.ssh/id_rsa\n");
        assert_eq!(
            host(&import, "web").identity_file.as_deref(),
            Some("~/.ssh/id_ed25519")
        );
    }

    /// `Match` conditions are not modelled, so its settings must not be
    /// attributed to the preceding host.
    #[test]
    fn a_match_block_does_not_leak_into_the_host_above_it() {
        let import =
            parse("Host web\n  HostName web.example.com\n\nMatch user root\n  Port 2222\n");
        assert_eq!(host(&import, "web").port, 22);
    }

    #[test]
    fn a_config_with_no_hosts_yields_nothing() {
        assert!(parse("").hosts.is_empty());
        assert!(parse("# only comments\n").hosts.is_empty());
    }

    // -- Include -----------------------------------------------------------

    fn scratch(files: &[(&str, &str)]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("harbour-sshcfg-{}", uuid::Uuid::new_v4()));
        for (name, contents) in files {
            let path = dir.join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, contents).unwrap();
        }
        dir
    }

    #[test]
    fn include_pulls_in_another_file() {
        let dir = scratch(&[
            (
                "config",
                "Include extra\nHost web\n  HostName web.example.com\n",
            ),
            ("extra", "Host db\n  HostName db.example.com\n"),
        ]);

        let import = read(&dir.join("config"));

        assert_eq!(import.hosts.len(), 2);
        assert_eq!(host(&import, "db").hostname, "db.example.com");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn include_expands_a_wildcard_in_the_file_name() {
        let dir = scratch(&[
            ("config", "Include conf.d/*\n"),
            ("conf.d/a", "Host a\n  HostName a.example.com\n"),
            ("conf.d/b", "Host b\n  HostName b.example.com\n"),
        ]);

        let import = read(&dir.join("config"));

        assert_eq!(import.hosts.len(), 2);
        assert_eq!(host(&import, "a").hostname, "a.example.com");
        assert_eq!(host(&import, "b").hostname, "b.example.com");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_missing_include_is_reported_rather_than_failing_the_import() {
        let dir = scratch(&[(
            "config",
            "Include nope\nHost web\n  HostName web.example.com\n",
        )]);

        let import = read(&dir.join("config"));

        assert_eq!(import.hosts.len(), 1);
        assert_eq!(import.notes.len(), 1);
        assert!(import.notes[0].contains("could not read"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_config_that_includes_itself_terminates() {
        let dir = scratch(&[(
            "config",
            "Include config\nHost web\n  HostName web.example.com\n",
        )]);

        let import = read(&dir.join("config"));

        assert_eq!(import.hosts.len(), 1);
        assert!(import
            .notes
            .iter()
            .any(|note| note.contains("already included")));
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Parsing text on its own cannot follow includes; it must not pretend it
    /// did, and must not fail either.
    #[test]
    fn parsing_text_ignores_include_without_complaint() {
        let import = parse("Include somewhere\nHost web\n  HostName web.example.com\n");
        assert_eq!(import.hosts.len(), 1);
    }
}
