//! Subject index: render ecphory recall keys into each harness's native
//! instruction file.
//!
//! ecphory is the shared store, but only Claude Code finds its way in
//! unaided: it injects a per-project `MEMORY.md` as a first-class instruction
//! source, and that slim index of search keys is what makes recall work — an
//! agent holding the index knows *what to search for*. Codex, agy and
//! opencode read `AGENTS.md`, get the same MCP access and none of the index,
//! so on the same repo they start materially behind.
//!
//! This module renders that index. It is keys only — query phrase, tags, a
//! one-line description, episode id — never episode bodies: the file is a
//! pointer, the store holds the content. Output goes inside a marked region
//! so a hand-written instruction file survives regeneration, and an unchanged
//! render produces byte-identical output (and therefore no diff).

use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::model::Episode;

pub const BEGIN_MARKER: &str = "<!-- BEGIN ecphory subject index -->";
pub const END_MARKER: &str = "<!-- END ecphory subject index -->";

/// Prefix of the tag that joins an episode to a workspace's index.
pub const WS_TAG_PREFIX: &str = "ws:";

/// The workspace an index is scoped to: the canonical working-copy root of
/// `start`, or `start` itself when it is not in a repo.
///
/// Deliberately the same rule Claude Code uses to key its per-project memory
/// directory, so the two agree on what "this project" means rather than
/// drifting. `--git-common-dir` is the load-bearing part: in a linked
/// worktree it points at the *main* checkout's `.git`, so a worktree agent
/// and a main-checkout agent resolve to one workspace and share one index.
pub fn workspace_root(start: &Path) -> PathBuf {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(start)
        .args(["rev-parse", "--path-format=absolute", "--git-common-dir"])
        .output();
    if let Ok(out) = out
        && out.status.success()
        && let Ok(text) = String::from_utf8(out.stdout)
    {
        let common = PathBuf::from(text.trim());
        // Only the ordinary `<root>/.git` layout tells us the working-copy
        // root. A submodule's common dir is `<super>/.git/modules/<name>`,
        // and a bare repo has no working copy at all — both fall through to
        // the toplevel below rather than being guessed at.
        if common.file_name().is_some_and(|n| n == ".git")
            && let Some(root) = common.parent()
            && root.is_dir()
        {
            return root.to_path_buf();
        }
    }
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(start)
        .args(["rev-parse", "--path-format=absolute", "--show-toplevel"])
        .output();
    if let Ok(out) = out
        && out.status.success()
        && let Ok(text) = String::from_utf8(out.stdout)
    {
        let top = text.trim();
        if !top.is_empty() {
            return PathBuf::from(top);
        }
    }
    // No repo: the working directory is its own workspace.
    start.to_path_buf()
}

/// Claude Code's project-directory slug, reimplemented byte for byte from
/// `sanitizePath` in the 2.1.263 bundle, so the tag, the memory path and
/// Claude's own `~/.claude/projects/<slug>` all agree:
///
/// ```js
/// function RA(e){let n=e.replace(/[^a-zA-Z0-9]/g,"-");if(n.length<=200)return n;
///   return `${n.slice(0,200)}-${Math.abs(gz(e)).toString(36)}`}
/// function gz(t){let e=0;for(let r=0;r<t.length;r++)e=(e<<5)-e+t.charCodeAt(r)|0;return e}
/// ```
///
/// Two details that a plain character map gets wrong. The replace runs over
/// UTF-16 code units, so a non-BMP character becomes *two* dashes. And a slug
/// past 200 characters is truncated and suffixed with a base36 hash of the
/// ORIGINAL path — get that wrong and a long workspace path renders into a
/// directory Claude never reads, with nothing to notice it by.
///
/// The mapping is lossy on purpose (it is a directory name, not an identity):
/// `/a/b-c` and `/a/b/c` slug the same. Nothing here reverses it — the
/// workspace path is always carried explicitly.
///
/// Known gap: Claude NFC-normalizes the path first. We do not (no unicode
/// dependency for a path that is ASCII in every real case); a decomposed
/// non-ASCII path would slug to a different number of dashes.
pub fn workspace_slug(path: &Path) -> String {
    const MAX: usize = 200;
    let raw = path.to_string_lossy();
    let sanitized: String = raw
        .encode_utf16()
        .map(|u| match u {
            0x41..=0x5A | 0x61..=0x7A | 0x30..=0x39 => char::from_u32(u32::from(u)).expect("ascii"),
            _ => '-',
        })
        .collect();
    if sanitized.len() <= MAX {
        return sanitized;
    }
    format!(
        "{}-{}",
        &sanitized[..MAX],
        base36(js_hash(&raw).unsigned_abs().into())
    )
}

