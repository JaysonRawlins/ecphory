//! Planning for `ecphory install`.
//!
//! Dry-run is the default, the same shape `purge` uses: the plan prints and
//! nothing changes unless the caller asks for it.
//!
//! The job is smaller than it first looked. All four supported harnesses were
//! measured searching the store unprompted from an ordinary task prompt, so
//! there is no instruction to inject — install only has to wire the MCP server
//! correctly, ONCE, and stamp it with the harness name.
//!
//! Stamping matters because the MCP handshake cannot identify a client: every
//! harness declares itself `rmcp 2.2.0`. Without the stamp a `miss` rated by a
//! harness that could not read the results is indistinguishable from a genuine
//! retrieval failure, and both feed heal replay and the gold set.

use std::path::{Path, PathBuf};

use crate::harness::Harness;

/// Where a harness keeps its MCP server list, and in what syntax.
enum Syntax {
    /// `{"mcpServers": {...}}` or `{"mcp": {...}}`
    Json { key: &'static str },
    /// `[mcp_servers.<name>]` sections
    Toml,
}

fn mcp_config(h: Harness, home: &Path) -> (PathBuf, Syntax) {
    match h {
        Harness::Claude => (
            home.join(".claude.json"),
            Syntax::Json { key: "mcpServers" },
        ),
        Harness::Codex => (home.join(".codex/config.toml"), Syntax::Toml),
        Harness::Opencode => (
            home.join(".config/opencode/opencode.json"),
            Syntax::Json { key: "mcp" },
        ),
        Harness::Copilot => (
            home.join(".copilot/mcp-config.json"),
            Syntax::Json { key: "mcpServers" },
        ),
    }
}

/// The stamp value for each harness. Stable strings: they land in the tape and
/// become the join key for any per-harness quality question.
fn client_name(h: Harness) -> &'static str {
    match h {
        Harness::Claude => "claude-code",
        Harness::Codex => "codex",
        Harness::Opencode => "opencode",
        Harness::Copilot => "copilot",
    }
}

pub const DEFAULT_URL: &str = "http://127.0.0.1:3491/mcp";

/// Add `?client=<name>` to a URL, or report the stamp already present.
/// Returns None when the URL already carries the right stamp.
fn stamped(url: &str, client: &str) -> Option<String> {
    if let Some((_, q)) = url.split_once('?') {
        if let Some(existing) = q.split('&').find_map(|kv| kv.strip_prefix("client=")) {
            return (existing != client).then(|| {
                let base = url.split('?').next().unwrap_or(url);
                format!("{base}?client={client}")
            });
        }
        return Some(format!("{url}&client={client}"));
    }
    Some(format!("{url}?client={client}"))
}

/// An ecphory server found in a harness config.
struct Found {
    name: String,
    url: String,
}

fn find_json(text: &str, key: &str) -> Vec<Found> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(text) else {
        return Vec::new();
    };
    let Some(map) = v.get(key).and_then(|m| m.as_object()) else {
        return Vec::new();
    };
    map.iter()
        .filter(|(name, _)| name.to_lowercase().contains("cphory"))
        .map(|(name, body)| Found {
            name: name.clone(),
            url: body
                .get("url")
                .and_then(|u| u.as_str())
                .unwrap_or_default()
                .to_string(),
        })
        .collect()
}

/// Scan `[mcp_servers.<name>]` sections without pulling in a TOML parser.
/// Deliberately simple: install only needs to FIND entries and report them, and
/// a dry-run that mis-parses is visible in its own output before anything runs.
fn find_toml(text: &str) -> Vec<Found> {
    let mut out: Vec<Found> = Vec::new();
    let mut current: Option<String> = None;
    for line in text.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("[mcp_servers.") {
            let name = rest.trim_end_matches(']').trim_matches('"').to_string();
            current = name.to_lowercase().contains("cphory").then_some(name);
            if let Some(ref n) = current {
                out.push(Found {
                    name: n.clone(),
                    url: String::new(),
                });
            }
            continue;
        }
        if t.starts_with('[') {
            current = None;
            continue;
        }
        if current.is_some()
            && let Some(rest) = t.strip_prefix("url")
            && let Some((_, val)) = rest.split_once('=')
            && let Some(last) = out.last_mut()
        {
            last.url = val.trim().trim_matches('"').to_string();
        }
    }
    out
}

