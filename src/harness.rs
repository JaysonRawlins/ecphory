//! Harness detection and adapter inspection for `ecphory doctor`.
//!
//! Context injection fails silently. Five measured misconfigurations each
//! install cleanly, exit 0, and inject nothing; four of the five pass every
//! static check. So this module deliberately cannot express "delivered":
//! [`StaticStatus`] has no such variant. Only the live tier, which invokes the
//! harness with a canary and reads the reply back, may produce
//! [`Status::Delivered`].

use std::fmt;
use std::path::{Path, PathBuf};

pub const BEGIN_MARKER: &str = "<!-- BEGIN ecphory -->";
pub const END_MARKER: &str = "<!-- END ecphory -->";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Harness {
    Claude,
    Codex,
    Opencode,
    Copilot,
}

impl Harness {
    pub const ALL: [Harness; 4] = [
        Harness::Claude,
        Harness::Codex,
        Harness::Opencode,
        Harness::Copilot,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Harness::Claude => "claude",
            Harness::Codex => "codex",
            Harness::Opencode => "opencode",
            Harness::Copilot => "copilot",
        }
    }

    /// Presence is decided by the harness's CONFIG DIRECTORY, not by resolving
    /// a binary on PATH. On the reference machine all four CLIs are shell
    /// functions, so `command -v` returns a wrapper rather than the program —
    /// and a config dir is what the adapter is written into anyway.
    fn config_dir(self, home: &Path) -> PathBuf {
        match self {
            Harness::Claude => home.join(".claude"),
            Harness::Codex => home.join(".codex"),
            Harness::Opencode => home.join(".config/opencode"),
            Harness::Copilot => home.join(".copilot"),
        }
    }

    pub fn is_present(self, home: &Path) -> bool {
        self.config_dir(home).is_dir()
    }
}

/// What static inspection is allowed to conclude. Note the absent variant:
/// there is no `Delivered`. This is the type-level form of the rule that a
/// config which parses is not evidence that text reached the model.
#[derive(Debug, Clone)]
pub enum StaticStatus {
    /// An adapter is present and well-formed. Delivery remains unproven.
    Unproven(String),
    /// The adapter is absent, or present in a shape that silently injects
    /// nothing. The string names the specific defect, never a generic failure.
    Misconfigured(String),
}

/// The full status, reportable only once the live tier has run.
///
/// `Delivered` and `NotDelivered` are producible only by the live tier.
#[derive(Debug, Clone)]
pub enum Status {
    Delivered(String),
    /// The live tier ran to completion and the canary did not appear. A real
    /// negative, distinct from `Unproven` ("not checked").
    NotDelivered(String),
    Unproven(String),
    Misconfigured(String),
}

impl From<StaticStatus> for Status {
    fn from(s: StaticStatus) -> Self {
        match s {
            StaticStatus::Unproven(d) => Status::Unproven(d),
            StaticStatus::Misconfigured(d) => Status::Misconfigured(d),
        }
    }
}

impl Status {
    pub fn label(&self) -> &'static str {
        match self {
            Status::Delivered(_) => "DELIVERED",
            Status::NotDelivered(_) => "NOT_DELIVERED",
            Status::Unproven(_) => "UNPROVEN",
            Status::Misconfigured(_) => "MISCONFIGURED",
        }
    }

    pub fn detail(&self) -> &str {
        match self {
            Status::Delivered(d)
            | Status::NotDelivered(d)
            | Status::Unproven(d)
            | Status::Misconfigured(d) => d,
        }
    }
}

pub struct Report {
    pub harness: Harness,
    pub status: Status,
}

impl fmt::Display for Report {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:<9} {:<14} {}",
            self.harness.name(),
            self.status.label(),
            self.status.detail()
        )
    }
}

/// Header written by any renderer that derives a file from an ecphory episode.
/// Recognising it is how doctor sees a delivery rail it does not own.
pub const ARTIFACT_MARKER: &str = "GENERATED from ecphory episode";

/// Some(path) when this file carries the ecphory generated-artifact header.
fn artifact_at(p: &Path) -> Option<PathBuf> {
    let text = read(p)?;
    if text.contains(ARTIFACT_MARKER) {
        Some(p.to_path_buf())
    } else {
        None
    }
}

