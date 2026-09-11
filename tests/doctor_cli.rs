//! `ecphory doctor` at the CLI boundary, driven against a temp $HOME that
//! stands in for a machine with harnesses installed.
//!
//! The invariant under test is the one the whole feature exists for: static
//! inspection MUST NOT claim delivery. Four of the five known silent-no-op
//! modes pass every static check, so a config that parses is not evidence that
//! text reaches the model. Only `--live` may print DELIVERED.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

fn doctor(home: &Path, args: &[&str]) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ecphory"));
    cmd.arg("doctor").args(args);
    cmd.env("HOME", home);
    // Scrub ambient config so the developer's real machine cannot leak in.
    cmd.env_remove("ECPHORY_DB");
    cmd.env_remove("CODEX_HOME");
    cmd.output().expect("run ecphory doctor")
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// A temp HOME with opencode present and correctly wired to an artifact that
/// really exists — i.e. everything a static check can see is correct.
fn home_with_wired_opencode() -> tempfile::TempDir {
    let home = tempfile::tempdir().expect("tempdir");
    let art = home.path().join("artifact.md");
    fs::write(&art, "# ecphory context\n").unwrap();
    let cfg_dir = home.path().join(".config/opencode");
    fs::create_dir_all(&cfg_dir).unwrap();
    fs::write(
        cfg_dir.join("opencode.json"),
        format!(r#"{{"instructions":["{}"]}}"#, art.to_string_lossy()),
    )
    .unwrap();
    home
}

#[test]
fn static_tier_never_reports_delivered() {
    let home = home_with_wired_opencode();
    let out = doctor(home.path(), &[]);
    let s = stdout(&out);

    assert!(
        s.contains("opencode"),
        "opencode should be detected from its config dir, got:\n{s}"
    );
    assert!(
        s.contains("UNPROVEN"),
        "a correctly-wired harness with no live run is UNPROVEN, got:\n{s}"
    );
    // The load-bearing assertion. If this ever passes because the word simply
    // isn't in the vocabulary, the mutation test below is what catches it.
    assert!(
        !s.contains("DELIVERED"),
        "static inspection must never claim delivery, got:\n{s}"
    );
}

/// CHANGED, and the requirement changed with it: this test previously asserted
/// that a detected harness with no ecphory adapter is MISCONFIGURED. That is
/// now wrong. Doctor verifies the OUTCOME, not the mechanism — delivery may be
/// carried by a rail ecphory does not own, so "no adapter I recognise" is
/// UNPROVEN, not a defect.
#[test]
fn absent_adapter_is_unproven_not_misconfigured() {
    let home = tempfile::tempdir().expect("tempdir");
    fs::create_dir_all(home.path().join(".copilot/hooks")).unwrap();

    let out = doctor(home.path(), &[]);
    let s = stdout(&out);

    assert!(
        s.contains("copilot"),
        "copilot should be detected, got:\n{s}"
    );
    assert!(
        s.contains("UNPROVEN"),
        "no recognised adapter means undetermined, not broken, got:\n{s}"
    );
    assert!(
        !s.contains("MISCONFIGURED"),
        "MISCONFIGURED is reserved for an ecphory-managed adapter that is broken, got:\n{s}"
    );
}

/// Regression for a miss observed against a real machine: opencode was reading
/// a live ecphory-generated artifact written by an external render script, and
/// doctor — looking only for its own pointer — failed to notice. A verification
/// tool that recognises only its own handiwork reports on itself.
#[test]
fn foreign_generated_artifact_is_recognised() {
    let home = tempfile::tempdir().expect("tempdir");
    let cfg = home.path().join(".config/opencode");
    fs::create_dir_all(&cfg).unwrap();
    // No `instructions` pointer at all — the artifact is reached by opencode's
    // own global AGENTS.md convention, written by something ecphory does not own.
    fs::write(cfg.join("opencode.json"), "{}").unwrap();
    fs::write(
        cfg.join("AGENTS.md"),
        "<!-- GENERATED from ecphory episode 019f7ac6-667e — DO NOT HAND-EDIT. -->\n# ctx\n",
    )
    .unwrap();

    let out = doctor(home.path(), &[]);
    let s = stdout(&out);

    assert!(
        s.contains("UNPROVEN"),
        "a recognised foreign rail is UNPROVEN pending a live check, got:\n{s}"
    );
    assert!(
        s.contains("AGENTS.md"),
        "the detail must name the artifact the live tier will test, got:\n{s}"
    );
    assert!(
        !s.contains("MISCONFIGURED"),
        "a working foreign rail must not be reported as a defect, got:\n{s}"
    );
}

#[test]
fn claude_non_relative_import_is_named_not_generic() {
    let home = tempfile::tempdir().expect("tempdir");
    let claude = home.path().join(".claude");
    fs::create_dir_all(&claude).unwrap();
    // Silent no-op #4: an ecphory-managed @import with an absolute path. This
    // resolves to nothing in Claude Code and reports no error anywhere.
    fs::write(
        claude.join("CLAUDE.md"),
        "# user\n\n<!-- BEGIN ecphory -->\n@/tmp/ecphory/context.md\n<!-- END ecphory -->\n",
    )
    .unwrap();

    let out = doctor(home.path(), &[]);
    let s = stdout(&out);

    assert!(
        s.contains("MISCONFIGURED"),
        "absolute @import must be flagged, got:\n{s}"
    );
    assert!(
        s.to_lowercase().contains("relative"),
        "the report must name the relative-path constraint, not fail generically, got:\n{s}"
    );
}

/// Regression: a codex config that merely MENTIONS ecphory somewhere unrelated
/// (a `[projects."/path/to/ecphory"]` trust entry) while also declaring
/// unrelated hooks must not be reported as a misplaced ecphory hook. Found by
/// running doctor against a real machine, where the two facts are independent
/// and ANDing them produced a confident, wrong diagnosis.
#[test]
fn codex_unrelated_ecphory_mention_is_not_a_hook_misconfiguration() {
    let home = tempfile::tempdir().expect("tempdir");
    let codex = home.path().join(".codex");
    fs::create_dir_all(&codex).unwrap();
    fs::write(
        codex.join("config.toml"),
        r#"model = "gpt-6-astra"

[[hooks.PreToolUse]]
matcher = ".*"

[[hooks.PreToolUse.hooks]]
type = "command"
command = "/opt/some-other-tool/guard hook"
timeout = 600

[projects."/Users/someone/code/ecphory"]
trust_level = "trusted"
"#,
    )
    .unwrap();

    let out = doctor(home.path(), &[]);
    let s = stdout(&out);

    assert!(
        !s.contains("does not read for hooks"),
        "an unrelated ecphory mention must not be reported as a misplaced hook, got:\n{s}"
    );
    assert!(
        s.contains("AGENTS.md"),
        "with no adapter at all, codex should be told to add the managed block, got:\n{s}"
    );
}
