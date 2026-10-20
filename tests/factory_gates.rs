//! The OpenSpec gates the software factory's workflows run:
//! `scripts/factory/check-tasks-complete.sh` (openspec-tasks-verify) and
//! `scripts/factory/check-change-archived.sh` (openspec-archive-verify).
//!
//! Each workflow's assert passes only when its script exits 0, so these scripts
//! decide whether the factory may advance. Each test builds a throwaway git
//! repo holding one OpenSpec change and runs the real `openspec` CLI against
//! it. CI installs openspec for this. A machine without it fails here rather
//! than skipping, because a gate test that skips proves nothing.
//!
//! Red-proofs, 2026-10-19. Against `exit 0` stubs, every test failed except the
//! committed-archive pass case. Removing one guard at a time from the real
//! scripts failed exactly one test each time: the status check, the strict
//! validate, the change-folder check and the uncommitted check. The last two
//! are layered: without them the gate still refuses, on a later check, so the
//! failure comes from the stderr assertion naming which check fired. That
//! assertion is what keeps each check's own test meaningful.
//!
//! The placeholder-Purpose check was added after archiving this repo's own
//! factory change: `openspec archive` wrote `TBD - created by archiving ...`,
//! which passes `--strict`. Without the check `archive_gate_fails_on_a_
//! placeholder_purpose` saw rc=0. A fixture Purpose under 50 characters fails
//! `--strict` ("Purpose section is too brief"), hence the long one.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const PROPOSAL: &str = "## Why\n\nA fixture change for the factory gate tests.\n\n\
## What Changes\n\n- Adds one requirement.\n\n## Impact\n\n- None.\n";

const DELTA: &str = "## ADDED Requirements\n\n### Requirement: Demo behaviour\n\
The system SHALL do the demo thing.\n\n#### Scenario: Demo runs\n\
- **WHEN** the demo is invoked\n- **THEN** it does the thing\n";

/// `1.1` starts unticked. The Live verification bullets carry no checkbox, so
/// openspec must not count them as tasks; the factory's planning prompt relies
/// on that.
const TASKS: &str = "## 1. Work\n\n- [ ] 1.1 Do the thing\n- [x] 1.2 Already done\n\n\
## Live verification\n\n- probe the scratch daemon\n";

struct Fixture {
    dir: tempfile::TempDir,
}

