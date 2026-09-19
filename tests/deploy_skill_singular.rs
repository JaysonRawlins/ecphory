//! One deploy runbook, and it must not tell the agent it needs a human.
//!
//! `.claude/skills/deploy/` and `.claude/skills/deploy-daemon/` both described
//! deploying the binary to the `com.ecphory.server` LaunchAgent, and they
//! disagreed about who restarts it. `deploy` said the agent was blocked and
//! had to hand the line to the user ("Gavel blocks launchctl for the agent");
//! `deploy-daemon` just ran it. An agent that loaded the wrong one acted on a
//! claim the other contradicted, and the false one cost a round-trip through
//! a human on every single deploy.
//!
//! The claim was also stale twice over: the gate is Placet, not Gavel, and
//! Placet's only hardcoded `launchctl` deny is scoped to `bootout|unload|remove`
//! against Placet's *own* LaunchAgent. A kickstart on `com.ecphory.server`
//! never matched it.
//!
//! Note what this test deliberately does NOT check: anything about the live
//! Placet rule set. That is machine-local state, not repo state — a test
//! asserting it would be testing the wrong surface and would fail on any
//! machine but one. Both checks below are repo-internal: how many runbooks
//! exist, and how the restart step is phrased.

use std::path::{Path, PathBuf};

/// The command that makes a file a deploy runbook. Anything carrying this is
/// telling someone how to restart the daemon.
const RESTART_CMD: &str = "launchctl kickstart";

/// Phrases that hand the restart to a human, or assert the agent cannot do it.
///
/// Matched case-insensitively, and only against files that mention launchctl —
/// "user runs" is common enough in prose that scanning every skill would
/// invite a false positive on something unrelated to the restart step.
const DELEGATION_PHRASES: &[&str] = &[
    "user runs",
    "hand them the line",
    "blocks launchctl",
    "gavel blocks",
];

/// A delegation claim, if the text contains one.
///
/// Two shapes. The prose shape is a phrase from `DELEGATION_PHRASES`. The
/// structural shape is a `!`-prefixed launchctl line — in these skills `!` is
/// the "the human types this in their shell" marker, so the prefix alone
/// changes who the step is addressed to even with the prose removed.
fn find_delegation_claim(text: &str) -> Option<String> {
    let lower = text.to_lowercase();
    for phrase in DELEGATION_PHRASES {
        if lower.contains(phrase) {
            return Some((*phrase).to_string());
        }
    }
    for line in text.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with('!') && trimmed.contains("launchctl") {
            return Some(trimmed.to_string());
        }
    }
    None
}

/// Every `SKILL.md` under `.claude/skills/`, as (path, contents).
fn skill_files() -> Vec<(PathBuf, String)> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".claude/skills");
    let entries = std::fs::read_dir(&root)
        .unwrap_or_else(|e| panic!("read_dir {}: {e}", root.display()));

    let mut out = Vec::new();
    for entry in entries {
        let dir = entry.expect("read skills dir entry").path();
        let skill = dir.join("SKILL.md");
        if skill.is_file() {
            let body = std::fs::read_to_string(&skill)
                .unwrap_or_else(|e| panic!("read {}: {e}", skill.display()));
            out.push((skill, body));
        }
    }
    out
}

/// Path relative to the repo root, for readable panic messages.
fn rel(path: &Path) -> String {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.strip_prefix(&root).unwrap_or(path).display().to_string()
}

#[test]
fn exactly_one_skill_owns_the_restart() {
    let skills = skill_files();

    // Without this the test passes vacuously if `.claude/skills/` is moved or
    // emptied: zero files trivially contain zero copies of the restart line.
    assert!(
        !skills.is_empty(),
        "no SKILL.md found under .claude/skills/ — if the skills moved, point \
         this test at the new location instead of letting it pass over nothing."
    );

    let owners: Vec<&PathBuf> = skills
        .iter()
        .filter(|(_, body)| body.contains(RESTART_CMD))
        .map(|(path, _)| path)
        .collect();

    assert_eq!(
        owners.len(),
        1,
        "{} skills carry `{RESTART_CMD}`: {}. Two deploy runbooks drift apart — \
         that is exactly how one of them came to claim the agent was blocked \
         from the restart while the other ran it. Keep one.",
        owners.len(),
        owners.iter().map(|p| rel(p)).collect::<Vec<_>>().join(", ")
    );
}

#[test]
fn the_restart_step_is_addressed_to_the_agent() {
    let skills = skill_files();
    assert!(!skills.is_empty(), "no SKILL.md found under .claude/skills/");

    let mut checked = 0;
    for (path, body) in &skills {
        if !body.contains("launchctl") {
            continue;
        }
        checked += 1;
        if let Some(claim) = find_delegation_claim(body) {
            panic!(
                "{} hands the daemon restart to a human: {claim:?}\n\n\
                 The agent can run `{RESTART_CMD}` itself. Placet's only hardcoded \
                 launchctl deny is `(bootout|unload|remove).*placet`, which protects \
                 Placet's own LaunchAgent and cannot match a kickstart on \
                 com.ecphory.server. See {} for the full history.",
                rel(path),
                file!()
            );
        }
    }

    assert!(
        checked > 0,
        "no skill mentions launchctl, so this test checked nothing. If the deploy \
         skill was renamed or removed, update this test rather than leaving it green."
    );
}

#[test]
fn delegation_detector_actually_detects() {
    // A detector that never matches would make the guard above hollow, so the
    // exact text that motivated this test is pinned here as a positive case.
    let old_deploy_skill = "3. **Restart the daemon — the USER runs this** (Gavel blocks \
         launchctl for the agent; hand them the line, single-line only):\n   \
         ```\n   ! launchctl kickstart -k gui/$(id -u)/com.ecphory.server\n   ```";
    assert!(find_delegation_claim(old_deploy_skill).is_some());

    assert_eq!(find_delegation_claim("the USER runs this").as_deref(), Some("user runs"));
    assert_eq!(find_delegation_claim("Gavel blocks launchctl").as_deref(), Some("blocks launchctl"));
    assert_eq!(find_delegation_claim("hand them the line").as_deref(), Some("hand them the line"));
    // Prose stripped, `!` left behind: still addressed to the human.
    assert_eq!(
        find_delegation_claim("   ! launchctl kickstart -k gui/1/x").as_deref(),
        Some("! launchctl kickstart -k gui/1/x")
    );

    // Negatives: the correct phrasing, and a bare `!` with no launchctl.
    assert_eq!(
        find_delegation_claim("4. **Restart:** `launchctl kickstart -k gui/$(id -u)/com.ecphory.server`"),
        None
    );
    assert_eq!(find_delegation_claim("! cargo build --release"), None);
    assert_eq!(find_delegation_claim("the daemon runs as a LaunchAgent"), None);
}
