//! `scripts/factory/cargo-test-counts.sh`, the pre-pr-verify step that runs
//! the suite and prints its totals.
//!
//! `cargo test` exiting 0 does not show that any test ran. A filter that
//! matches nothing, or a target that was dropped, still exits 0. So the step
//! sums cargo's own `test result:` lines and fails on zero tests as well as on
//! any failure. Those totals are also what the factory's open-pr stage puts in
//! the PR body. Each case below builds a tiny dependency-free crate.
//!
//! Red-proofs, 2026-10-19. Against a bare `cargo test --all-targets` stub,
//! the zero-test case passed (cargo exited 0) and both totals assertions
//! failed. In the real script, removing the zero-test check fails
//! `a_suite_with_no_tests_fails`, and removing the cargo exit-code check fails
//! `a_failing_test_fails_and_is_counted`. A separate failure-count check was
//! cut: cargo's exit code already covers it, so no mutation could fail it.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

fn run_on_crate(lib_rs: &str) -> (Output, String) {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"fixture\"\nversion = \"0.0.0\"\nedition = \"2021\"\n\n[workspace]\n",
    )
    .unwrap();
    fs::write(root.join("src/lib.rs"), lib_rs).unwrap();
    let script =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scripts/factory/cargo-test-counts.sh");
    let out = Command::new("bash")
        .arg(&script)
        .current_dir(root)
        .env("CARGO_TARGET_DIR", root.join("target"))
        .output()
        .expect("run cargo-test-counts.sh");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let totals = stdout
        .lines()
        .rev()
        .find(|l| l.starts_with("{\"tests\""))
        .unwrap_or("")
        .to_string();
    (out, totals)
}

#[test]
fn a_passing_suite_reports_its_totals() {
    let (out, totals) =
        run_on_crate("#[test] fn a() {}\n#[test] fn b() {}\n#[test] #[ignore] fn c() {}\n");
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(totals, r#"{"tests":2,"failures":0,"ignored":1}"#);
}

#[test]
fn a_suite_with_no_tests_fails() {
    let (out, totals) = run_on_crate("pub fn nothing() {}\n");
    assert!(
        !out.status.success(),
        "zero tests must not pass. totals: {totals}"
    );
    assert_eq!(totals, r#"{"tests":0,"failures":0,"ignored":0}"#);
}

#[test]
fn a_failing_test_fails_and_is_counted() {
    let (out, totals) = run_on_crate("#[test] fn ok() {}\n#[test] fn bad() { panic!(\"no\") }\n");
    assert!(
        !out.status.success(),
        "a failing suite must not pass. totals: {totals}"
    );
    assert_eq!(totals, r#"{"tests":1,"failures":1,"ignored":0}"#);
}

#[test]
fn a_compile_error_fails() {
    let (out, totals) = run_on_crate("#[test] fn broken() { let x: u8 = \"s\"; }\n");
    assert!(
        !out.status.success(),
        "a crate that does not compile must not pass. totals: {totals}"
    );
}