/// `h = h * 31 + code_unit`, wrapping at 32 signed bits — JS's `(e<<5)-e+c|0`.
fn js_hash(s: &str) -> i32 {
    let mut h: i32 = 0;
    for unit in s.encode_utf16() {
        h = h
            .wrapping_shl(5)
            .wrapping_sub(h)
            .wrapping_add(i32::from(unit));
    }
    h
}

/// `Number.prototype.toString(36)`: lowercase, no padding.
fn base36(mut n: u64) -> String {
    if n == 0 {
        return "0".to_string();
    }
    const DIGITS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut out = Vec::new();
    while n > 0 {
        out.push(DIGITS[(n % 36) as usize]);
        n /= 36;
    }
    out.reverse();
    String::from_utf8(out).expect("ascii digits")
}

/// The tag an episode carries to join a workspace's index.
pub fn workspace_tag(workspace: &Path) -> String {
    format!("{WS_TAG_PREFIX}{}", workspace_slug(workspace))
}

/// Claude Code's per-project memory file for a workspace.
/// `CLAUDE_CONFIG_DIR` overrides `~/.claude`, matching Claude's own lookup.
pub fn claude_memory_path(workspace: &Path) -> Result<PathBuf> {
    let base = match std::env::var("CLAUDE_CONFIG_DIR") {
        Ok(dir) if !dir.trim().is_empty() => PathBuf::from(dir),
        _ => {
            let home = std::env::var("HOME")
                .map_err(|_| Error::Storage("HOME is unset; pass an explicit path".into()))?;
            PathBuf::from(home).join(".claude")
        }
    };
    Ok(base
        .join("projects")
        .join(workspace_slug(workspace))
        .join("memory")
        .join("MEMORY.md"))
}

/// One recall key. Never carries episode content — that is the invariant the
/// whole feature rests on.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    /// Plain-words phrasing that retrieves the episode.
    pub query: String,
    pub tags: Vec<String>,
    pub description: String,
    pub id: String,
}

/// Build a row from an episode, or `None` when it carries nothing to key on.
///
/// `search_phrases` are the point: they are written as "how someone would ask
/// for this", which is exactly what an index row needs. The name is the
/// description. An episode with neither has no key material and is skipped —
/// a row with no query is a pointer to nowhere.
pub fn row(ep: &Episode, ws_tag: &str) -> Option<Row> {
    let phrase = ep
        .search_phrases
        .iter()
        .map(|p| p.trim())
        .find(|p| !p.is_empty());
    let name = ep.name.as_deref().map(str::trim).filter(|n| !n.is_empty());
    let (query, description) = match (phrase, name) {
        (Some(p), n) => (p, n.unwrap_or("")),
        (None, Some(n)) => (n, ""),
        (None, None) => return None,
    };
    Some(Row {
        query: cell(query),
        // The workspace tag is identical on every row of a given file, so it
        // is pure noise there.
        tags: ep
            .tags
            .iter()
            .filter(|t| t.as_str() != ws_tag)
            .map(|t| cell(t))
            .collect(),
        description: cell(description),
        id: ep.id.to_string(),
    })
}

/// Make a string safe for one markdown table cell: single line, pipes
/// escaped, whitespace runs collapsed. Deterministic for a given input.
fn cell(s: &str) -> String {
    let flattened = s.split_whitespace().collect::<Vec<_>>().join(" ");
    flattened.replace('|', "\\|")
}

