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
#[derive(Debug, Clone)]
pub enum Status {
    Delivered(String),
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
            Status::Unproven(_) => "UNPROVEN",
            Status::Misconfigured(_) => "MISCONFIGURED",
        }
    }

    pub fn detail(&self) -> &str {
        match self {
            Status::Delivered(d) | Status::Unproven(d) | Status::Misconfigured(d) => d,
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
    if let Some(text) = read(&home.join(".claude/settings.json")) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
            let hooks = &v["hooks"]["SessionStart"];
            if let Some(arr) = hooks.as_array() {
                for group in arr {
                    if let Some(inner) = group["hooks"].as_array() {
                        for h in inner {
                            if let Some(cmd) = h["command"].as_str() {
                                if is_ecphory_hook(cmd) {
                                    return StaticStatus::Unproven(format!(
                                        "SessionStart hook -> {cmd}; delivery not verified"
                                    ));
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Fallback adapter: a managed block in CLAUDE.md. Claude Code resolves
    // `@import` ONLY for paths relative to the file containing them; absolute
    // and `~`-rooted imports resolve to nothing and report no error.
    if let Some(text) = read(&home.join(".claude/CLAUDE.md")) {
        if let Some(block) = managed_block(&text) {
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
    }

    StaticStatus::Misconfigured(
        "no ecphory adapter: expected a SessionStart hook in .claude/settings.json \
         or a managed block in .claude/CLAUDE.md"
            .into(),
    )
}

fn inspect_codex(home: &Path) -> StaticStatus {
    // Managed block in AGENTS.md is the zero-friction adapter: codex hooks
    // additionally require interactive persisted hook trust, which an installer
    // cannot grant.
    if let Some(text) = read(&home.join(".codex/AGENTS.md")) {
        if managed_block(&text).is_some() {
            return StaticStatus::Unproven(
                "managed block in .codex/AGENTS.md; delivery not verified".into(),
            );
        }
    }

    // A hook in config.toml is a measured silent no-op: codex reads hooks from
    // hooks.json, so the config.toml form installs cleanly and never runs.
    if let Some(text) = read(&home.join(".codex/config.toml")) {
        if toml_declares_ecphory_hook(&text) {
            return StaticStatus::Misconfigured(
                "ecphory hook declared in .codex/config.toml, which codex does not read for \
                 hooks; it must live in .codex/hooks.json"
                    .into(),
            );
        }
    }

    if let Some(text) = read(&home.join(".codex/hooks.json")) {
        if is_ecphory_hook(&text) {
            return StaticStatus::Unproven(
                "SessionStart hook in .codex/hooks.json; requires persisted hook trust \
                 (granted interactively on first run); delivery not verified"
                    .into(),
            );
        }
    }

    StaticStatus::Misconfigured(
        "no ecphory adapter: expected a managed block in .codex/AGENTS.md".into(),
    )
}

fn inspect_opencode(home: &Path) -> StaticStatus {
    let cfg = home.join(".config/opencode/opencode.json");
    let Some(text) = read(&cfg) else {
        return StaticStatus::Misconfigured(
            "no .config/opencode/opencode.json to hold the instructions pointer".into(),
        );
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else {
        return StaticStatus::Misconfigured("opencode.json is not valid JSON".into());
    };
    let Some(arr) = v["instructions"].as_array() else {
        return StaticStatus::Misconfigured(
            "opencode.json has no `instructions` array to point at the artifact".into(),
        );
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
    StaticStatus::Misconfigured("opencode.json `instructions` array is empty".into())
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
    StaticStatus::Misconfigured(
        "no ecphory sessionStart hook in .copilot/hooks/; copilot has no global instruction \
         file, so the hook is its only single-source rail"
            .into(),
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
