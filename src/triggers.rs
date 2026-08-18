//! Post-write triggers: run a local command when a matching episode changes.
//!
//! The store is the one layer every client shares — Claude Code, Codex, and
//! anything else that writes through MCP or HTTP — so an artifact derived
//! from an episode (a rendered session-context file, an exported doc) is
//! regenerated here, on the write itself, instead of relying on each client
//! remembering a second manual step. The motivating incident: an episode
//! edit whose render step was skipped, leaving the rendered file three days
//! stale while its metadata claimed otherwise.
//!
//! Commands come only from the local config file the operator owns
//! (ECPHORY_TRIGGERS_FILE). Episode content never influences what runs.

use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Deserialize;

const DEFAULT_TIMEOUT_SECONDS: u64 = 60;

/// Events a trigger can subscribe to. `insert` and `demote` exist for
/// completeness but are opt-in: the default set is the three events that
/// change an existing episode's content — the drift class this feature
/// exists to close.
const DEFAULT_EVENTS: &[&str] = &["update", "restore", "restore_version"];
const KNOWN_EVENTS: &[&str] = &["insert", "update", "demote", "restore", "restore_version"];

#[derive(Debug, Deserialize)]
struct TriggersConfig {
    #[serde(default)]
    triggers: Vec<TriggerSpec>,
}

#[derive(Debug, Clone, Deserialize)]
struct TriggerSpec {
    name: String,
    /// Subset of KNOWN_EVENTS; empty means DEFAULT_EVENTS.
    #[serde(default)]
    events: Vec<String>,
    #[serde(default, rename = "match")]
    matcher: Matcher,
    /// argv: program plus args. Absolute program path required.
    run: Vec<String>,
    #[serde(default = "default_timeout")]
    timeout_seconds: u64,
}

fn default_timeout() -> u64 {
    DEFAULT_TIMEOUT_SECONDS
}

#[derive(Debug, Clone, Default, Deserialize)]
struct Matcher {
    /// Fires when the episode carries any of these tags.
    #[serde(default)]
    tags_any: Vec<String>,
    /// Fires when the episode id starts with this prefix.
    #[serde(default)]
    id_prefix: Option<String>,
}

impl Matcher {
    fn is_empty(&self) -> bool {
        self.tags_any.is_empty() && self.id_prefix.as_deref().unwrap_or("").is_empty()
    }

    fn matches(&self, id: &str, tags: &[String]) -> bool {
        if let Some(prefix) = self.id_prefix.as_deref()
            && !prefix.is_empty()
            && id.starts_with(prefix)
        {
            return true;
        }
        self.tags_any.iter().any(|t| tags.iter().any(|et| et == t))
    }
}

/// One trigger plus the lock that serializes its runs: overlapping fires for
/// the same trigger queue instead of racing (renders are idempotent, but two
/// concurrent writers to one output file are not).
#[derive(Debug)]
struct Registered {
    spec: TriggerSpec,
    running: Mutex<()>,
}

#[derive(Debug)]
pub struct TriggerEngine {
    triggers: Vec<Arc<Registered>>,
}

impl TriggerEngine {
    /// Build from ECPHORY_TRIGGERS_FILE. Unset → Ok(None) (feature off).
    /// A present-but-invalid config is an Err: the caller decides whether
    /// that's fatal, but it must never be silent.
    pub fn from_env() -> anyhow::Result<Option<Self>> {
        let Ok(path) = std::env::var("ECPHORY_TRIGGERS_FILE") else {
            return Ok(None);
        };
        let raw = std::fs::read_to_string(&path)
            .map_err(|e| anyhow::anyhow!("cannot read triggers file {path}: {e}"))?;
        let engine = Self::from_json(&raw)
            .map_err(|e| anyhow::anyhow!("invalid triggers file {path}: {e}"))?;
        tracing::info!("triggers: {} active from {path}", engine.triggers.len());
        Ok(Some(engine))
    }

    /// Parse a raw JSON config — the testable core of `from_env`.
    pub(crate) fn from_json(raw: &str) -> anyhow::Result<Self> {
        let cfg: TriggersConfig = serde_json::from_str(raw)?;
        Self::from_config(cfg)
    }

    fn from_config(cfg: TriggersConfig) -> anyhow::Result<Self> {
        let mut triggers = Vec::new();
        for spec in cfg.triggers {
            if spec.run.is_empty() {
                anyhow::bail!("trigger {:?}: run must not be empty", spec.name);
            }
            if !spec.run[0].starts_with('/') {
                anyhow::bail!(
                    "trigger {:?}: run[0] must be an absolute path, got {:?}",
                    spec.name,
                    spec.run[0]
                );
            }
            // A trigger with no matcher would fire an external command on
            // every write in the store. That is never what anyone meant.
            if spec.matcher.is_empty() {
                anyhow::bail!(
                    "trigger {:?}: match must set tags_any and/or id_prefix",
                    spec.name
                );
            }
            for ev in &spec.events {
                if !KNOWN_EVENTS.contains(&ev.as_str()) {
                    anyhow::bail!(
                        "trigger {:?}: unknown event {ev:?} (known: {KNOWN_EVENTS:?})",
                        spec.name
                    );
                }
            }
            triggers.push(Arc::new(Registered {
                spec,
                running: Mutex::new(()),
            }));
        }
        Ok(Self { triggers })
    }

