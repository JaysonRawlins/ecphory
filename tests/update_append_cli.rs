//! `ecphory update --append` adds to the stored content rather than replacing
//! it — the CLI's half of the fix for ecphory 01a14120, where an agent's
//! guessed `mode: "append"` was silently dropped and the update replaced a
//! 5,500-word episode. The MCP and REST halves are pinned in-crate.
use std::path::Path;
use std::process::{Command, Output};

fn ecphory(db: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ecphory"))
        .arg("--db")
        .arg(db)
        .args(args)
        .output()
        .expect("run ecphory binary")
}

fn json(out: &Output) -> serde_json::Value {
    assert!(
        out.status.success(),
        "ecphory failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}

#[test]
fn append_keeps_the_original_and_requires_content() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("append.redb");

    let added = json(&ecphory(&db, &["add", "the long original body"]));
    let id = added["id"].as_str().unwrap();

    let updated = json(&ecphory(
        &db,
        &["update", id, "--append", "--content", "OUTCOMES: shipped"],
    ));
    assert_eq!(
        updated["content"],
        "the long original body\n\nOUTCOMES: shipped"
    );

    // --append with nothing to append is refused. Two layers refuse it (clap's
    // `requires` and the store's guard), so removing either one alone stays green.
    let bare = ecphory(&db, &["update", id, "--append", "--name", "n"]);
    assert!(
        !bare.status.success(),
        "--append without --content must fail"
    );

    // Without the flag, --content still replaces.
    let replaced = json(&ecphory(&db, &["update", id, "--content", "rewritten"]));
    assert_eq!(replaced["content"], "rewritten");
}
