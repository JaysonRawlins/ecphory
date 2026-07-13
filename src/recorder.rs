use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// The flight recorder: every search and every by-id fetch, recorded in the
/// same redb file as the episodes. This is the design's center of gravity —
/// retrieval quality gets scored against the real query stream, not a
/// hand-built gold set. Log entries are local-only by design (queries can
/// contain sensitive terms) and never leave this file.
///
/// Keys are UUIDv7 strings, so key order is time order: retention pruning
/// and "newest first" reads are both range scans, no secondary index.

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggedHit {
    pub id: String,
    pub rank: usize,
    pub score: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchLogEntry {
    pub id: Uuid,
    pub ts: DateTime<Utc>,
    pub query: String,
    pub limit: usize,
    pub include_deleted: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    pub result_count: usize,
    pub results: Vec<LoggedHit>,
    pub latency_us: u64,
}

/// The used-signal: a by-id fetch. A fetch shortly after a search marks
/// which result actually got consumed — ranking failures fall out of the
/// join between this and SearchLogEntry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessLogEntry {
    pub id: Uuid,
    pub ts: DateTime<Utc>,
    pub episode_id: String,
}

/// The consumer's verdict on a search, given at the moment of use.
/// Explicit ratings are ground truth; the access-log temporal join is the
/// fallback inference for unrated searches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Rating {
    /// The results answered the question.
    Hit,
    /// Something useful surfaced, but not the best answer or not ranked well.
    Partial,
    /// Nothing relevant came back.
    Miss,
}

impl std::str::FromStr for Rating {
    type Err = String;
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.trim().to_lowercase().as_str() {
            "hit" => Ok(Rating::Hit),
            "partial" => Ok(Rating::Partial),
            "miss" => Ok(Rating::Miss),
            other => Err(format!("invalid rating {other:?} (expected hit|partial|miss)")),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RatingLogEntry {
    pub id: Uuid,
    pub ts: DateTime<Utc>,
    /// The SearchLogEntry this verdict is about.
    pub search_id: String,
    pub rating: Rating,
    /// Episode ids (full or prefix) from the results that were actually used.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub used_episode_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// Aggregates over the recorded workload, for `ecphory stats`.
#[derive(Debug, Serialize)]
pub struct RecorderStats {
    pub searches: usize,
    pub unique_queries: usize,
    pub zero_hit: usize,
    pub accesses: usize,
    pub latency_us_p50: u64,
    pub latency_us_p95: u64,
    pub latency_us_p99: u64,
    pub latency_us_max: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oldest: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub newest: Option<DateTime<Utc>>,
    pub top_queries: Vec<(String, usize)>,
    /// Explicit consumer verdicts (rate_search): the preferred quality signal.
    pub rated: usize,
    pub rated_hit: usize,
    pub rated_partial: usize,
    pub rated_miss: usize,
}

pub fn compute_stats(
    searches: &[SearchLogEntry],
    accesses: usize,
    ratings: &[RatingLogEntry],
) -> RecorderStats {
    let mut latencies: Vec<u64> = searches.iter().map(|s| s.latency_us).collect();
    latencies.sort_unstable();
    let pct = |p: f64| -> u64 {
        if latencies.is_empty() {
            return 0;
        }
        let idx = ((latencies.len() as f64 * p).ceil() as usize).saturating_sub(1);
        latencies[idx.min(latencies.len() - 1)]
    };

    let mut counts: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for s in searches {
        *counts.entry(s.query.as_str()).or_default() += 1;
    }
    let unique_queries = counts.len();
    let mut top: Vec<(String, usize)> =
        counts.into_iter().map(|(q, n)| (q.to_string(), n)).collect();
    top.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    top.truncate(10);

    RecorderStats {
        searches: searches.len(),
        unique_queries,
        zero_hit: searches.iter().filter(|s| s.result_count == 0).count(),
        accesses,
        latency_us_p50: pct(0.50),
        latency_us_p95: pct(0.95),
        latency_us_p99: pct(0.99),
        latency_us_max: latencies.last().copied().unwrap_or(0),
        oldest: searches.iter().map(|s| s.ts).min(),
        newest: searches.iter().map(|s| s.ts).max(),
        top_queries: top,
        rated: ratings.len(),
        rated_hit: ratings.iter().filter(|r| r.rating == Rating::Hit).count(),
        rated_partial: ratings.iter().filter(|r| r.rating == Rating::Partial).count(),
        rated_miss: ratings.iter().filter(|r| r.rating == Rating::Miss).count(),
    }
}

/// Reads the recorder opt-out. Recording is on unless explicitly disabled.
pub fn recording_enabled() -> bool {
    !matches!(
        std::env::var("ECPHORY_SEARCH_LOG").unwrap_or_default().to_lowercase().as_str(),
        "off" | "false" | "0" | "disabled"
    )
}

/// Retention window in days (ECPHORY_SEARCH_LOG_RETENTION: "90d" or a Go-ish
/// duration is overkill here — days or a plain integer). Default 90.
pub fn retention_days() -> i64 {
    let raw = std::env::var("ECPHORY_SEARCH_LOG_RETENTION").unwrap_or_default();
    let raw = raw.trim();
    if raw.is_empty() {
        return 90;
    }
    let numeric = raw.strip_suffix('d').unwrap_or(raw);
    match numeric.parse::<i64>() {
        Ok(n) if n > 0 => n,
        _ => {
            tracing::warn!("invalid ECPHORY_SEARCH_LOG_RETENTION {raw:?}, using 90d");
            90
        }
    }
}