/// Render the marked region. Deterministic: no timestamps, no counters, no
/// anything that changes when the episodes do not.
pub fn render_block(rows: &[Row], ws_tag: &str) -> String {
    let mut out = String::with_capacity(256 + rows.len() * 160);
    out.push_str(BEGIN_MARKER);
    out.push('\n');
    out.push_str(&format!(
        "<!-- GENERATED by `ecphory render-index` — DO NOT HAND-EDIT.\n     \
         Recall keys for `{ws_tag}`. Keys only: the content lives in ecphory.\n     \
         Search the Query text (MCP `search`, or `ecphory search \"<query>\"`),\n     \
         then read the episode by its id. -->\n\n"
    ));
    if rows.is_empty() {
        out.push_str(&format!(
            "No episodes are tagged `{ws_tag}` yet. Tag one to have it indexed here.\n"
        ));
    } else {
        out.push_str("| Query | Tags | Description | Episode |\n");
        out.push_str("| --- | --- | --- | --- |\n");
        for r in rows {
            out.push_str(&format!(
                "| {} | {} | {} | {} |\n",
                r.query,
                r.tags.join(", "),
                r.description,
                r.id
            ));
        }
    }
    out.push_str(END_MARKER);
    out.push('\n');
    out
}

/// Put `block` into `existing`, replacing an existing marked region and
/// leaving every other byte alone. Absent markers append the region.
///
/// A half-open region (one marker, or END before BEGIN) is an error, not a
/// guess: the file has been hand-edited into a shape where any choice we make
/// risks eating someone's text.
pub fn splice(existing: &str, block: &str) -> Result<String> {
    let begin = existing.find(BEGIN_MARKER);
    let end = existing.find(END_MARKER);
    match (begin, end) {
        (Some(b), Some(e)) => {
            if e < b {
                return Err(Error::Storage(
                    "subject index markers are out of order (END before BEGIN)".into(),
                ));
            }
            let tail = e + END_MARKER.len();
            let mut out = String::with_capacity(existing.len() + block.len());
            out.push_str(&existing[..b]);
            out.push_str(block.trim_end_matches('\n'));
            out.push_str(&existing[tail..]);
            if !out.ends_with('\n') {
                out.push('\n');
            }
            Ok(out)
        }
        (None, None) => {
            let mut out = String::with_capacity(existing.len() + block.len() + 2);
            if !existing.trim().is_empty() {
                out.push_str(existing);
                if !out.ends_with('\n') {
                    out.push('\n');
                }
                out.push('\n');
            }
            out.push_str(block);
            Ok(out)
        }
        _ => Err(Error::Storage(
            "subject index region is half open: exactly one marker found".into(),
        )),
    }
}

/// Write only when the bytes change, so a no-op render leaves no diff and no
/// mtime churn. Written to a temp file in the same directory and renamed, so
/// a reader never sees a half-written instruction file.
///
/// A symlinked target is followed, not replaced: the rename would otherwise
/// silently swap someone's deliberate link for a regular file (the reference
/// deployment symlinks each harness's global instruction file at one place).
pub fn write_if_changed(path: &Path, contents: &str) -> Result<bool> {
    if std::fs::read_to_string(path).ok().as_deref() == Some(contents) {
        return Ok(false);
    }
    let target = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| Error::Storage(format!("creating {}: {e}", parent.display())))?;
    }
    let mut name = target.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".ecphory-tmp-{}", std::process::id()));
    let tmp = target.with_file_name(name);
    std::fs::write(&tmp, contents)
        .map_err(|e| Error::Storage(format!("writing {}: {e}", tmp.display())))?;
    std::fs::rename(&tmp, &target).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        Error::Storage(format!("renaming into {}: {e}", target.display()))
    })?;
    Ok(true)
}

/// Rows for one workspace, newest first (UUIDv7 ids sort by creation time).
pub fn rows_for(episodes: &[Episode], ws_tag: &str) -> Vec<Row> {
    let mut scoped: Vec<&Episode> = episodes
        .iter()
        .filter(|ep| !ep.is_deleted() && ep.tags.iter().any(|t| t == ws_tag))
        .collect();
    scoped.sort_by_key(|ep| std::cmp::Reverse(ep.id));
    scoped.iter().filter_map(|ep| row(ep, ws_tag)).collect()
}

