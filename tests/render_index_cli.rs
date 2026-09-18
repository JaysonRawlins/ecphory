//! End-to-end subject index: drives the compiled `ecphory` binary against a
//! real `ecphory serve` daemon and a real git repository with a linked
//! worktree.
//!
//! Two things only this level can prove. First, that the render works while
//! the daemon holds redb's process-exclusive lock — the reason it reads over
//! HTTP at all, and the failure mode a unit test cannot see. Second, that an
//! agent launched in a linked worktree and one launched in the main checkout
//! resolve to the same workspace, and so read the same index.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output};

fn ecphory(db: &Path, args: &[&str]) -> Output {
    cmd(db, args).output().expect("run ecphory binary")
}

fn cmd(db: &Path, args: &[&str]) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_ecphory"));
    c.arg("--db").arg(db).args(args);
    // Scrub the host's ecphory configuration: a real triggers file or export
    // dir would fire against the developer's own machine from a test run.
    for var in [
        "ECPHORY_TRIGGERS_FILE",
        "ECPHORY_EXPORT_DIR",
        "ECPHORY_HIDDEN_GROUPS",
        "ECPHORY_AUTH_TOKEN",
        // The renderer honours this, so leaving the developer's own value in
        // place would aim a test render at their live daemon.
        "ECPHORY_URL",
    ] {
        c.env_remove(var);
    }
    c
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// Hermetic: a fixture repo must not inherit the developer's git config.
/// `core.hooksPath` is global, so a commit-msg hook policing authorship
/// applies to this throwaway repo and fails the test for a reason that has
/// nothing to do with index rendering. Same for gpgsign and templates. CI
/// passes without this only because CI has none of them.
fn git(cwd: &Path, args: &[&str]) {
    let out = Command::new("git")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .output()
        .expect("run git");
    assert!(
        out.status.success(),
        "git {args:?} in {}: {}",
        cwd.display(),
        String::from_utf8_lossy(&out.stderr)
    );
}

/// Kills the daemon even when an assertion unwinds past it.
struct Daemon(Child);

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn free_port() -> u16 {
    let l = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    l.local_addr().unwrap().port()
}