impl Fixture {
    /// A committed repo with the change `demo` in progress (1 of 2 tasks).
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let fx = Fixture { dir };
        let change = fx.root().join("openspec/changes/demo");
        fs::create_dir_all(change.join("specs/demo-cap")).unwrap();
        fs::write(fx.root().join("openspec/config.yaml"), "schema: spec-driven\n").unwrap();
        fs::write(change.join("proposal.md"), PROPOSAL).unwrap();
        fs::write(change.join("tasks.md"), TASKS).unwrap();
        fs::write(change.join("specs/demo-cap/spec.md"), DELTA).unwrap();
        fx.git(&["init", "-q"]);
        fx.commit("fixture");
        fx
    }

    fn root(&self) -> &Path {
        self.dir.path()
    }

    /// Fixture commits are unsigned and skip the machine's global commit-msg
    /// hook: they are throwaway test data, not commits to any real repo.
    fn git(&self, args: &[&str]) {
        let out = Command::new("git")
            .args(["-c", "core.hooksPath=/dev/null", "-c", "commit.gpgsign=false"])
            .args(["-c", "user.name=fixture", "-c", "user.email=fixture@example.invalid"])
            .args(args)
            .current_dir(self.root())
            .output()
            .expect("run git");
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    }

    fn commit(&self, msg: &str) {
        self.git(&["add", "-A"]);
        self.git(&["commit", "-q", "-m", msg]);
    }

    fn tick_all(&self) {
        let tasks = self.root().join("openspec/changes/demo/tasks.md");
        let body = fs::read_to_string(&tasks).unwrap().replace("- [ ]", "- [x]");
        fs::write(tasks, body).unwrap();
    }

    fn openspec(&self, args: &[&str]) {
        let out = Command::new("openspec")
            .args(args)
            .current_dir(self.root())
            .output()
            .expect("openspec must be on PATH for the factory gate tests");
        assert!(out.status.success(), "openspec {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    }

    fn gate(&self, script: &str, args: &[&str]) -> Output {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scripts/factory").join(script);
        Command::new("bash")
            .arg(&path)
            .args(args)
            .current_dir(self.root())
            .output()
            .unwrap_or_else(|e| panic!("run {}: {e}", path.display()))
    }
}

fn assert_gate(out: &Output, pass: bool, why: &str, stderr_has: &str) {
    let stderr = String::from_utf8_lossy(&out.stderr);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(
        out.status.success(),
        pass,
        "{why}: rc={:?}\nstdout: {stdout}\nstderr: {stderr}",
        out.status.code()
    );
    assert!(stderr.contains(stderr_has), "{why}: stderr lacks {stderr_has:?}: {stderr}");
}

#[test]
fn tasks_gate_fails_on_an_unticked_task() {
    let fx = Fixture::new();
    let out = fx.gate("check-tasks-complete.sh", &["demo"]);
    assert_gate(&out, false, "1 of 2 tasks ticked", "1/2");
}

#[test]
fn tasks_gate_passes_once_every_task_is_ticked() {
    let fx = Fixture::new();
    fx.tick_all();
    let out = fx.gate("check-tasks-complete.sh", &["demo"]);
    assert_gate(&out, true, "2 of 2 tasks ticked", "");
    assert!(String::from_utf8_lossy(&out.stdout).contains("all 2 tasks complete"));
}

#[test]
fn tasks_gate_fails_on_an_unknown_or_missing_change() {
    let fx = Fixture::new();
    fx.tick_all();
    assert_gate(&fx.gate("check-tasks-complete.sh", &["nope"]), false, "unknown slug", "not found");
    assert_gate(&fx.gate("check-tasks-complete.sh", &[]), false, "no slug", "usage");
}

#[test]
fn archive_gate_fails_while_the_change_folder_exists() {
    let fx = Fixture::new();
    fx.tick_all();
    fx.commit("tick");
    let out = fx.gate("check-change-archived.sh", &["demo"]);
    assert_gate(&out, false, "not archived", "still exists");
}

#[test]
fn archive_gate_fails_on_an_uncommitted_archive() {
    let fx = Fixture::new();
    fx.tick_all();
    fx.openspec(&["archive", "demo", "--yes"]);
    let out = fx.gate("check-change-archived.sh", &["demo"]);
    assert_gate(&out, false, "archive not committed", "uncommitted");
}

impl Fixture {
    /// `openspec archive` gives a new capability `Purpose: TBD - created by
    /// archiving...`; the archive stage replaces it, as every spec here has.
    fn fill_purpose(&self) {
        let spec = self.root().join("openspec/specs/demo-cap/spec.md");
        let body = fs::read_to_string(&spec).unwrap();
        let tbd = body.lines().find(|l| l.starts_with("TBD - created by archiving")).expect("TBD purpose").to_string();
        fs::write(spec, body.replace(&tbd, "A demo capability, here only so the factory gate tests have a spec to merge.")).unwrap();
    }
}

#[test]
fn archive_gate_passes_on_a_committed_archive() {
    let fx = Fixture::new();
    fx.tick_all();
    fx.openspec(&["archive", "demo", "--yes"]);
    fx.fill_purpose();
    fx.commit("archive demo");
    let out = fx.gate("check-change-archived.sh", &["demo"]);
    assert_gate(&out, true, "committed archive", "");
}

#[test]
fn archive_gate_fails_on_a_placeholder_purpose() {
    let fx = Fixture::new();
    fx.tick_all();
    fx.openspec(&["archive", "demo", "--yes"]);
    fx.commit("archive demo, purpose left as TBD");
    let out = fx.gate("check-change-archived.sh", &["demo"]);
    assert_gate(&out, false, "TBD purpose", "Purpose");
}

#[test]
fn archive_gate_fails_when_the_merged_specs_are_not_strictly_valid() {
    let fx = Fixture::new();
    fx.tick_all();
    fx.openspec(&["archive", "demo", "--yes"]);
    fx.fill_purpose();
    // A requirement with no SHALL/MUST fails `openspec validate --specs --strict`.
    let spec = fx.root().join("openspec/specs/demo-cap/spec.md");
    let body = fs::read_to_string(&spec).unwrap().replace("SHALL do", "does");
    fs::write(spec, body).unwrap();
    fx.commit("archive demo, then break the merged spec");
    let out = fx.gate("check-change-archived.sh", &["demo"]);
    assert_gate(&out, false, "merged spec invalid", "strict");
}
