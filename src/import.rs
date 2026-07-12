use std::path::Path;

use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::error::{Error, Result};
use crate::model::Episode;

pub struct MirrorRead {
    pub episodes: Vec<Episode>,
    /// (path, reason) for files that didn't yield an episode. Real mirrors
    /// contain the occasional empty or malformed row; import reports these
    /// instead of dying — silent truncation would be worse, a hard fail on
    /// one bad file worse still.
    pub skipped: Vec<(String, String)>,
}

/// Parse an engram git-export mirror tree (one markdown file per episode,
/// YAML frontmatter + body) into Episodes.
///
/// The mirror format is deterministic single-line `key: "value"` scalars
/// (double-quoted, JSON-compatible escaping) plus JSON arrays for tags — so
/// each value parses as a JSON literal and no YAML dependency is needed.
/// The embedding field is deliberately ignored: ecphory is lexical-first.
pub fn read_mirror(dir: impl AsRef<Path>) -> Result<MirrorRead> {
    let mut episodes = Vec::new();
    let mut skipped = Vec::new();
    let mut stack = vec![dir.as_ref().to_path_buf()];
    while let Some(d) = stack.pop() {
        let entries = std::fs::read_dir(&d)
            .map_err(|e| Error::Storage(format!("reading {}: {e}", d.display())))?;
        for entry in entries {
            let path = entry
                .map_err(|e| Error::Storage(format!("reading dir entry: {e}")))?
                .path();
            if path.is_dir() {
                // Skip the mirror's own .git if present.
                if path.file_name().is_some_and(|n| n == ".git") {
                    continue;
                }
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "md") {
                let raw = std::fs::read_to_string(&path)
                    .map_err(|e| Error::Storage(format!("reading {}: {e}", path.display())))?;
                match parse_episode(&raw) {
                    Ok(ep) => episodes.push(ep),
                    Err(e) => skipped.push((path.display().to_string(), e.to_string())),
                }
            }
        }
    }
    Ok(MirrorRead { episodes, skipped })
}

fn parse_episode(raw: &str) -> Result<Episode> {
    let rest = raw
        .strip_prefix("---\n")
        .ok_or_else(|| Error::Storage("missing frontmatter open".into()))?;
    let (front, body) = rest
        .split_once("\n---\n")
        .ok_or_else(|| Error::Storage("missing frontmatter close".into()))?;

    let mut ep = Episode::new(String::new(), "import");
    ep.metadata = serde_json::Value::Null;

    for line in front.lines() {
        let Some((key, value)) = line.split_once(": ") else {
            continue;
        };
        let key = key.trim();
        let value = value.trim();
        match key {
            "embedding" => continue, // lexical-first: vectors not imported
            "id" => ep.id = Uuid::parse_str(&json_str(value)?).map_err(|e| Error::Storage(format!("bad id: {e}")))?,
            "name" => ep.name = Some(json_str(value)?),
            "search_phrases" => ep.search_phrases = serde_json::from_str(value)?,
            "source" => ep.source = json_str(value)?,
            "source_model" => ep.source_model = Some(json_str(value)?),
            "source_description" => ep.source_description = Some(json_str(value)?),
            "group_id" => ep.group_id = json_str(value)?,
            "tags" => ep.tags = serde_json::from_str(value)?,
            "created_at" => ep.created_at = json_time(value)?,
            "valid_at" => ep.valid_at = Some(json_time(value)?),
            "expired_at" => ep.expired_at = Some(json_time(value)?),
            "deleted_at" => ep.deleted_at = Some(json_time(value)?),
            "metadata" => {
                // Stored as a quoted JSON string in the mirror; keep the
                // decoded JSON if it parses, else the raw string.
                let s = json_str(value)?;
                ep.metadata = serde_json::from_str(&s).unwrap_or(serde_json::Value::String(s));
            }
            _ => {}
        }
    }

    ep.content = body.trim().to_string();
    if ep.content.is_empty() {
        return Err(Error::Storage("empty episode body".into()));
    }
    Ok(ep)
}

/// Frontmatter scalars are double-quoted with JSON-compatible escapes.
fn json_str(value: &str) -> Result<String> {
    Ok(serde_json::from_str(value)?)
}

fn json_time(value: &str) -> Result<DateTime<Utc>> {
    let s: String = serde_json::from_str(value)?;
    DateTime::parse_from_rfc3339(&s)
        .map(|t| t.with_timezone(&Utc))
        .map_err(|e| Error::Storage(format!("bad timestamp {s}: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"---
id: "a4286fc4-ebdb-46a9-8d3b-c6fe83695b40"
name: "sample-note"
source: "claude-code"
source_description: "an assessment of a claim that X is \"the only safe bet\" today"
group_id: "default"
tags: ["reference","supply-chain"]
created_at: "2026-06-30T20:35:55Z"
valid_at: "2026-06-30T20:00:00Z"
embedding: "[0.047736686, 0.09486618]"
---

The body of the note.
Second line.
"#;

    #[test]
    fn parses_mirror_file() {
        let ep = parse_episode(SAMPLE).unwrap();
        assert_eq!(ep.id.to_string(), "a4286fc4-ebdb-46a9-8d3b-c6fe83695b40");
        assert_eq!(ep.name.as_deref(), Some("sample-note"));
        assert_eq!(ep.tags, vec!["reference", "supply-chain"]);
        // Escaped quotes inside YAML double-quoted scalars survive.
        assert!(ep.source_description.unwrap().contains("\"the only safe bet\""));
        assert!(ep.content.starts_with("The body"));
        assert!(ep.deleted_at.is_none());
    }

    #[test]
    fn parses_deleted_marker() {
        let raw = SAMPLE.replace(
            "valid_at: \"2026-06-30T20:00:00Z\"",
            "deleted_at: \"2026-07-01T00:00:00Z\"",
        );
        let ep = parse_episode(&raw).unwrap();
        assert!(ep.is_deleted());
    }
}