pub enum Action {
    /// No config file at all — nothing to edit, and creating one for a harness
    /// that may not be installed is not install's call to make.
    NoConfig(PathBuf),
    /// No ecphory server configured; one would be added.
    Add { target: PathBuf, url: String },
    /// A server is configured but unstamped; its URL would be rewritten.
    Stamp {
        target: PathBuf,
        server: String,
        from: String,
        to: String,
    },
    /// Already wired and stamped correctly.
    Ok { server: String, client: String },
    /// More than one ecphory server. The agent picks non-deterministically and
    /// a stamp would land on only some traffic, so refuse rather than guess.
    Collision { servers: Vec<String> },
}

pub struct Plan {
    pub harness: Harness,
    pub action: Action,
}

impl Plan {
    pub fn render(&self) -> String {
        let h = self.harness.name();
        match &self.action {
            Action::NoConfig(p) => {
                format!("{h:<9} SKIP          no MCP config at {}", p.display())
            }
            Action::Add { target, url } => format!(
                "{h:<9} ADD           would add an ecphory server -> {url}  [{}]",
                target.display()
            ),
            Action::Stamp {
                target,
                server,
                from,
                to,
            } => format!(
                "{h:<9} STAMP         `{server}` {from} -> {to}  [{}]",
                target.display()
            ),
            Action::Ok { server, client } => {
                format!(
                    "{h:<9} OK            `{server}` already stamped client={client}; no change"
                )
            }
            Action::Collision { servers } => format!(
                "{h:<9} COLLISION     two or more ecphory servers ({}) -- refusing to guess which is \
                 real; remove the extra, then re-run",
                servers.join(", ")
            ),
        }
    }
}

pub fn plan(home: &Path) -> Vec<Plan> {
    Harness::ALL
        .iter()
        .filter(|h| h.is_present(home))
        .map(|&harness| {
            let (target, syntax) = mcp_config(harness, home);
            let client = client_name(harness);
            let Ok(text) = std::fs::read_to_string(&target) else {
                return Plan {
                    harness,
                    action: Action::NoConfig(target),
                };
            };
            let found = match syntax {
                Syntax::Json { key } => find_json(&text, key),
                Syntax::Toml => find_toml(&text),
            };
            let action = match found.len() {
                0 => Action::Add {
                    target,
                    url: format!("{DEFAULT_URL}?client={client}"),
                },
                1 => {
                    let f = &found[0];
                    match stamped(&f.url, client) {
                        Some(to) => Action::Stamp {
                            target,
                            server: f.name.clone(),
                            from: f.url.clone(),
                            to,
                        },
                        None => Action::Ok {
                            server: f.name.clone(),
                            client: client.to_string(),
                        },
                    }
                }
                _ => Action::Collision {
                    servers: found.into_iter().map(|f| f.name).collect(),
                },
            };
            Plan { harness, action }
        })
        .collect()
}

/// Strip an ecphory client stamp back out of a URL, restoring what was there
/// before. Returns None when there is no stamp to remove.
fn unstamped(url: &str) -> Option<String> {
    let (base, q) = url.split_once('?')?;
    let all: Vec<&str> = q.split('&').collect();
    let kept: Vec<&str> = all
        .iter()
        .copied()
        .filter(|kv| !kv.starts_with("client="))
        .collect();
    if kept.len() == all.len() {
        return None;
    }
    Some(if kept.is_empty() {
        base.to_string()
    } else {
        format!("{base}?{}", kept.join("&"))
    })
}

pub enum Outcome {
    Changed(String),
    Unchanged(String),
    Refused(String),
}

/// Replace one URL with another by SURGICAL TEXT EDIT.
///
/// Never a parse-and-reserialize: a JSON or TOML round-trip rewrites the whole
/// file (one measured case took opencode.json from 3952 to 4455 bytes), turning
/// "add a query parameter" into an unreviewable diff of a config the user
/// maintains by hand. Comments and key order in TOML would not survive it at all.
///
/// Refuses when the old URL appears more than once: the edit target is then
/// ambiguous, and guessing wrong silently repoints some other server at ecphory.
fn edit_url(target: &Path, from: &str, to: &str) -> Outcome {
    let Ok(text) = std::fs::read_to_string(target) else {
        return Outcome::Refused(format!("cannot read {}", target.display()));
    };
    match text.matches(from).count() {
        0 => return Outcome::Refused(format!("`{from}` not found in {}", target.display())),
        1 => {}
        n => {
            return Outcome::Refused(format!(
                "`{from}` appears {n} times in {} -- the edit is ambiguous and more than one \
                 server may share it; nothing written",
                target.display()
            ));
        }
    }

    // Back up before touching anything; refuse if the backup cannot be made,
    // rather than making a change that cannot be undone.
    let backup = target.with_extension(format!(
        "{}.ecphory-bak-{}",
        target.extension().and_then(|e| e.to_str()).unwrap_or("cfg"),
        chrono::Utc::now().format("%Y%m%d%H%M%S")
    ));
    if std::fs::write(&backup, &text).is_err() {
        return Outcome::Refused(format!(
            "cannot write a backup at {}; refusing to modify {}",
            backup.display(),
            target.display()
        ));
    }

    match std::fs::write(target, text.replacen(from, to, 1)) {
        Ok(()) => Outcome::Changed(format!(
            "{} updated (backup {})",
            target.display(),
            backup.display()
        )),
        Err(e) => Outcome::Refused(format!("write failed for {}: {e}", target.display())),
    }
}

