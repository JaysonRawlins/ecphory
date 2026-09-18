use std::path::Path;

use chrono::SecondsFormat;

use crate::error::{Error, Result};
use crate::model::Episode;

// Git-mirror export: one markdown file per episode, deterministic bytes,
// same frontmatter shape as engram's mirror (our importer already reads
// it) plus `search_phrases` — so a fresh import from an ecphory mirror
// round-trips the enrichment that the engram mirror loses.
//
// This is the durability layer: the live redb file is the hot tier, the
// git mirror is the cold tier. Deterministic rendering keeps diffs clean
// (unchanged episodes produce zero-byte diffs).

pub struct ExportOutcome {
    pub written: usize,
    pub unchanged: usize,
}

fn json_str(s: &str) -> String {
    serde_json::to_string(s).expect("string serializes")
}

fn time_field(t: chrono::DateTime<chrono::Utc>) -> String {
    json_str(&t.to_rfc3339_opts(SecondsFormat::Secs, true))
}

pub fn render_episode(ep: &Episode) -> String {
    let mut out = String::with_capacity(ep.content.len() + 512);
    out.push_str("---\n");
    out.push_str(&format!("id: {}\n", json_str(&ep.id.to_string())));
    if let Some(name) = &ep.name {
        out.push_str(&format!("name: {}\n", json_str(name)));
    }
    if !ep.search_phrases.is_empty() {
        out.push_str(&format!(
            "search_phrases: {}\n",
            serde_json::to_string(&ep.search_phrases).expect("phrases serialize")
        ));
    }
    out.push_str(&format!("source: {}\n", json_str(&ep.source)));
    if let Some(v) = &ep.source_model {
        out.push_str(&format!("source_model: {}\n", json_str(v)));
    }
    if let Some(v) = &ep.source_description {
        out.push_str(&format!("source_description: {}\n", json_str(v)));
    }
    out.push_str(&format!("group_id: {}\n", json_str(&ep.group_id)));
    if !ep.tags.is_empty() {
        out.push_str(&format!(
            "tags: {}\n",
            serde_json::to_string(&ep.tags).expect("tags serialize")
        ));
    }
    out.push_str(&format!("created_at: {}\n", time_field(ep.created_at)));
    if let Some(t) = ep.valid_at {
        out.push_str(&format!("valid_at: {}\n", time_field(t)));
    }
    if let Some(t) = ep.expired_at {
        out.push_str(&format!("expired_at: {}\n", time_field(t)));
    }
    if let Some(t) = ep.deleted_at {
        out.push_str(&format!("deleted_at: {}\n", time_field(t)));
    }
    if !ep.metadata.is_null() {
        // Engram-mirror convention: metadata is a JSON string field whose
        // value is the serialized JSON — our importer round-trips this.
        let serialized = serde_json::to_string(&ep.metadata).expect("metadata serializes");
        out.push_str(&format!("metadata: {}\n", json_str(&serialized)));
    }
    out.push_str("---\n\n");
    out.push_str(ep.content.trim_end());
    out.push('\n');
    out
}

/// Write the mirror tree. Only touches files whose content changed.
pub fn write_mirror(episodes: &[Episode], dir: &Path) -> Result<ExportOutcome> {
    std::fs::create_dir_all(dir)
        .map_err(|e| Error::Storage(format!("creating {}: {e}", dir.display())))?;

    let mut written = 0;
    let mut unchanged = 0;
    for ep in episodes {
        let group_dir = dir.join(&ep.group_id);
        std::fs::create_dir_all(&group_dir)
            .map_err(|e| Error::Storage(format!("creating {}: {e}", group_dir.display())))?;
        let path = group_dir.join(format!("{}.md", ep.id));
        let rendered = render_episode(ep);
        let current = std::fs::read_to_string(&path).ok();
        if current.as_deref() == Some(rendered.as_str()) {
            unchanged += 1;
            continue;
        }
        std::fs::write(&path, rendered)
            .map_err(|e| Error::Storage(format!("writing {}: {e}", path.display())))?;
        written += 1;
    }
    Ok(ExportOutcome { written, unchanged })
}

/// Commit the mirror. Initializes the repo on first use. "Nothing to
/// commit" counts as success (a prior commit already captured the state).
pub fn git_commit(dir: &Path, message: &str) -> Result<bool> {
    let git = |args: &[&str]| -> Result<std::process::Output> {
        std::process::Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .output()
            .map_err(|e| Error::Storage(format!("git {:?}: {e}", args.first())))
    };

    if !dir.join(".git").exists() {
        let init = git(&["init", "-q"])?;
        if !init.status.success() {
            return Err(Error::Storage(format!(
                "git init failed: {}",
                String::from_utf8_lossy(&init.stderr)
            )));
        }
    }

    let add = git(&["add", "-A"])?;
    if !add.status.success() {
        return Err(Error::Storage(format!(
            "git add failed: {}",
            String::from_utf8_lossy(&add.stderr)
        )));
    }

    let staged = git(&["diff", "--cached", "--quiet"])?;
    if staged.status.success() {
        return Ok(false); // nothing to commit — mirror already current
    }

    // Fixed identity: exports are machine-generated, and the daemon must
    // be able to commit on hosts with no global git config (CI, containers).
    //
    // --no-verify for the same reason one level up: a mirror commit is a
    // machine snapshot, not an authored change, and the operator's hooks
    // are written to police authorship. A global `core.hooksPath` applies
    // to every repo on the box including this one, so a commit-msg hook
    // that demands a trailer — or a pre-commit that lints a source tree
    // this mirror is not — silently turns the backup path into an error.
    // The push keeps its hooks: the commit is local bookkeeping, the push
    // is what leaves the machine and is where a guard still earns its
    // place.
    let commit = git(&[
        "-c",
        "user.name=ecphory",
        "-c",
        "user.email=ecphory@localhost",
        "commit",
        "--no-verify",
        "-q",
        "-m",
        message,
    ])?;
    if !commit.status.success() {
        return Err(Error::Storage(format!(
            "git commit failed: {}",
            String::from_utf8_lossy(&commit.stderr)
        )));
    }
    Ok(true)
}