/// The bytes a render would produce for one file, without writing.
pub fn rendered_file(episodes: &[Episode], ws_tag: &str, path: &Path) -> Result<String> {
    let rows = rows_for(episodes, ws_tag);
    let block = render_block(&rows, ws_tag);
    let existing = std::fs::read_to_string(path).unwrap_or_default();
    splice(&existing, &block)
}

/// Render one workspace's index into one file. Returns whether it changed.
pub fn render_into(episodes: &[Episode], ws_tag: &str, path: &Path) -> Result<bool> {
    let out = rendered_file(episodes, ws_tag, path)?;
    write_if_changed(path, &out)
}

/// Would a render change this file? Writes nothing.
pub fn is_stale(episodes: &[Episode], ws_tag: &str, path: &Path) -> Result<bool> {
    let out = rendered_file(episodes, ws_tag, path)?;
    Ok(std::fs::read_to_string(path).ok().as_deref() != Some(out.as_str()))
}

/// Where a workspace's index goes when the operator names no targets:
/// the repo-root `AGENTS.md` that codex, agy and opencode read, and the
/// per-project `MEMORY.md` Claude Code injects.
pub fn default_targets(workspace: &Path) -> Result<Vec<PathBuf>> {
    Ok(vec![
        workspace.join("AGENTS.md"),
        claude_memory_path(workspace)?,
    ])
}

/// A listing large enough that no real subject index reaches it. Explicit
/// because `max_results=0` means "the default 10" on the wire, and a
/// silently truncated index is worse than a loud one.
const MAX_INDEX_ROWS: usize = 10_000;

#[derive(serde::Deserialize)]
struct ListResponse {
    episodes: Vec<Episode>,
}

