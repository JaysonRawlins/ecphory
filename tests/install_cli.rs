//! `ecphory install` planning, driven against a temp $HOME.
//!
//! Dry-run is the DEFAULT (the same shape `purge` uses): the plan prints and
//! nothing is written unless `--apply` is passed.
//!
//! The collision case is the one that bit for real: a machine already had an
//! `Ecphory` server configured, an installer-added sibling would have made two,
//! and the agent picked whichever it liked — non-deterministic memory and a
//! stamp that lands on only some traffic.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

fn install(home: &Path, args: &[&str]) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ecphory"));
    cmd.arg("install").args(args);
    cmd.env("HOME", home);
    cmd.env_remove("ECPHORY_DB");
    cmd.env_remove("CODEX_HOME");
    cmd.output().expect("run ecphory install")
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn claude_home(entries: &str) -> tempfile::TempDir {
    let home = tempfile::tempdir().expect("tempdir");
    fs::create_dir_all(home.path().join(".claude")).unwrap();
    fs::write(
        home.path().join(".claude.json"),
        format!(r#"{{"mcpServers":{{{entries}}}}}"#),
    )
    .unwrap();
    home
}

#[test]
fn plans_to_add_when_no_ecphory_server_is_configured() {
    let home = claude_home(r#""Other":{"type":"http","url":"http://127.0.0.1:9999/mcp"}"#);
    let out = install(home.path(), &[]);
    let s = stdout(&out);

    assert!(s.contains("claude"), "claude should be detected, got:\n{s}");
    assert!(
        s.to_uppercase().contains("ADD"),
        "should plan an add, got:\n{s}"
    );
}

#[test]
fn plans_to_stamp_an_existing_unstamped_server() {
    let home = claude_home(r#""Ecphory":{"type":"http","url":"http://127.0.0.1:3491/mcp"}"#);
    let out = install(home.path(), &[]);
    let s = stdout(&out);

    assert!(
        s.to_uppercase().contains("STAMP"),
        "an existing server should be stamped in place, not duplicated, got:\n{s}"
    );
    assert!(
        !s.to_uppercase().contains("ADD"),
        "must not plan to add a second ecphory server, got:\n{s}"
    );
}

#[test]
fn already_stamped_is_a_no_op() {
    let home = claude_home(
        r#""Ecphory":{"type":"http","url":"http://127.0.0.1:3491/mcp?client=claude-code"}"#,
    );
    let out = install(home.path(), &[]);
    let s = stdout(&out);

    assert!(
        s.to_lowercase().contains("no change") || s.to_uppercase().contains("OK"),
        "a correctly wired harness needs nothing done, got:\n{s}"
    );
}

/// Two ecphory servers means the agent chooses non-deterministically and the
/// stamp lands on only some traffic. Refuse rather than guess which is real.
#[test]
fn two_ecphory_servers_is_a_refusal_not_a_guess() {
    let home = claude_home(
        r#""Ecphory":{"type":"http","url":"http://127.0.0.1:3491/mcp"},"ecphory":{"type":"http","url":"http://127.0.0.1:3492/mcp"}"#,
    );
    let out = install(home.path(), &[]);
    let s = stdout(&out);

    assert!(
        s.to_uppercase().contains("COLLISION") || s.to_lowercase().contains("two"),
        "duplicate servers must be reported, got:\n{s}"
    );
    assert!(
        !s.to_uppercase().contains("STAMP "),
        "must not silently pick one to stamp, got:\n{s}"
    );
}

/// Dry-run is the default: planning must never touch the config.
#[test]
fn planning_writes_nothing() {
    let home = claude_home(r#""Ecphory":{"type":"http","url":"http://127.0.0.1:3491/mcp"}"#);
    let cfg = home.path().join(".claude.json");
    let before = fs::read(&cfg).unwrap();

    install(home.path(), &[]);

    assert_eq!(before, fs::read(&cfg).unwrap(), "dry-run must not write");
}

// ---------------------------------------------------------------------------
// --apply and uninstall.
//
// Edits are SURGICAL text replacements, never JSON/TOML round-trips: a
// round-trip reformats the whole file (opencode.json 3952 -> 4455 bytes,
// mcp-config.json 2779 -> 3563 in one measured case), which turns "add a query
// param" into an unreviewable diff of someone's hand-maintained config.
// ---------------------------------------------------------------------------

fn uninstall(home: &Path, args: &[&str]) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ecphory"));
    cmd.arg("uninstall").args(args);
    cmd.env("HOME", home);
    cmd.env_remove("ECPHORY_DB");
    cmd.env_remove("CODEX_HOME");
    cmd.output().expect("run ecphory uninstall")
}

/// A config with deliberate hand-formatting either side of the URL, so a
/// reformatting write is visible as a test failure rather than a nicety.
fn formatted_home() -> tempfile::TempDir {
    let home = tempfile::tempdir().expect("tempdir");
    fs::create_dir_all(home.path().join(".claude")).unwrap();
    fs::write(
        home.path().join(".claude.json"),
        "{\n  \"someOtherKey\": [1,2,3],\n  \"mcpServers\": {\n    \"Ecphory\": {\n      \"type\": \"http\",\n      \"url\": \"http://127.0.0.1:3491/mcp\"\n    }\n  }\n}\n",
    )
    .unwrap();
    home
}

#[test]
fn apply_edits_surgically_and_leaves_the_rest_byte_for_byte() {
    let home = formatted_home();
    let cfg = home.path().join(".claude.json");
    let before = fs::read_to_string(&cfg).unwrap();

    let out = install(home.path(), &["--apply"]);
    assert!(out.status.success(), "apply failed: {}", stdout(&out));

    let after = fs::read_to_string(&cfg).unwrap();
    assert!(
        after.contains("?client=claude-code"),
        "the stamp should be present, got:\n{after}"
    );
    // Everything except the URL must be untouched, formatting included.
    assert_eq!(
        before.replace("http://127.0.0.1:3491/mcp", "URL"),
        after.replace("http://127.0.0.1:3491/mcp?client=claude-code", "URL"),
        "only the URL should have changed"
    );
}

#[test]
fn apply_then_uninstall_is_byte_identical() {
    let home = formatted_home();
    let cfg = home.path().join(".claude.json");
    let before = fs::read(&cfg).unwrap();

    install(home.path(), &["--apply"]);
    assert_ne!(
        before,
        fs::read(&cfg).unwrap(),
        "apply should have changed it"
    );

    let out = uninstall(home.path(), &["--apply"]);
    assert!(out.status.success(), "uninstall failed: {}", stdout(&out));

    assert_eq!(
        before,
        fs::read(&cfg).unwrap(),
        "uninstall must restore the original bytes exactly"
    );
}

#[test]
fn apply_is_idempotent() {
    let home = formatted_home();
    let cfg = home.path().join(".claude.json");

    install(home.path(), &["--apply"]);
    let once = fs::read(&cfg).unwrap();
    install(home.path(), &["--apply"]);
    assert_eq!(
        once,
        fs::read(&cfg).unwrap(),
        "second apply must change nothing"
    );
}

/// If the URL text occurs more than once the edit target is ambiguous, and a
/// wrong guess silently repoints some other server at ecphory.
#[test]
fn apply_refuses_an_ambiguous_url() {
    let home = tempfile::tempdir().expect("tempdir");
    fs::create_dir_all(home.path().join(".claude")).unwrap();
    let cfg = home.path().join(".claude.json");
    fs::write(
        &cfg,
        r#"{"mcpServers":{"Ecphory":{"type":"http","url":"http://127.0.0.1:3491/mcp"},"Twin":{"type":"http","url":"http://127.0.0.1:3491/mcp"}}}"#,
    )
    .unwrap();
    let before = fs::read(&cfg).unwrap();

    let out = install(home.path(), &["--apply"]);
    let s = format!("{}{}", stdout(&out), String::from_utf8_lossy(&out.stderr));

    assert!(
        s.to_lowercase().contains("ambiguous") || s.to_lowercase().contains("more than once"),
        "should refuse an ambiguous edit, got:\n{s}"
    );
    assert_eq!(before, fs::read(&cfg).unwrap(), "nothing may be written");
}

#[test]
fn apply_leaves_a_backup() {
    let home = formatted_home();
    install(home.path(), &["--apply"]);

    let backups: Vec<_> = fs::read_dir(home.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .contains("claude.json.ecphory-bak")
        })
        .collect();
    assert!(!backups.is_empty(), "apply must leave a restorable backup");
}
