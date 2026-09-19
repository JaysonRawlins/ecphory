//! The README's `## Status` section may not pin a version.
//!
//! It did, and the line went stale three times in two days: v0.3.5 against a
//! 0.3.6 Cargo.toml, then v0.3.7 released mid-edit, then v0.3.7 sitting there
//! while main was 0.3.8 — and v0.3.7 never got a GitHub Release at all, so the
//! README was advertising a version nobody could install. Every catch came
//! from someone re-fetching origin just before merging.
//!
//! Note the check that is NOT here. Requiring the README to AGREE with
//! Cargo.toml is the obvious version and it breaks releases: release-plz bumps
//! Cargo.toml and leaves the README alone, so its own release PR would fail.
//! Requiring the README to be SILENT costs nothing at release time.

use std::path::PathBuf;

/// A release pin: `v` then three dot-separated numbers.
///
/// The `v` is required. Every real instance had it (tags and CHANGELOG
/// headings are `v0.3.8`), and a bare `N.N.N` would fire on the `127.0.0.1`
/// that appears all over this README. A pin written without the `v` is caught
/// by the `CARGO_PKG_VERSION` check instead.
fn find_v_pin(haystack: &str) -> Option<String> {
    let bytes = haystack.as_bytes();
    for (i, _) in haystack.match_indices('v') {
        // Must start a word, or `rev1.2.3` and `@v4` read as pins.
        if i > 0 && (bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'.') {
            continue;
        }
        let rest = &haystack[i + 1..];
        if let Some(len) = dotted_triple_len(rest) {
            return Some(format!("v{}", &rest[..len]));
        }
    }
    None
}

/// Length of a leading `<digits>.<digits>.<digits>` run, if there is one.
fn dotted_triple_len(s: &str) -> Option<usize> {
    let mut pos = 0;
    for part in 0..3 {
        if part > 0 {
            if s.as_bytes().get(pos) != Some(&b'.') {
                return None;
            }
            pos += 1;
        }
        let digits = s[pos..].bytes().take_while(u8::is_ascii_digit).count();
        if digits == 0 {
            return None;
        }
        pos += digits;
    }
    Some(pos)
}

/// The body of a `## <name>` section, up to the next `## ` heading.
fn section<'a>(readme: &'a str, heading: &str) -> Option<&'a str> {
    let start = readme.find(heading)? + heading.len();
    let body = &readme[start..];
    let end = body.find("\n## ").unwrap_or(body.len());
    Some(&body[..end])
}

fn readme() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("README.md");
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

#[test]
fn status_section_pins_no_version() {
    let readme = readme();
    let status = section(&readme, "\n## Status\n").expect(
        "README has no `## Status` section. If it was renamed, rename it here too — \
         otherwise this test passes over nothing.",
    );

    // Both checks below are satisfied by an empty string.
    assert!(
        status.trim().len() > 200,
        "`## Status` is {} bytes of content — too short to be the real section, so the \
         checks below would pass vacuously.",
        status.trim().len()
    );

    if let Some(pin) = find_v_pin(status) {
        panic!(
            "`## Status` pins {pin}. That number goes stale on the next release and nobody \
             reads Status for it. Say it in CHANGELOG.md and let the releases page be the \
             version of record."
        );
    }

    let current = env!("CARGO_PKG_VERSION");
    assert!(
        !status.contains(current),
        "`## Status` names the current crate version ({current}). Same problem without \
         the `v`: release-plz bumps Cargo.toml and does not touch the README, so this \
         line is wrong the moment the next release lands."
    );

    // Status points here so the next editor knows the rule is enforced. `file!()`
    // rather than a literal, so renaming this file fails here instead of quietly
    // leaving the README pointing at nothing.
    assert!(
        status.contains(file!()),
        "`## Status` should point at `{}` so the next person editing it knows the \
         rule is enforced and where.",
        file!()
    );
}

#[test]
fn v_pin_detector_actually_detects() {
    // Negative cases included deliberately: a matcher that never matches would
    // make the guard above pass on any README at all.
    assert_eq!(find_v_pin("v0.3.7. Canonical store,").as_deref(), Some("v0.3.7"));
    assert_eq!(find_v_pin("landed in v0.3.4. v0.3.7 added").as_deref(), Some("v0.3.4"));
    assert_eq!(find_v_pin("Since v0.3.3 the recorder").as_deref(), Some("v0.3.3"));
    assert_eq!(find_v_pin("v1.0.0-rc.1").as_deref(), Some("v1.0.0"));

    assert_eq!(find_v_pin("http://127.0.0.1:3491/mcp"), None);
    assert_eq!(find_v_pin("Pre-1.0. Canonical store,"), None);
    assert_eq!(find_v_pin("actions/checkout@v4"), None);
    assert_eq!(find_v_pin("v0.3"), None);
    assert_eq!(find_v_pin("rev1.2.3"), None);
}
