//! `scripts/swamp` runs swamp under the maintainer's own swamp-club identity or
//! not at all.
//!
//! This machine may also carry another swamp-club account's login in
//! `~/.config/swamp/auth.json`, and swamp falls back to it silently: with no
//! `SWAMP_API_KEY`, every command authenticates as that other account, and a cached
//! proof means not even a network call is made. Setting only the key is not
//! enough either. With the default config dir, swamp writes the personal
//! identity into that account's `auth.json` and `auth_verified.json`. So the
//! wrapper requires BOTH the personal config dir and a non-empty key, and an
//! agent shell, which never runs the direnv hook, gets one hop through
//! `direnv exec` to load them before it is refused.
//!
//! A fake `swamp` on PATH records whether it was reached and which credential
//! variables it saw, by presence only, so no test ever handles a real key.
//!
//! Red-proofs, 2026-10-19. Against a bare `exec swamp "$@"` stub all six
//! failed. Then one guard at a time was removed from the real wrapper, and
//! exactly one test failed each time:
//! - key-file flag guard: `a_key_file_flag_is_refused`
//! - the unset of SWAMP_API_KEY_FILE/SWAMP_SIGNIN_TOKEN: the
//!   `..._competing_sources_removed` test
//! - the empty-key check: `an_empty_key_is_refused`
//! - the hop marker: `direnv_gets_one_hop_and_cannot_loop`, "saw 3". Before
//!   the fake direnv capped itself, this one hung instead of failing.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const KEY: &str = "fake-personal-key-for-tests";

struct Sandbox {
    _dir: tempfile::TempDir,
    home: PathBuf,
    bin: PathBuf,
    record: PathBuf,
}

impl Sandbox {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let home = dir.path().join("home");
        let bin = dir.path().join("bin");
        let record = dir.path().join("swamp-called");
        fs::create_dir_all(&home).unwrap();
        fs::create_dir_all(&bin).unwrap();
        write_exe(
            &bin.join("swamp"),
            &format!(
                "#!/bin/sh\n{{\n  echo \"args=$*\"\n  echo \"config_dir=$SWAMP_CONFIG_DIR\"\n  \
                 [ -n \"$SWAMP_API_KEY\" ] && echo key=set || echo key=unset\n  \
                 [ -n \"${{SWAMP_API_KEY_FILE+x}}\" ] && echo key_file=set || echo key_file=unset\n  \
                 [ -n \"${{SWAMP_SIGNIN_TOKEN+x}}\" ] && echo signin=set || echo signin=unset\n\
                 }} > '{}'\n",
                record.display()
            ),
        );
        Sandbox { _dir: dir, home, bin, record }
    }

    fn personal_dir(&self) -> String {
        self.home.join(".config/swamp-personal").display().to_string()
    }

    /// Runs the wrapper with a clean environment: PATH is the sandbox bin
    /// plus the system dirs, so a real swamp or direnv cannot be found.
    fn run(&self, env: &[(&str, &str)], args: &[&str]) -> Output {
        let script = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scripts/swamp");
        let mut cmd = Command::new("bash");
        cmd.arg(&script)
            .args(args)
            .env_clear()
            .env("HOME", &self.home)
            .env("PATH", format!("{}:/usr/bin:/bin", self.bin.display()));
        for (k, v) in env {
            cmd.env(k, v);
        }
        cmd.output().expect("run scripts/swamp")
    }

    fn reached(&self) -> Option<String> {
        fs::read_to_string(&self.record).ok()
    }
}

fn write_exe(path: &Path, body: &str) {
    fs::write(path, body).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

fn assert_refused(sb: &Sandbox, out: &Output, why: &str) {
    assert!(
        !out.status.success(),
        "{why}: wrapper exited 0. stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(sb.reached(), None, "{why}: the real swamp was invoked anyway");
}

#[test]
fn no_identity_at_all_is_refused() {
    let sb = Sandbox::new();
    let out = sb.run(&[], &["auth", "whoami"]);
    assert_refused(&sb, &out, "no SWAMP_CONFIG_DIR and no key");
}

#[test]
fn an_empty_key_is_refused() {
    // swamp treats an empty SWAMP_API_KEY as unset and falls back to auth.json.
    let sb = Sandbox::new();
    let dir = sb.personal_dir();
    let out = sb.run(&[("SWAMP_CONFIG_DIR", &dir), ("SWAMP_API_KEY", "")], &["auth", "whoami"]);
    assert_refused(&sb, &out, "empty SWAMP_API_KEY");
}

#[test]
fn the_default_config_dir_is_refused_even_with_a_key() {
    let sb = Sandbox::new();
    let work_dir = sb.home.join(".config/swamp").display().to_string();
    let out = sb.run(&[("SWAMP_CONFIG_DIR", &work_dir), ("SWAMP_API_KEY", KEY)], &["auth", "whoami"]);
    assert_refused(&sb, &out, "SWAMP_CONFIG_DIR is the other login's dir");

    let out = sb.run(&[("SWAMP_API_KEY", KEY)], &["auth", "whoami"]);
    assert_refused(&sb, &out, "key set but SWAMP_CONFIG_DIR unset");
}

#[test]
fn a_key_file_flag_is_refused() {
    // --club-api-key-file outranks SWAMP_API_KEY, so it could name the work key.
    let sb = Sandbox::new();
    let dir = sb.personal_dir();
    let out = sb.run(
        &[("SWAMP_CONFIG_DIR", &dir), ("SWAMP_API_KEY", KEY)],
        &["--club-api-key-file", "/tmp/k", "auth", "whoami"],
    );
    assert_refused(&sb, &out, "--club-api-key-file on the command line");
}

#[test]
fn direnv_gets_one_hop_and_cannot_loop() {
    // A direnv that loads nothing: the wrapper re-runs itself through it once,
    // still has no identity, and must refuse rather than recurse. The fake
    // gives up after three hops so a loop fails this test instead of hanging it.
    let sb = Sandbox::new();
    let hops = sb.bin.join("direnv-hops");
    write_exe(
        &sb.bin.join("direnv"),
        &format!(
            "#!/bin/sh\necho hop >> '{h}'\n[ \"$(wc -l < '{h}')\" -ge 3 ] && exit 99\n\
             [ \"$1\" = exec ] || exit 2\nshift 2\nexec \"$@\"\n",
            h = hops.display()
        ),
    );
    let out = sb.run(&[], &["auth", "whoami"]);
    assert_refused(&sb, &out, "direnv loaded nothing");
    let hop_count = fs::read_to_string(&hops).map(|s| s.lines().count()).unwrap_or(0);
    assert_eq!(hop_count, 1, "expected exactly one direnv hop, saw {hop_count}");
}

#[test]
fn the_personal_identity_reaches_swamp_with_competing_sources_removed() {
    let sb = Sandbox::new();
    let dir = sb.personal_dir();
    let out = sb.run(
        &[
            ("SWAMP_CONFIG_DIR", &dir),
            ("SWAMP_API_KEY", KEY),
            // Both outrank or shadow SWAMP_API_KEY; the wrapper must drop them.
            ("SWAMP_API_KEY_FILE", "/tmp/work-key"),
            ("SWAMP_SIGNIN_TOKEN", "work-proof"),
        ],
        &["model", "search", "a b"],
    );
    assert!(
        out.status.success(),
        "wrapper refused a correct identity. stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let seen = sb.reached().expect("swamp was not invoked");
    assert_eq!(
        seen,
        format!("args=model search a b\nconfig_dir={dir}\nkey=set\nkey_file=unset\nsignin=unset\n")
    );
}