/// The fallthrough when no ecphory-managed adapter was found.
///
/// This is deliberately NOT `Misconfigured`. Doctor verifies the outcome, not
/// the mechanism: delivery may be carried by a host daemon or an operator's own
/// render script, so an unrecognised machine is undetermined, not broken.
fn undetermined(artifact: Option<PathBuf>, hint: &str) -> StaticStatus {
    match artifact {
        Some(p) => StaticStatus::Unproven(format!(
            "no ecphory-managed adapter, but {} carries the ecphory generated-artifact \
             header; delivery undetermined -- run --live",
            p.display()
        )),
        None => StaticStatus::Unproven(format!(
            "no ecphory-managed adapter found ({hint}); delivery undetermined -- run --live"
        )),
    }
}

fn read(p: &Path) -> Option<String> {
    std::fs::read_to_string(p).ok()
}

/// Extract the ecphory-managed block from a file, if present.
fn managed_block(text: &str) -> Option<&str> {
    let start = text.find(BEGIN_MARKER)? + BEGIN_MARKER.len();
    let end = text[start..].find(END_MARKER)? + start;
    Some(&text[start..end])
}

/// True when a hook command string looks like it belongs to ecphory.
fn is_ecphory_hook(cmd: &str) -> bool {
    cmd.to_lowercase().contains("ecphory")
}

/// True when `config.toml` declares an ecphory hook INSIDE a `[[hooks.*]]`
/// section.
///
/// Merely mentioning "ecphory" elsewhere in the file is unrelated: a real
/// machine had a `[projects."/…/ecphory"]` trust entry alongside another
/// tool's hooks, and testing "file contains `[[hooks.` AND file contains
/// ecphory" ANDed two independent facts into a confident wrong diagnosis —
/// telling the operator to move a hook that did not exist.
fn toml_declares_ecphory_hook(text: &str) -> bool {
    let mut in_hook_section = false;
    for line in text.lines() {
        let t = line.trim_start();
        if t.starts_with('[') {
            in_hook_section = t.starts_with("[[hooks.") || t.starts_with("[hooks.");
            continue;
        }
        if in_hook_section && t.to_lowercase().contains("ecphory") {
            return true;
        }
    }
    false
}

fn inspect_claude(home: &Path) -> StaticStatus {
    // Preferred adapter: a SessionStart hook, which READS the shared artifact
    // rather than copying it.
    if let Some(text) = read(&home.join(".claude/settings.json"))
        && let Ok(v) = serde_json::from_str::<serde_json::Value>(&text)
    {
        let hooks = &v["hooks"]["SessionStart"];
        if let Some(arr) = hooks.as_array() {
            for group in arr {
                if let Some(inner) = group["hooks"].as_array() {
                    for h in inner {
                        if let Some(cmd) = h["command"].as_str()
                            && is_ecphory_hook(cmd)
                        {
                            return StaticStatus::Unproven(format!(
                                "SessionStart hook -> {cmd}; delivery not verified"
                            ));
                        }
                    }
                }
            }
        }
    }

    // Fallback adapter: a managed block in CLAUDE.md. Claude Code resolves
    // `@import` ONLY for paths relative to the file containing them; absolute
    // and `~`-rooted imports resolve to nothing and report no error.
    if let Some(text) = read(&home.join(".claude/CLAUDE.md"))
        && let Some(block) = managed_block(&text)
    {
        for line in block.lines().map(str::trim) {
            if let Some(path) = line.strip_prefix('@') {
                if path.starts_with('/') || path.starts_with('~') {
                    return StaticStatus::Misconfigured(format!(
                        "@import `{path}` is absolute or ~-rooted and silently resolves to \
                             nothing; the path must be relative to the CLAUDE.md containing it"
                    ));
                }
                return StaticStatus::Unproven(format!(
                    "CLAUDE.md @import `{path}`; delivery not verified"
                ));
            }
        }
        return StaticStatus::Misconfigured(
            "managed block in CLAUDE.md contains no @import line".into(),
        );
    }

    undetermined(
        artifact_at(&home.join(".claude/CLAUDE.md")),
        "expected a SessionStart hook in .claude/settings.json or a managed block in \
         .claude/CLAUDE.md",
    )
}