/// Apply the stamping half of a plan.
pub fn apply(plans: &[Plan]) -> Vec<(Harness, Outcome)> {
    plans
        .iter()
        .map(|p| {
            let outcome = match &p.action {
                Action::Stamp {
                    target, from, to, ..
                } => edit_url(target, from, to),
                Action::Ok { server, .. } => {
                    Outcome::Unchanged(format!("`{server}` already stamped"))
                }
                Action::Collision { servers } => Outcome::Refused(format!(
                    "two or more ecphory servers ({}) -- remove the extra, then re-run",
                    servers.join(", ")
                )),
                Action::Add { .. } => Outcome::Refused(
                    "adding a new server is not implemented yet; configure the MCP server for \
                     this harness by hand, then re-run to have it stamped"
                        .into(),
                ),
                Action::NoConfig(p) => {
                    Outcome::Unchanged(format!("no MCP config at {}", p.display()))
                }
            };
            (p.harness, outcome)
        })
        .collect()
}

/// The reverse: find stamped ecphory servers and strip the stamp back out.
pub fn uninstall_plan(home: &Path) -> Vec<Plan> {
    Harness::ALL
        .iter()
        .filter(|h| h.is_present(home))
        .map(|&harness| {
            let (target, syntax) = mcp_config(harness, home);
            let Ok(text) = std::fs::read_to_string(&target) else {
                return Plan {
                    harness,
                    action: Action::NoConfig(target),
                };
            };
            let found = match syntax {
                Syntax::Json { key } => find_json(&text, key),
                Syntax::Toml => find_toml(&text),
            };
            let action = match found.len() {
                1 => match unstamped(&found[0].url) {
                    Some(to) => Action::Stamp {
                        target,
                        server: found[0].name.clone(),
                        from: found[0].url.clone(),
                        to,
                    },
                    None => Action::Ok {
                        server: found[0].name.clone(),
                        client: "none".into(),
                    },
                },
                0 => Action::NoConfig(target),
                _ => Action::Collision {
                    servers: found.into_iter().map(|f| f.name).collect(),
                },
            };
            Plan { harness, action }
        })
        .collect()
}

/// Per-harness ATTRIBUTION state, for `doctor`.
///
/// Delivery and attribution are different facts and doctor only reported the
/// first. A harness can be DELIVERED and still record every search as
/// `client=None`, because a config edit dropped the `?client=` stamp — and
/// nothing said so. Since a mis-rating from an unattributable harness is
/// indistinguishable from a genuine retrieval failure, and both feed heal
/// replay and the gold set, silence there is expensive.
///
/// Returns one line per harness that needs attention; an empty vec means every
/// detected harness is correctly stamped.
pub fn stamp_report(home: &Path) -> Vec<String> {
    plan(home)
        .into_iter()
        .filter_map(|p| {
            let h = p.harness.name();
            match p.action {
                Action::Stamp { ref server, .. } => Some(format!(
                    "{h:<9} UNSTAMPED     `{server}` carries no client stamp; searches record \
                     client=None (fix: ecphory install --apply)"
                )),
                Action::Add { .. } => Some(format!(
                    "{h:<9} UNSTAMPED     no ecphory server configured, so nothing to attribute"
                )),
                Action::Collision { ref servers } => Some(format!(
                    "{h:<9} UNSTAMPED     two ecphory servers ({}) -- attribution is ambiguous",
                    servers.join(", ")
                )),
                Action::Ok { .. } | Action::NoConfig(_) => None,
            }
        })
        .collect()
}