    /// Fire-and-forget: spawns a thread per matching trigger and returns
    /// immediately. A trigger can never fail the write that fired it.
    pub fn fire(&self, event: &str, id: &str, tags: &[String]) {
        for reg in &self.triggers {
            let subscribed = if reg.spec.events.is_empty() {
                DEFAULT_EVENTS.contains(&event)
            } else {
                reg.spec.events.iter().any(|e| e == event)
            };
            if !subscribed || !reg.spec.matcher.matches(id, tags) {
                continue;
            }
            let reg = Arc::clone(reg);
            let event = event.to_string();
            let id = id.to_string();
            std::thread::spawn(move || run_one(&reg, &event, &id));
        }
    }
}

fn run_one(reg: &Registered, event: &str, id: &str) {
    // Serialize runs of the same trigger; a poisoned lock just means an
    // earlier run panicked — still safe to proceed.
    let _guard = reg.running.lock().unwrap_or_else(|p| p.into_inner());
    let started = Instant::now();
    let name = &reg.spec.name;

    let spawned = Command::new(&reg.spec.run[0])
        .args(&reg.spec.run[1..])
        .env("ECPHORY_TRIGGER_NAME", name)
        .env("ECPHORY_TRIGGER_EVENT", event)
        .env("ECPHORY_TRIGGER_EPISODE_ID", id)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn();

    let mut child = match spawned {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("trigger {name}: failed to spawn {:?}: {e}", reg.spec.run[0]);
            return;
        }
    };

    let deadline = started + Duration::from_secs(reg.spec.timeout_seconds);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let stderr = child
                    .stderr
                    .take()
                    .and_then(|mut s| {
                        use std::io::Read;
                        let mut buf = String::new();
                        s.read_to_string(&mut buf).ok().map(|_| buf)
                    })
                    .unwrap_or_default();
                if status.success() {
                    tracing::info!(
                        "trigger {name}: ok ({event} {id}) in {:?}",
                        started.elapsed()
                    );
                } else {
                    tracing::warn!(
                        "trigger {name}: exit {status} ({event} {id}): {}",
                        stderr.trim()
                    );
                }
                return;
            }
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    tracing::warn!(
                        "trigger {name}: killed after {}s timeout ({event} {id})",
                        reg.spec.timeout_seconds
                    );
                    return;
                }
                std::thread::sleep(Duration::from_millis(200));
            }
            Err(e) => {
                tracing::warn!("trigger {name}: wait failed ({event} {id}): {e}");
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(json: &str) -> anyhow::Result<TriggerEngine> {
        TriggerEngine::from_json(json)
    }

    #[test]
    fn empty_matcher_is_rejected() {
        let err = spec(r#"{"triggers": [{"name": "t", "run": ["/bin/true"]}]}"#).unwrap_err();
        assert!(err.to_string().contains("match must set"));
    }

    #[test]
    fn relative_program_path_is_rejected() {
        let err =
            spec(r#"{"triggers": [{"name": "t", "run": ["true"], "match": {"tags_any": ["x"]}}]}"#)
                .unwrap_err();
        assert!(err.to_string().contains("absolute path"));
    }

    #[test]
    fn unknown_event_is_rejected() {
        let err = spec(
            r#"{"triggers": [{"name": "t", "run": ["/bin/true"], "events": ["upsert"], "match": {"tags_any": ["x"]}}]}"#,
        )
        .unwrap_err();
        assert!(err.to_string().contains("unknown event"));
    }

    #[test]
    fn matcher_by_tag_and_prefix() {
        let m = Matcher {
            tags_any: vec!["rendered-artifact".into()],
            id_prefix: Some("019f".into()),
        };
        assert!(m.matches("abc", &["rendered-artifact".into()]));
        assert!(m.matches("019f7ac6", &[]));
        assert!(!m.matches("abc", &["other".into()]));
    }

    #[test]
    fn fire_runs_matching_command() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("fired");
        let engine = spec(&format!(
            r#"{{"triggers": [{{"name": "touch", "run": ["/usr/bin/touch", {marker:?}], "match": {{"tags_any": ["hit"]}}}}]}}"#
        ))
        .unwrap();

        // Non-matching tag: nothing runs.
        engine.fire("update", "id1", &["miss".into()]);
        // Non-subscribed event (default set excludes insert): nothing runs.
        engine.fire("insert", "id1", &["hit".into()]);
        std::thread::sleep(Duration::from_millis(300));
        assert!(!marker.exists());

        engine.fire("update", "id1", &["hit".into()]);
        let deadline = Instant::now() + Duration::from_secs(5);
        while !marker.exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(marker.exists(), "trigger command did not run");
    }
}
