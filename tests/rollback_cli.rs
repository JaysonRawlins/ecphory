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

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

#[test]
fn rollback_end_to_end() {
    let store_dir = tempfile::tempdir().unwrap();
    let db = store_dir.path().join("rollback.redb");

    let added = ecphory(&db, &["add", "original rollback content"]);
    assert!(added.status.success(), "add failed: {}", stderr(&added));
    let original: serde_json::Value = serde_json::from_str(&stdout(&added)).unwrap();
    let id = original["id"].as_str().unwrap();

    let updated = ecphory(
        &db,
        &[
            "update",
            id,
            "--content",
            "revised rollback content",
            "--name",
            "revised name",
            "--phrase",
            "revised cue",
            "--tag",
            "revised",
        ],
    );
    assert!(
        updated.status.success(),
        "update failed: {}",
        stderr(&updated)
    );

    let versions = ecphory(&db, &["versions", id]);
    assert!(
        versions.status.success(),
        "versions failed: {}",
        stderr(&versions)
    );
    let versions: serde_json::Value = serde_json::from_str(&stdout(&versions)).unwrap();
    let version_id = versions[0]["version_id"].as_str().unwrap();

    let rolled_back = ecphory(&db, &["rollback", id, version_id]);
    assert!(
        rolled_back.status.success(),
        "rollback failed: {}",
        stderr(&rolled_back)
    );

    let got = ecphory(&db, &["get", id]);
    assert!(got.status.success(), "get failed: {}", stderr(&got));
    let restored: serde_json::Value = serde_json::from_str(&stdout(&got)).unwrap();
    assert_eq!(restored["content"], "original rollback content");
    assert!(restored.get("name").is_none());
    assert!(restored.get("search_phrases").is_none());
    assert!(restored.get("tags").is_none());

    let versions = ecphory(&db, &["versions", id]);
    let versions: serde_json::Value = serde_json::from_str(&stdout(&versions)).unwrap();
    assert_eq!(versions.as_array().unwrap().len(), 2);
    assert_eq!(versions[1]["operation"], "rollback");
    assert_eq!(
        versions[1]["episode"]["content"],
        "revised rollback content"
    );
}
