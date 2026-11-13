//! Small text helpers the application and its views share. Case is folded the ASCII way throughout, as the
//! paths, names and ids they are used on call for.

/// The last component of a `/`-separated path.
#[must_use]
pub fn file_name(path: &str) -> &str {
    path.rfind('/').map_or(path, |slash| &path[slash + 1..])
}

/// A `/`-separated path without its last component; empty when it has only one.
#[must_use]
pub fn directory_name(path: &str) -> &str {
    path.rfind('/').map_or("", |slash| &path[..slash])
}

/// `text` with its ASCII letters in upper case.
#[must_use]
pub fn to_upper(text: &str) -> String {
    text.to_ascii_uppercase()
}

/// Whether `haystack` contains `needle`, ASCII case ignored. An empty needle is in everything.
#[must_use]
pub fn contains_ci(haystack: &str, needle: &str) -> bool {
    let needle = needle.as_bytes();
    needle.is_empty()
        || haystack
            .as_bytes()
            .windows(needle.len())
            .any(|window| window.eq_ignore_ascii_case(needle))
}

/// The lowest score of a haystack that matches.
const MIN_MATCH_SCORE: i32 = 1;

/// Subsequence fuzzy match score, higher is better; below 0 when `needle` does not match, and at least 1 when it
/// does (the length penalties stop there, so a match in a very long haystack still counts as one).
///
/// A haystack containing the needle outright scores highest (shorter haystacks first); otherwise each needle
/// character (spaces skipped) must appear in order, runs of consecutive hits scoring more.
#[must_use]
pub fn fuzzy_score(haystack: &str, needle: &str) -> i32 {
    if needle.is_empty() {
        return 0;
    }
    let length = i32::try_from(haystack.len()).unwrap_or(i32::MAX);
    if contains_ci(haystack, needle) {
        return (1000 - length).max(MIN_MATCH_SCORE);
    }
    let mut score = 0;
    let mut streak = 0;
    let mut rest = haystack.bytes();
    for wanted in needle.bytes().map(|byte| byte.to_ascii_lowercase()) {
        if wanted == b' ' {
            continue;
        }
        let mut found = false;
        for byte in rest.by_ref() {
            if byte.to_ascii_lowercase() == wanted {
                found = true;
                score += 10 + streak * 5;
                streak += 1;
                break;
            }
            streak = 0;
        }
        if !found {
            return -1;
        }
    }
    (score - length / 4).max(MIN_MATCH_SCORE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_components() {
        assert_eq!(file_name("src/app/main.rs"), "main.rs");
        assert_eq!(directory_name("src/app/main.rs"), "src/app");
        assert_eq!(file_name("main.rs"), "main.rs");
        assert_eq!(directory_name("main.rs"), "");
        assert_eq!(file_name("src/"), "");
        assert_eq!(directory_name("src/"), "src");
    }

    #[test]
    fn upper_case_is_ascii_only() {
        assert_eq!(to_upper("Staged files é"), "STAGED FILES é");
    }

    #[test]
    fn contains_ignores_ascii_case() {
        assert!(contains_ci("Fix the Parser", "PARSER"));
        assert!(contains_ci("Fix the Parser", "x t"));
        assert!(contains_ci("anything", ""));
        assert!(contains_ci("", ""));
        assert!(!contains_ci("Fix", "fixed"));
        assert!(!contains_ci("", "a"));
        assert!(contains_ci("Grüße", "grüß"));
        assert!(!contains_ci("GRÜSSE", "grüße"));
    }

    #[test]
    fn fuzzy_scores() {
        assert_eq!(fuzzy_score("anything", ""), 0);
        // Contained outright: 1000 less the haystack's length.
        assert_eq!(fuzzy_score("src/main.rs", "MAIN"), 1000 - 11);
        // A subsequence: 10 per hit, 5 more per hit of a run, less a quarter of the length.
        // "s" (10), "m" after a miss (10), "r" after misses (10).
        assert_eq!(fuzzy_score("src/main.rs", "smr"), 30 - 2);
        // "m", "a" (run of 2: 15), then "r" after misses.
        assert_eq!(fuzzy_score("src/main.rs", "ma r"), 10 + 15 + 10 - 2);
        assert!(fuzzy_score("src/main.rs", "xyz") < 0);
        assert!(fuzzy_score("abc", "cba") < 0);
    }

    #[test]
    fn a_match_never_scores_below_one() {
        // A long path: the length penalty would pass the hits (and 1000 for a contained needle).
        let long = format!("{}/target.rs", "dir/".repeat(400));
        assert!(long.len() > 1000);
        assert_eq!(fuzzy_score(&long, "target"), 1);
        assert!(fuzzy_score(&long, "ddt") >= 1);
        assert_eq!(fuzzy_score(&"x".repeat(5000), "xx"), 1);
        assert!(fuzzy_score(&long, "zzz") < 0);
        // Shorter haystacks keep their order.
        assert!(fuzzy_score("a/target.rs", "target") > fuzzy_score("a/b/c/target.rs", "target"));
    }
}