fn serve(db: &Path) -> (Daemon, String) {
    let port = free_port();
    // Into the guard before anything can panic, so the never-healthy path
    // reaps the child instead of leaving a daemon holding the store's lock.
    let daemon = Daemon(
        cmd(db, &["serve", "--port", &port.to_string()])
            .spawn()
            .expect("spawn ecphory serve"),
    );
    let base = format!("http://127.0.0.1:{port}");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while std::time::Instant::now() < deadline {
        if ureq::get(format!("{base}/health")).call().is_ok() {
            return (daemon, base);
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    panic!("daemon never became healthy on {base}");
}

fn rows(rendered: &str) -> Vec<String> {
    rendered
        .lines()
        .filter(|l| l.starts_with("| ") && !l.starts_with("| ---") && !l.starts_with("| Query"))
        .map(str::to_string)
        .collect()
}

/// A git repository with a linked worktree, plus a subdirectory.
fn repo_with_worktree(root: &Path) -> (PathBuf, PathBuf) {
    let main = root.join("checkout");
    std::fs::create_dir_all(&main).unwrap();
    git(&main, &["init", "-q", "-b", "main"]);
    git(&main, &["config", "user.email", "t@example.com"]);
    git(&main, &["config", "user.name", "t"]);
    std::fs::write(main.join("README.md"), "hi\n").unwrap();
    git(&main, &["add", "-A"]);
    git(&main, &["commit", "-qm", "init"]);
    let wt = root.join("worktree");
    git(
        &main,
        &["worktree", "add", "-q", "-b", "side", wt.to_str().unwrap()],
    );
    (main, wt)
}

#[test]
fn subject_index_end_to_end() {
    let store_dir = tempfile::tempdir().unwrap();
    let db = store_dir.path().join("e2e.redb");
    let repo_dir = tempfile::tempdir().unwrap();
    let (main, worktree) = repo_with_worktree(repo_dir.path());
    let claude_cfg = tempfile::tempdir().unwrap();

    // The tag an episode carries to join this workspace's index.
    let out = ecphory(
        &db,
        &["workspace-key", "--workspace", main.to_str().unwrap()],
    );
    assert!(out.status.success(), "workspace-key: {}", stderr(&out));
    let tag = stdout(&out).trim().to_string();
    assert!(tag.starts_with("ws:-"), "unexpected tag {tag:?}");

    // A linked worktree is the SAME workspace: same tag, no second index.
    let from_worktree = ecphory(
        &db,
        &["workspace-key", "--workspace", worktree.to_str().unwrap()],
    );
    assert_eq!(
        stdout(&from_worktree).trim(),
        tag,
        "worktree resolved to a different workspace than its main checkout"
    );

    // Two episodes, written before the daemon takes the lock: one in this
    // workspace, one that must never appear in its index.
    let add = |content: &str, name: &str, phrase: &str, tags: &[&str]| {
        let mut args = vec!["add", content, "--name", name, "--phrase", phrase];
        for t in tags {
            args.push("--tag");
            args.push(t);
        }
        let out = ecphory(&db, &args);
        assert!(out.status.success(), "add: {}", stderr(&out));
    };
    add(
        "SENTINEL_BODY the daemon holds an exclusive redb lock",
        "the redb lock incident",
        "why does the CLI hang while the daemon is up",
        &[&tag, "gotcha"],
    );
    add(
        "OTHER_BODY unrelated project note",
        "someone else's project",
        "how do I do the other thing",
        &["ws:-somewhere-else"],
    );

    let (_daemon, base) = serve(&db);

    // --- render with the default targets -----------------------------------
    let out = cmd(
        &db,
        &[
            "render-index",
            "--workspace",
            main.to_str().unwrap(),
            "--url",
            &base,
        ],
    )
    .env("CLAUDE_CONFIG_DIR", claude_cfg.path())
    .output()
    .expect("run render-index");
    assert!(out.status.success(), "render-index: {}", stderr(&out));

    let agents = main.join("AGENTS.md");
    assert!(
        agents.exists(),
        "AGENTS.md was not written: {}",
        stdout(&out)
    );
    let memory_files: Vec<PathBuf> = walk(claude_cfg.path())
        .into_iter()
        .filter(|p| p.file_name().is_some_and(|n| n == "MEMORY.md"))
        .collect();
    assert_eq!(
        memory_files.len(),
        1,
        "expected exactly one Claude MEMORY.md target, got {memory_files:?}"
    );

    for path in [&agents, &memory_files[0]] {
        let rendered = std::fs::read_to_string(path).unwrap();
        let r = rows(&rendered);
        assert_eq!(
            r.len(),
            1,
            "{}: expected one row, got {r:?}",
            path.display()
        );
        assert!(r[0].contains("why does the CLI hang while the daemon is up"));
        assert!(
            !rendered.contains("SENTINEL_BODY") && !rendered.contains("OTHER_BODY"),
            "{}: episode body leaked:\n{rendered}",
            path.display()
        );
    }

    // The Claude target sits under the slug of the workspace, not the cwd.
    let slug = tag.strip_prefix("ws:").unwrap();
    assert_eq!(
        memory_files[0],
        claude_cfg
            .path()
            .join("projects")
            .join(slug)
            .join("memory")
            .join("MEMORY.md")
    );

    // --- re-render is a no-op ----------------------------------------------
    let before = std::fs::read_to_string(&agents).unwrap();
    let out = cmd(
        &db,
        &[
            "render-index",
            "--workspace",
            main.to_str().unwrap(),
            "--url",
            &base,
        ],
    )
    .env("CLAUDE_CONFIG_DIR", claude_cfg.path())
    .output()
    .unwrap();
    assert!(out.status.success());
    assert!(
        stdout(&out).contains("unchanged"),
        "second render reported a write: {}",
        stdout(&out)
    );
    assert_eq!(before, std::fs::read_to_string(&agents).unwrap());

    // --- an agent in the linked worktree renders the same index ------------
    let wt_target = repo_dir.path().join("from-worktree.md");
    let out = ecphory(
        &db,
        &[
            "render-index",
            "--workspace",
            worktree.to_str().unwrap(),
            "--url",
            &base,
            "--out",
            wt_target.to_str().unwrap(),
        ],
    );
    assert!(out.status.success(), "worktree render: {}", stderr(&out));
    assert_eq!(
        rows(&std::fs::read_to_string(&wt_target).unwrap()),
        rows(&before),
        "the worktree and the main checkout saw different indexes"
    );

    // --- --check goes red when a new episode lands -------------------------
    let out = ecphory(
        &db,
        &[
            "render-index",
            "--workspace",
            main.to_str().unwrap(),
            "--url",
            &base,
            "--out",
            agents.to_str().unwrap(),
            "--check",
        ],
    );
    assert!(out.status.success(), "--check was red on a fresh render");

    ureq::post(format!("{base}/api/v1/memory"))
        .send_json(serde_json::json!({
            "content": "LATER_BODY a second lesson",
            "name": "the second lesson",
            "search_phrases": ["what did we learn the second time"],
            "tags": [tag],
        }))
        .expect("POST new episode");

    let out = ecphory(
        &db,
        &[
            "render-index",
            "--workspace",
            main.to_str().unwrap(),
            "--url",
            &base,
            "--out",
            agents.to_str().unwrap(),
            "--check",
        ],
    );
    assert_eq!(
        out.status.code(),
        Some(1),
        "--check stayed green after a new episode: {}",
        stdout(&out)
    );
    assert!(stdout(&out).contains("STALE"), "{}", stdout(&out));
}

#[test]
fn hand_written_agents_file_survives_the_render() {
    let store_dir = tempfile::tempdir().unwrap();
    let db = store_dir.path().join("e2e.redb");
    let repo_dir = tempfile::tempdir().unwrap();
    let (main, _wt) = repo_with_worktree(repo_dir.path());

    let agents = main.join("AGENTS.md");
    let prologue = "# House rules\n\nRun `cargo test` before pushing.\n";
    std::fs::write(&agents, prologue).unwrap();

    let tag = stdout(&ecphory(
        &db,
        &["workspace-key", "--workspace", main.to_str().unwrap()],
    ))
    .trim()
    .to_string();
    let out = ecphory(
        &db,
        &[
            "add",
            "body",
            "--name",
            "a lesson",
            "--phrase",
            "how do I do the thing",
            "--tag",
            &tag,
        ],
    );
    assert!(out.status.success(), "add: {}", stderr(&out));

    let (_daemon, base) = serve(&db);
    let out = ecphory(
        &db,
        &[
            "render-index",
            "--workspace",
            main.to_str().unwrap(),
            "--url",
            &base,
            "--out",
            agents.to_str().unwrap(),
        ],
    );
    assert!(out.status.success(), "render-index: {}", stderr(&out));

    let rendered = std::fs::read_to_string(&agents).unwrap();
    assert!(
        rendered.starts_with(prologue),
        "hand-written prologue was clobbered:\n{rendered}"
    );
    assert!(rendered.contains("how do I do the thing"));
}

/// `--url` is the global flag, so `$ECPHORY_URL` reaches the renderer like it
/// reaches every other command that reads the daemon.
///
/// Red, observed: with a `--url` of its own on the subcommand, `render-index`
/// took the local default and went to loopback:3491 — the environment was
/// silently ignored, and on a developer's machine that is a live daemon
/// answering for a store the caller never named.
#[test]
fn the_daemon_url_can_come_from_the_environment() {
    let store_dir = tempfile::tempdir().unwrap();
    let db = store_dir.path().join("e2e.redb");
    let repo_dir = tempfile::tempdir().unwrap();
    let (main, _wt) = repo_with_worktree(repo_dir.path());

    let tag = stdout(&ecphory(
        &db,
        &["workspace-key", "--workspace", main.to_str().unwrap()],
    ))
    .trim()
    .to_string();
    let out = ecphory(
        &db,
        &[
            "add",
            "body",
            "--name",
            "a lesson",
            "--phrase",
            "reached through the environment",
            "--tag",
            &tag,
        ],
    );
    assert!(out.status.success(), "add: {}", stderr(&out));

    let (_daemon, base) = serve(&db);
    let agents = main.join("AGENTS.md");
    let out = cmd(
        &db,
        &[
            "render-index",
            "--workspace",
            main.to_str().unwrap(),
            "--out",
            agents.to_str().unwrap(),
        ],
    )
    .env("ECPHORY_URL", &base)
    .output()
    .expect("run ecphory binary");
    assert!(out.status.success(), "render-index: {}", stderr(&out));

    let rendered = std::fs::read_to_string(&agents).unwrap();
    assert_eq!(
        rows(&rendered).len(),
        1,
        "the render did not reach the daemon named by $ECPHORY_URL:\n{rendered}"
    );
    assert!(rendered.contains("reached through the environment"));
}

fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            out.extend(walk(&path));
        } else {
            out.push(path);
        }
    }
    out
}
