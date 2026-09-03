//! The wildcard matching OpenSSH uses, in `known_hosts` patterns and in
//! `ssh_config` `Host` and `Include` lines.
//!
//! It is not the shell's glob: `*` crosses any character including a path
//! separator, there are no character classes, and there is no brace expansion.
//! Matching anything else would quietly disagree with `ssh` about which
//! entries apply to a host, which is worse than not matching at all.

/// `*` matches any run of characters, `?` exactly one.
///
/// Written iteratively with backtracking so a pathological pattern cannot blow
/// the stack.
pub fn matches(pattern: &str, target: &str) -> bool {
    let pattern: Vec<char> = pattern.chars().collect();
    let target: Vec<char> = target.chars().collect();
    let (mut p, mut t) = (0usize, 0usize);
    // Where to resume if the current `*` turns out to have consumed too little.
    let (mut star, mut resume) = (None, 0usize);

    while t < target.len() {
        if p < pattern.len() && (pattern[p] == '?' || pattern[p] == target[t]) {
            p += 1;
            t += 1;
        } else if p < pattern.len() && pattern[p] == '*' {
            star = Some(p);
            resume = t;
            p += 1;
        } else if let Some(index) = star {
            p = index + 1;
            resume += 1;
            t = resume;
        } else {
            return false;
        }
    }
    pattern[p..].iter().all(|&c| c == '*')
}

/// Matches a comma-separated pattern list, where a leading `!` on any pattern
/// vetoes the whole list. Used for `known_hosts` host fields and `ssh_config`
/// `Host` lines, which share the convention.
pub fn matches_list(patterns: &str, target: &str) -> bool {
    let mut matched = false;
    for pattern in patterns.split(',') {
        let pattern = pattern.trim();
        if let Some(rest) = pattern.strip_prefix('!') {
            if matches(rest, target) {
                return false;
            }
        } else if matches(pattern, target) {
            matched = true;
        }
    }
    matched
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stars_and_question_marks_behave_like_openssh() {
        assert!(matches("*.example.com", "web.example.com"));
        assert!(!matches("*.example.com", "example.com"));
        assert!(matches("web?.example.com", "web1.example.com"));
        assert!(!matches("web?.example.com", "web12.example.com"));
        assert!(matches("*", "anything"));
        assert!(matches("10.0.*.*", "10.0.1.7"));
        assert!(!matches("10.0.*", "10.1.0.1"));
    }

    /// The first `*` has to give ground for the tail to match.
    #[test]
    fn backtracks_when_a_star_took_too_much() {
        assert!(matches("*b*c", "abxbxc"));
        assert!(matches("*.tar.gz", "a.b.tar.gz"));
    }

    #[test]
    fn an_exact_pattern_matches_only_itself() {
        assert!(matches("example.com", "example.com"));
        assert!(!matches("example.com", "example.com.evil.test"));
    }

    #[test]
    fn a_list_matches_on_any_entry() {
        assert!(matches_list("a.example,b.example", "b.example"));
        assert!(!matches_list("a.example,b.example", "c.example"));
    }

    #[test]
    fn a_negation_vetoes_the_whole_list() {
        assert!(matches_list("*.example,!admin.example", "web.example"));
        assert!(!matches_list("*.example,!admin.example", "admin.example"));
    }

    /// Order must not matter: a veto listed first still wins.
    #[test]
    fn a_negation_wins_wherever_it_appears() {
        assert!(!matches_list("!admin.example,*.example", "admin.example"));
    }
}
