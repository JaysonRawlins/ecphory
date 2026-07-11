use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A single memory. Content is stored verbatim — the server never rewrites it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Episode {
    pub id: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub content: String,
    /// Write-time lexical enrichment: paraphrase cues the capturing agent
    /// generates so BM25 lands the episode for vocabulary it doesn't contain.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub search_phrases: Vec<String>,
    pub source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_description: Option<String>,
    #[serde(default = "default_group")]
    pub group_id: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    pub created_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub valid_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expired_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deleted_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "serde_json::Value::is_null")]
    pub metadata: serde_json::Value,
}

fn default_group() -> String {
    "default".to_string()
}

impl Episode {
    pub fn new(content: impl Into<String>, source: impl Into<String>) -> Self {
        Self {
            // UUIDv7: time-ordered, so redb's key order is creation order and
            // short prefixes stay collision-friendly.
            id: Uuid::now_v7(),
            name: None,
            content: content.into(),
            search_phrases: Vec::new(),
            source: source.into(),
            source_model: None,
            source_description: None,
            group_id: default_group(),
            tags: Vec::new(),
            created_at: Utc::now(),
            valid_at: None,
            expired_at: None,
            deleted_at: None,
            metadata: serde_json::Value::Null,
        }
    }

    pub fn is_deleted(&self) -> bool {
        self.deleted_at.is_some()
    }
}

/// Fields that may be changed on an existing episode. The prior state is
/// archived as an EpisodeVersion before any of these apply.
#[derive(Debug, Default, Clone, Deserialize)]
pub struct UpdateParams {
    pub name: Option<String>,
    pub content: Option<String>,
    pub search_phrases: Option<Vec<String>>,
    pub tags: Option<Vec<String>>,
    pub expired_at: Option<DateTime<Utc>>,
    pub metadata: Option<serde_json::Value>,
}

/// Archived prior state of an episode, captured before update or demote so
/// every mutation is recoverable.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpisodeVersion {
    pub version_id: Uuid,
    pub archived_at: DateTime<Utc>,
    /// "update", "delete", or "restore"
    pub operation: String,
    pub episode: Episode,
}