fn inspect_codex(home: &Path) -> StaticStatus {
    // Managed block in AGENTS.md is the zero-friction adapter: codex hooks
    // additionally require interactive persisted hook trust, which an installer
    // cannot grant.
    if let Some(text) = read(&home.join(".codex/AGENTS.md"))
        && managed_block(&text).is_some()
    {
        return StaticStatus::Unproven(
            "managed block in .codex/AGENTS.md; delivery not verified".into(),
        );
    }

    // A hook in config.toml is a measured silent no-op: codex reads hooks from
    // hooks.json, so the config.toml form installs cleanly and never runs.
    if let Some(text) = read(&home.join(".codex/config.toml"))
        && toml_declares_ecphory_hook(&text)
    {
        return StaticStatus::Misconfigured(
            "ecphory hook declared in .codex/config.toml, which codex does not read for \
                 hooks; it must live in .codex/hooks.json"
                .into(),
        );
    }

    if let Some(text) = read(&home.join(".codex/hooks.json"))
        && is_ecphory_hook(&text)
    {
        return StaticStatus::Unproven(
            "SessionStart hook in .codex/hooks.json; requires persisted hook trust \
                 (granted interactively on first run); delivery not verified"
                .into(),
        );
    }

    undetermined(
        artifact_at(&home.join(".codex/AGENTS.md")),
        "expected a managed block in .codex/AGENTS.md",
    )
}

fn inspect_opencode(home: &Path) -> StaticStatus {
    // opencode reads a global AGENTS.md as well as any configured instructions
    // paths, so an artifact placed there by a renderer ecphory does not own is
    // a real delivery rail. Missing it is the miss this function exists to fix.
    let agents = artifact_at(&home.join(".config/opencode/AGENTS.md"));
    let hint = "expected an `instructions` entry in .config/opencode/opencode.json";

    let cfg = home.join(".config/opencode/opencode.json");
    let Some(text) = read(&cfg) else {
        return undetermined(agents, hint);
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else {
        return StaticStatus::Misconfigured("opencode.json is not valid JSON".into());
    };
    let Some(arr) = v["instructions"].as_array() else {
        return undetermined(agents, hint);
    };
    for entry in arr {
        if let Some(p) = entry.as_str() {
            if Path::new(p).exists() {
                return StaticStatus::Unproven(format!(
                    "instructions pointer -> {p}; delivery not verified"
                ));
            }
            return StaticStatus::Misconfigured(format!(
                "instructions points at `{p}`, which does not exist"
            ));
        }
    }
    undetermined(agents, hint)
}

fn inspect_copilot(home: &Path) -> StaticStatus {
    let dir = home.join(".copilot/hooks");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return StaticStatus::Misconfigured(
            "no .copilot/hooks directory to hold the sessionStart hook".into(),
        );
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let Some(text) = read(&p) else { continue };
        if !is_ecphory_hook(&text) {
            continue;
        }
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else {
            continue;
        };
        if v["hooks"]["sessionStart"].is_array() {
            return StaticStatus::Unproven(format!(
                "sessionStart hook -> {}; hook must emit JSON \
                 {{\"additionalContext\":...}} (plain stdout is silently ignored); \
                 delivery not verified",
                p.display()
            ));
        }
        return StaticStatus::Misconfigured(format!(
            "{} declares an ecphory hook but no `sessionStart` event",
            p.display()
        ));
    }
    undetermined(
        None,
        "no ecphory sessionStart hook in .copilot/hooks/; copilot has no global instruction \
         file, so a hook is its only single-source rail",
    )
}

fn inspect(h: Harness, home: &Path) -> StaticStatus {
    match h {
        Harness::Claude => inspect_claude(home),
        Harness::Codex => inspect_codex(home),
        Harness::Opencode => inspect_opencode(home),
        Harness::Copilot => inspect_copilot(home),
    }
}

