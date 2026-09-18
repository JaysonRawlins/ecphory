//! End-to-end purge: drives the compiled `ecphory` binary against a temp
//! store and a temp git-mirror export dir. Proves the two-phase contract at
//! the CLI boundary — refusal before demote, dry-run destroys nothing, and
//! execute removes the record, the index entries, and the mirror file (with
//! the removal committed, since the mirror is a git repo).

use std::path::Path;
use std::process::{Command, Output};

fn ecphory(db: &Path, export_dir: Option<&Path>, args: &[&str]) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ecphory"));
    cmd.arg("--db").arg(db).args(args);
    // Only purge reads ECPHORY_EXPORT_DIR; set it explicitly per call and
    // scrub any ambient value so the host environment can't leak in.
    match export_dir {
        Some(dir) => cmd.env("ECPHORY_EXPORT_DIR", dir),
        None => cmd.env_remove("ECPHORY_EXPORT_DIR"),
    };
    cmd.output().expect("run ecphory binary")
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

fn add(db: &Path, content: &str, name: &str) -> String {
    let out = ecphory(db, None, &["add", content, "--name", name]);
    assert!(out.status.success(), "add failed: {}", stderr(&out));
    let ep: serde_json::Value =
        serde_json::from_str(&stdout(&out)).expect("add prints episode JSON");
    ep["id"].as_str().expect("episode id").to_string()
}

#[test]
fn purge_end_to_end() {
    let store_dir = tempfile::tempdir().unwrap();
    let db = store_dir.path().join("e2e.redb");
    let mirror = tempfile::tempdir().unwrap();

    // Two episodes: one to purge, one that must survive untouched.
    let target_id = add(&db, "leaked secret capture about flamingos", "target");
    let bystander_id = add(&db, "unrelated bystander about walruses", "bystander");

    // Export to the mirror with a commit — the committed-mirror shape the
    // daemon's scheduled export produces, which purge must play along with.
    let mirror_arg = mirror.path().to_str().unwrap();
    let out = ecphory(&db, None, &["export", "--dir", mirror_arg, "--commit"]);
    assert!(out.status.success(), "export failed: {}", stderr(&out));
    let target_file = mirror
        .path()
        .join("default")
        .join(format!("{target_id}.md"));
    let bystander_file = mirror
        .path()
        .join("default")
        .join(format!("{bystander_id}.md"));
    assert!(target_file.exists());
    assert!(bystander_file.exists());

    // Not demoted yet: purge refuses, naming the id, destroying nothing.
    let out = ecphory(&db, Some(mirror.path()), &["purge", &target_id, "--yes"]);
    assert!(!out.status.success(), "purge of a live episode must fail");
    let err = stderr(&out);
    assert!(err.contains(&target_id), "refusal must name the id: {err}");
    assert!(err.contains("not demoted"), "refusal must say why: {err}");
    assert!(ecphory(&db, None, &["get", &target_id]).status.success());

    // Demote, then dry-run: manifest printed, nothing destroyed.
    let out = ecphory(&db, None, &["demote", &target_id]);
    assert!(out.status.success(), "demote failed: {}", stderr(&out));
    let out = ecphory(&db, Some(mirror.path()), &["purge", &target_id]);
    assert!(out.status.success(), "dry run failed: {}", stderr(&out));
    let text = stdout(&out);
    assert!(
        text.contains(&target_id),
        "manifest must show the id: {text}"
    );
    assert!(
        text.contains("target"),
        "manifest must show the name: {text}"
    );
    assert!(
        text.contains("dry run"),
        "default must be a dry run: {text}"
    );
    assert!(
        text.contains(target_file.to_str().unwrap()),
        "manifest must show the mirror path: {text}"
    );
    assert!(
        target_file.exists(),
        "dry run must not remove the mirror file"
    );
    assert!(
        ecphory(&db, None, &["get", &target_id]).status.success(),
        "dry run must not remove the episode"
    );

    // Execute: record, versions, index entries, and mirror file all go.
    let out = ecphory(&db, Some(mirror.path()), &["purge", &target_id, "--yes"]);
    assert!(out.status.success(), "purge --yes failed: {}", stderr(&out));
    let text = stdout(&out);
    assert!(
        text.contains("purged"),
        "must report what was destroyed: {text}"
    );
    assert!(!target_file.exists(), "mirror file must be gone");

    // Gone from get...
    assert!(!ecphory(&db, None, &["get", &target_id]).status.success());
    // ...and from search, even including deleted.
    let out = ecphory(&db, None, &["search", "flamingos", "--include-deleted"]);
    assert!(out.status.success(), "search failed: {}", stderr(&out));
    let json: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    assert_eq!(json["count"], 0, "purged episode still searchable: {json}");

    // The mirror removal was committed (the mirror was already a git repo).
    let log = Command::new("git")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .arg("-C")
        .arg(mirror.path())
        .args(["log", "--oneline", "--all"])
        .output()
        .unwrap();
    let log_text = String::from_utf8_lossy(&log.stdout).into_owned();
    assert!(
        log_text.contains("ecphory purge"),
        "purge commit missing from mirror: {log_text}"
    );

    // The bystander survives everything.
    assert!(ecphory(&db, None, &["get", &bystander_id]).status.success());
    assert!(
        bystander_file.exists(),
        "bystander mirror file must survive"
    );
}