/// Push the mirror to its first configured remote, if any. No remote →
/// Ok(false): the mirror stays a local cold tier until the operator adds
/// one, and adding one IS the opt-in (`git remote add` is the whole setup).
/// `HEAD` so it works whatever the local branch is named, with or without
/// an upstream configured.
pub fn git_push(dir: &Path) -> Result<bool> {
    let git = |args: &[&str]| -> Result<std::process::Output> {
        std::process::Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .output()
            .map_err(|e| Error::Storage(format!("git {:?}: {e}", args.first())))
    };

    let remotes = git(&["remote"])?;
    if !remotes.status.success() {
        return Err(Error::Storage(format!(
            "git remote failed: {}",
            String::from_utf8_lossy(&remotes.stderr)
        )));
    }
    let stdout = String::from_utf8_lossy(&remotes.stdout);
    let Some(remote) = stdout.lines().map(str::trim).find(|s| !s.is_empty()) else {
        return Ok(false);
    };

    let push = git(&["push", "-q", remote, "HEAD"])?;
    if !push.status.success() {
        return Err(Error::Storage(format!(
            "git push to {remote} failed: {}",
            String::from_utf8_lossy(&push.stderr)
        )));
    }
    Ok(true)
}

/// ECPHORY_EXPORT_PUSH gates the automatic post-export push (default on —
/// a remote on the mirror already expresses the operator's intent).
pub fn push_enabled() -> bool {
    !matches!(
        std::env::var("ECPHORY_EXPORT_PUSH").as_deref(),
        Ok("false") | Ok("0") | Ok("off")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import::read_mirror;

    fn sample() -> Episode {
        let mut ep = Episode::new("body line one\n\nbody line two", "test");
        ep.name = Some("round trip".into());
        ep.search_phrases = vec![
            "how do phrases survive export".into(),
            "mirror keeps enrichment".into(),
        ];
        ep.tags = vec!["a".into(), "b".into()];
        ep.metadata = serde_json::json!({"k": "v"});
        ep
    }

    #[test]
    fn render_is_deterministic() {
        let ep = sample();
        assert_eq!(render_episode(&ep), render_episode(&ep));
    }

    #[test]
    fn export_import_round_trips_phrases() {
        let dir = tempfile::tempdir().unwrap();
        let ep = sample();
        let out = write_mirror(std::slice::from_ref(&ep), dir.path()).unwrap();
        assert_eq!(out.written, 1);

        let read = read_mirror(dir.path()).unwrap();
        assert!(read.skipped.is_empty());
        let got = &read.episodes[0];
        assert_eq!(got.id, ep.id);
        assert_eq!(got.search_phrases, ep.search_phrases);
        assert_eq!(got.content, ep.content);
        assert_eq!(got.tags, ep.tags);
        assert_eq!(got.metadata, ep.metadata);

        // Idempotent second export: no rewrites.
        let again = write_mirror(std::slice::from_ref(&ep), dir.path()).unwrap();
        assert_eq!(again.written, 0);
        assert_eq!(again.unchanged, 1);
    }

    #[test]
    fn git_commit_lifecycle() {
        let dir = tempfile::tempdir().unwrap();
        let ep = sample();
        write_mirror(std::slice::from_ref(&ep), dir.path()).unwrap();
        assert!(git_commit(dir.path(), "first").unwrap());
        assert!(!git_commit(dir.path(), "second").unwrap()); // nothing new
    }

    #[test]
    fn git_push_lifecycle() {
        let root = tempfile::tempdir().unwrap();
        let mirror = root.path().join("mirror");
        let bare = root.path().join("offsite.git");
        std::fs::create_dir_all(&mirror).unwrap();

        let ep = sample();
        write_mirror(std::slice::from_ref(&ep), &mirror).unwrap();
        assert!(git_commit(&mirror, "first").unwrap());

        // No remote yet: push is a clean no-op, not an error.
        assert!(!git_push(&mirror).unwrap());

        // Hermetic: fixtures must not inherit the developer's git config
        // (hooks, gpgsign, templates). CI passes without this only because
        // CI has none of them.
        let sh = |dir: &Path, args: &[&str]| {
            let out = std::process::Command::new("git")
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_CONFIG_SYSTEM", "/dev/null")
                .arg("-C")
                .arg(dir)
                .args(args)
                .output()
                .unwrap();
            assert!(out.status.success(), "git {args:?}: {out:?}");
            out
        };
        sh(root.path(), &["init", "-q", "--bare", "offsite.git"]);
        sh(
            &mirror,
            &["remote", "add", "origin", bare.to_str().unwrap()],
        );

        assert!(git_push(&mirror).unwrap());
        // --all, not HEAD: the bare repo's default branch and the mirror's
        // need not agree once neither is reading a developer's
        // init.defaultBranch, and what is under test is that the commit
        // arrived at all.
        let log = sh(&bare, &["log", "--oneline", "--all"]);
        assert!(String::from_utf8_lossy(&log.stdout).contains("first"));
    }

    #[test]
    fn push_enabled_env_gate() {
        // Only asserts the default path (env unset in the test runner):
        // mutating the process env here would race parallel tests.
        assert!(push_enabled());
    }
}