/// Pull one workspace's episodes from the live daemon.
///
/// HTTP, not the store: a render fired by a post-write trigger runs while the
/// daemon holds redb's process-exclusive lock, so opening the database
/// directly would fail every time — the same reason `eval` is HTTP-only.
pub fn fetch_scoped(base: &str, token: Option<&str>, ws_tag: &str) -> Result<Vec<Episode>> {
    let url = format!(
        "{}/api/v1/memory/episodes?max_results={MAX_INDEX_ROWS}&tags={}",
        base.trim_end_matches('/'),
        crate::eval::url_encode(ws_tag)
    );
    let mut req = ureq::get(&url);
    if let Some(token) = token {
        req = req.header("Authorization", &format!("Bearer {token}"));
    }
    let mut resp = req
        .call()
        .map_err(|e| Error::Storage(format!("GET {url}: {e}")))?;
    let body: ListResponse = resp
        .body_mut()
        .read_json()
        .map_err(|e| Error::Storage(format!("decoding {url}: {e}")))?;
    if body.episodes.len() >= MAX_INDEX_ROWS {
        return Err(Error::Storage(format!(
            "{} episodes carry {ws_tag} — at or past the {MAX_INDEX_ROWS} listing cap, \
             so the index would be silently truncated",
            body.episodes.len()
        )));
    }
    Ok(body.episodes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ep(name: &str, phrases: &[&str], tags: &[&str], content: &str) -> Episode {
        let mut e = Episode::new(content, "test");
        e.name = Some(name.to_string());
        e.search_phrases = phrases.iter().map(|s| s.to_string()).collect();
        e.tags = tags.iter().map(|s| s.to_string()).collect();
        e
    }

    /// The issue's three assertions, in one place, on one rendered file.
    #[test]
    fn workspace_index_is_one_row_per_episode_keys_only_and_stable() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("AGENTS.md");
        let ws_x = "ws:-work-x";
        let ws_y = "ws:-work-y";

        let mine = ep(
            "the redb lock incident",
            &["why does the CLI hang when the daemon is up"],
            &[ws_x, "gotcha"],
            "SENTINEL_BODY_TEXT redb takes an exclusive file lock",
        );
        let theirs = ep(
            "someone else's project",
            &["unrelated"],
            &[ws_y],
            "OTHER_BODY_TEXT",
        );
        let episodes = vec![mine, theirs];

        // 1. exactly one row.
        assert!(render_into(&episodes, ws_x, &target).unwrap());
        let rendered = std::fs::read_to_string(&target).unwrap();
        let rows: Vec<&str> = rendered
            .lines()
            .filter(|l| l.starts_with("| ") && !l.starts_with("| ---") && !l.starts_with("| Query"))
            .collect();
        assert_eq!(rows.len(), 1, "expected one row, got: {rows:?}");
        assert!(rows[0].contains("why does the CLI hang"));

        // 2. no episode body text anywhere in the file.
        assert!(
            !rendered.contains("SENTINEL_BODY_TEXT"),
            "episode body leaked into the rendered index:\n{rendered}"
        );
        assert!(!rendered.contains("OTHER_BODY_TEXT"));

        // 3. re-render with no new episodes is byte-identical.
        let changed = render_into(&episodes, ws_x, &target).unwrap();
        assert!(!changed, "second render reported a change");
        assert_eq!(
            rendered,
            std::fs::read_to_string(&target).unwrap(),
            "second render produced different bytes"
        );
    }

    #[test]
    fn hand_written_text_survives_regeneration() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("AGENTS.md");
        std::fs::write(&target, "# Project rules\n\nRun the tests.\n").unwrap();

        let ws = "ws:-work-x";
        render_into(&[ep("a", &["q a"], &[ws], "body")], ws, &target).unwrap();
        let first = std::fs::read_to_string(&target).unwrap();
        assert!(first.starts_with("# Project rules\n\nRun the tests.\n"));

        // A second, different episode set rewrites only the region.
        render_into(&[ep("b", &["q b"], &[ws], "body")], ws, &target).unwrap();
        let second = std::fs::read_to_string(&target).unwrap();
        assert!(second.starts_with("# Project rules\n\nRun the tests.\n"));
        assert!(second.contains("q b"));
        assert!(!second.contains("q a"));
        assert_eq!(second.matches(BEGIN_MARKER).count(), 1);
    }

    #[test]
    fn a_symlinked_target_is_followed_not_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("shared-instructions.md");
        std::fs::write(&real, "").unwrap();
        let link = dir.path().join("AGENTS.md");
        std::os::unix::fs::symlink(&real, &link).unwrap();

        let ws = "ws:-work-x";
        render_into(&[ep("a", &["q a"], &[ws], "body")], ws, &link).unwrap();

        assert!(
            std::fs::symlink_metadata(&link).unwrap().is_symlink(),
            "the render replaced a symlinked target with a regular file"
        );
        assert!(std::fs::read_to_string(&real).unwrap().contains("q a"));
    }

    #[test]
    fn half_open_region_is_an_error() {
        let err = splice("prose\n<!-- BEGIN ecphory subject index -->\n", "block\n").unwrap_err();
        assert!(err.to_string().contains("half open"), "{err}");
        let err = splice(
            "<!-- END ecphory subject index -->\n<!-- BEGIN ecphory subject index -->\n",
            "block\n",
        )
        .unwrap_err();
        assert!(err.to_string().contains("out of order"), "{err}");
    }

    /// Vectors produced by running Claude's own `sanitizePath` (transcribed
    /// from the 2.1.263 bundle) under node — a second witness, because the
    /// failure this guards against is silent: a slug that diverges writes to
    /// a directory Claude never reads and nothing anywhere reports it.
    #[test]
    fn slug_matches_claude_project_directories() {
        let expect = |path: &str, want: &str| {
            assert_eq!(workspace_slug(Path::new(path)), want, "path {path:?}");
        };

        // Plain mapping. Also observable directly under ~/.claude/projects.
        expect(
            "/Users/jjrawlins/code/GitHub/JaysonRawlins/ecphory",
            "-Users-jjrawlins-code-GitHub-JaysonRawlins-ecphory",
        );
        expect("/Users/jjrawlins/.claude", "-Users-jjrawlins--claude");

        // Past 200 characters: truncate, then a base36 hash of the ORIGINAL.
        let long = format!(
            "/Users/jjrawlins/code/GitHub/JaysonRawlins/{}",
            "a".repeat(180)
        );
        expect(
            &long,
            &format!(
                "-Users-jjrawlins-code-GitHub-JaysonRawlins-{}-nq877e",
                "a".repeat(157)
            ),
        );
        let deep = format!("/Users/x/{}leaf", "deep/".repeat(60));
        assert!(workspace_slug(Path::new(&deep)).ends_with("-ox660l"));

        // UTF-16 code units, not chars: a non-BMP character is two dashes.
        expect("/Users/x/café/\u{1F600}/repo", "-Users-x-caf-----repo");

        assert_eq!(workspace_tag(Path::new("/a/b")), "ws:-a-b");
    }

    #[test]
    fn slug_hash_matches_js_semantics() {
        // JS `Math.abs` on the i32 minimum yields 2147483648, which a Rust
        // `i32::abs` cannot represent.
        assert_eq!(base36(u64::from(i32::MIN.unsigned_abs())), "zik0zk");
        assert_eq!(base36(0), "0");
        assert_eq!(js_hash(""), 0);
        // (e<<5)-e+c for "ab": 0*31+97 = 97, 97*31+98 = 3105.
        assert_eq!(js_hash("ab"), 3105);
    }

    #[test]
    fn row_needs_something_to_key_on() {
        let mut bare = Episode::new("body only", "test");
        bare.tags = vec!["ws:-x".into()];
        assert_eq!(row(&bare, "ws:-x"), None);

        // Name-only falls back to the name as the query.
        let mut named = Episode::new("body", "test");
        named.name = Some("the name".into());
        let r = row(&named, "ws:-x").unwrap();
        assert_eq!(r.query, "the name");
        assert_eq!(r.description, "");
    }

    #[test]
    fn cells_stay_on_one_line_and_escape_pipes() {
        let e = ep(
            "a | b\nsecond line",
            &["query with | pipe"],
            &["ws:-x", "t|g"],
            "body",
        );
        let r = row(&e, "ws:-x").unwrap();
        assert_eq!(r.query, "query with \\| pipe");
        assert_eq!(r.description, "a \\| b second line");
        assert_eq!(r.tags, vec!["t\\|g"]);
    }

    #[test]
    fn workspace_root_is_shared_by_linked_worktrees() {
        let dir = tempfile::tempdir().unwrap();
        let main = dir.path().join("main");
        std::fs::create_dir_all(&main).unwrap();
        // Hermetic: the fixture must not inherit the developer's git config.
        // A global `core.hooksPath` (or `commit.gpgsign`, or a message
        // template) applies to every repo on the machine including this
        // temp one, and a commit-msg hook that rejects the fixture's
        // trailer-less "init" fails the test for a reason that has nothing
        // to do with workspace resolution. CI passes only because CI has no
        // such config.
        let git = |args: &[&str], cwd: &Path| {
            let out = std::process::Command::new("git")
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_CONFIG_SYSTEM", "/dev/null")
                .arg("-C")
                .arg(cwd)
                .args(args)
                .output()
                .unwrap();
            assert!(out.status.success(), "git {args:?}: {:?}", out);
        };
        git(&["init", "-q", "-b", "main"], &main);
        git(&["config", "user.email", "t@example.com"], &main);
        git(&["config", "user.name", "t"], &main);
        std::fs::write(main.join("f"), "x").unwrap();
        git(&["add", "-A"], &main);
        git(&["commit", "-qm", "init"], &main);

        let wt = dir.path().join("wt");
        git(
            &["worktree", "add", "-q", "-b", "side", wt.to_str().unwrap()],
            &main,
        );

        let from_main = workspace_root(&main);
        let from_worktree = workspace_root(&wt);
        assert_eq!(
            std::fs::canonicalize(&from_main).unwrap(),
            std::fs::canonicalize(&from_worktree).unwrap(),
            "a linked worktree resolved to a different workspace than its main checkout"
        );

        // A subdirectory resolves to the same workspace.
        let sub = main.join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        assert_eq!(
            std::fs::canonicalize(workspace_root(&sub)).unwrap(),
            std::fs::canonicalize(&from_main).unwrap()
        );

        // Outside any repo, the directory is its own workspace.
        let plain = dir.path().join("plain");
        std::fs::create_dir_all(&plain).unwrap();
        // (tempdir itself is not a repo, so `plain` has no enclosing git dir)
        assert_eq!(workspace_root(&plain), plain);
    }
}