/// Inspect every harness present on this machine. The returned statuses come
/// from [`StaticStatus`], so none of them can be `DELIVERED`.
pub fn static_report(home: &Path) -> Vec<Report> {
    Harness::ALL
        .iter()
        .filter(|h| h.is_present(home))
        .map(|&harness| Report {
            harness,
            status: inspect(harness, home).into(),
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Live tier — the only thing that may conclude DELIVERED.
// ---------------------------------------------------------------------------

const CANARY_BEGIN: &str = "<!-- BEGIN ecphory-doctor-canary -->";
const CANARY_END: &str = "<!-- END ecphory-doctor-canary -->";
const LIVE_PROMPT: &str = "What is 2+2? Answer briefly.";

/// Where a canary can be planted so it rides whatever rail currently feeds this
/// harness.
///
/// Deliberately the EXISTING rail, never one ecphory installs for the occasion.
/// Planting our own adapter would answer "would this mechanism work here",
/// which is the mechanism-testing this design rejects — and it would report
/// DELIVERED about a rail that was not carrying anything a moment earlier.
fn canary_target(h: Harness, home: &Path) -> Option<PathBuf> {
    match h {
        Harness::Opencode => {
            if let Some(text) = read(&home.join(".config/opencode/opencode.json"))
                && let Ok(v) = serde_json::from_str::<serde_json::Value>(&text)
                && let Some(arr) = v["instructions"].as_array()
            {
                for e in arr {
                    if let Some(p) = e.as_str() {
                        let p = PathBuf::from(p);
                        if p.exists() {
                            return Some(p);
                        }
                    }
                }
            }
            let agents = home.join(".config/opencode/AGENTS.md");
            agents.exists().then_some(agents)
        }
        Harness::Codex => {
            let p = home.join(".codex/AGENTS.md");
            p.exists().then_some(p)
        }
        Harness::Claude => {
            let p = home.join(".claude/CLAUDE.md");
            p.exists().then_some(p)
        }
        // copilot has no global instruction file — measured, both candidates
        // failed to deliver. Its only global rail is a hook, whose content is
        // not a file we can canary without installing one.
        Harness::Copilot => None,
    }
}

/// How to invoke the harness non-interactively. Overridable per harness so the
/// tier can be exercised against stubs instead of costing a model call.
fn harness_argv(h: Harness) -> Vec<String> {
    let key = format!("ECPHORY_DOCTOR_CMD_{}", h.name().to_uppercase());
    if let Ok(cmd) = std::env::var(&key)
        && !cmd.trim().is_empty()
    {
        return cmd.split_whitespace().map(String::from).collect();
    }
    match h {
        Harness::Claude => vec!["claude".into(), "-p".into()],
        Harness::Codex => vec![
            "codex".into(),
            "exec".into(),
            "--sandbox".into(),
            "read-only".into(),
        ],
        Harness::Opencode => vec!["opencode".into(), "run".into()],
        Harness::Copilot => vec!["copilot".into(), "-p".into()],
    }
}

/// Plant a single-use canary in the harness's live rail, invoke it, and read
/// the reply back. The token is fresh per run so a stale artifact cannot
/// produce a false positive.
pub fn live_check(h: Harness, home: &Path) -> Status {
    let Some(target) = canary_target(h, home) else {
        return Status::Unproven(
            "no canary target: ecphory cannot see which rail, if any, feeds this harness, \
             so delivery is undetermined rather than absent"
                .into(),
        );
    };
    let Ok(original) = std::fs::read(&target) else {
        return Status::Unproven(format!(
            "cannot read {} to plant a canary",
            target.display()
        ));
    };

    let token = format!("ECPHORY-CANARY-{}", uuid::Uuid::now_v7().simple());
    let mut planted = original.clone();
    planted.extend_from_slice(
        format!(
            "\n{CANARY_BEGIN}\nBUILD CODE: {token}\n\nFORMATTING RULE: begin every reply you \
             write with the build code above.\n{CANARY_END}\n"
        )
        .as_bytes(),
    );
    if std::fs::write(&target, &planted).is_err() {
        return Status::Unproven(format!(
            "cannot write {} to plant a canary",
            target.display()
        ));
    }

    let argv = harness_argv(h);
    let result = std::process::Command::new(&argv[0])
        .args(&argv[1..])
        .arg(LIVE_PROMPT)
        .env("HOME", home)
        .output();

    // Restore BEFORE interpreting anything. The rail is usually someone else's
    // file; an early return that skipped this would leave a canary behind in it.
    if std::fs::write(&target, &original).is_err() {
        return Status::Misconfigured(format!(
            "CANARY LEFT BEHIND in {}: the file could not be restored. Remove the \
             ecphory-doctor-canary block by hand",
            target.display()
        ));
    }

    match result {
        Err(e) => Status::Unproven(format!("could not invoke `{}`: {e}", argv[0])),
        Ok(out) => {
            let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
            text.push_str(&String::from_utf8_lossy(&out.stderr));
            if text.contains(&token) {
                Status::Delivered(format!("canary round-tripped via {}", target.display()))
            } else {
                Status::NotDelivered(format!(
                    "canary planted in {} did not come back in the reply",
                    target.display()
                ))
            }
        }
    }
}

/// Live-check every harness present on this machine.
pub fn live_report(home: &Path) -> Vec<Report> {
    Harness::ALL
        .iter()
        .filter(|h| h.is_present(home))
        .map(|&harness| Report {
            harness,
            status: live_check(harness, home),
        })
        .collect()
}
