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

/// Provenance of a taped search. Only Organic entries are workload signal;
/// everything else is synthetic traffic that must stay visible on the tape
/// (filterable, debuggable) without polluting top-queries and the quality
/// aggregates. This generalizes the v0.2 recorder bypass: `no_record` still
/// skips the tape entirely, but jobs that SHOULD be taped for visibility now
/// tag themselves instead of hiding.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SearchOrigin {
    /// A real consumer asking a real question — the default, and the only
    /// origin that counts toward workload aggregates.
    #[default]
    Organic,
    /// Eval replays (gold / from-log): scoring traffic, not usage.
    Eval,
    /// Bulk maintenance sweeps (enrichment backfills, migrations).
    Backfill,
    /// Heal-replay regression passes re-running healed misses.
    HealReplay,
}

impl SearchOrigin {
    pub fn is_organic(&self) -> bool {
        matches!(self, SearchOrigin::Organic)
    }
}

impl std::fmt::Display for SearchOrigin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SearchOrigin::Organic => write!(f, "organic"),
            SearchOrigin::Eval => write!(f, "eval"),
            SearchOrigin::Backfill => write!(f, "backfill"),
            SearchOrigin::HealReplay => write!(f, "heal-replay"),
        }
    }
}

impl std::str::FromStr for SearchOrigin {
    type Err = String;
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        // Liberal on the separator: "heal-replay" is canonical, but the
        // snake_case habit is inevitable in query strings.
        match s.trim().to_lowercase().replace('_', "-").as_str() {
            "" | "organic" => Ok(SearchOrigin::Organic),
            "eval" => Ok(SearchOrigin::Eval),
            "backfill" => Ok(SearchOrigin::Backfill),
            "heal-replay" => Ok(SearchOrigin::HealReplay),
            other => Err(format!(
                "invalid origin {other:?} (expected organic|eval|backfill|heal-replay)"
            )),
        }
    }
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
    /// Episode-source FILTER applied to this search (mirrors
    /// SearchOptions.source) — not to be confused with `origin`, which is
    /// the provenance of the search itself.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    /// Who issued this search: organic consumer traffic or a tagged
    /// synthetic job. Absent on pre-0.4 rows, which are all organic (any
    /// synthetic traffic back then either bypassed the tape or is caught by
    /// the eval's burst filter).
    #[serde(default, skip_serializing_if = "SearchOrigin::is_organic")]
    pub origin: SearchOrigin,
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
            other => Err(format!(
                "invalid rating {other:?} (expected hit|partial|miss)"
            )),
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
    /// Episodes that SHOULD have surfaced (miss/partial) — explicit ground
    /// truth for self-correction. Never inferred from access joins: enriching
    /// a wrongly-guessed target would bury the right one behind it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub intended_episode_ids: Vec<String>,
    /// Self-correction outcomes for this rating (enrich → redo → validate).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub corrections: Vec<Correction>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// One self-correction attempt: on a non-hit rating with a known target,
/// the missed query is appended to the target's search_phrases (lexical
/// enrichment — the query IS how this will be asked for again), the search
/// re-runs, and the outcome records whether that closed the gap.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Correction {
    pub episode_id: String,
    pub action: CorrectionAction,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before_rank: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after_rank: Option<usize>,
    /// Collateral damage from the enrichment: episodes that prior ratings
    /// marked as used (or that earlier heals validated) and that this heal
    /// displaced out of the top k. A warning, never a failure — the consumer
    /// decides whether the trade was worth it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub displaced_used: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CorrectionAction {
    /// Target already ranks within k for this query — nothing to fix.
    AlreadyRanks,
    /// Phrase appended and the redo validated: target now ranks within k.
    Enriched,
    /// Phrase appended but the target STILL ranks outside k — the gap is
    /// ranking/crowding, not vocabulary. Needs human or ranking-layer work.
    EnrichedStillLow,
    /// The query is already a search phrase on the target yet it still
    /// misses — enrichment can't help; suspect crowding or a search bug.
    DuplicatePhrase,
    /// Target already carries the max phrases; not appended. Curation flag.
    PhraseCapReached,
    /// The intended id didn't resolve to an episode.
    TargetNotFound,
}

/// A validated heal, persisted as its own record so the rating it resolves
/// stays immutable. The original miss is ground truth for first-contact
/// failure rate; "resetting the miss to a hit" by mutation would cook the
/// acceptance metrics — resolution is a layer on top, never a rewrite.
///
/// Resolutions are PERMANENT (never pruned with the 90-day recorder
/// retention): every healed miss is a regression test, so the record
/// carries the query itself — the taped search it came from will age out.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolutionLogEntry {
    pub id: Uuid,
    pub ts: DateTime<Utc>,
    /// The immutable RatingLogEntry this resolves.
    pub rating_id: String,
    /// The taped search the rating was about (may be pruned by retention).
    pub search_id: String,
    /// The failed query, denormalized so heal-replay outlives the tape.
    pub query: String,
    /// Canonical id of the intended episode that validated.
    pub episode_id: String,
    /// Rank of the intended episode at validation time (within top k).
    pub validated_rank: usize,
    /// Top-k episode ids right after validation — the replay baseline for
    /// the collateral-damage diff (current vs as-healed).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub top_k: Vec<String>,
    /// Used/hit episodes the heal displaced out of the top k at validation.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub displaced_used: Vec<String>,
    /// Outcome of the most recent heal-replay pass; None until first replay.
    /// This is the ONLY mutable part of the heal lifecycle — the rating and
    /// the resolution facts above are written once.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_replay: Option<ReplayOutcome>,
}

/// One heal-replay verdict: does the intended episode still rank within
/// top k for the original failed query, against the live index?
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayOutcome {
    pub ts: DateTime<Utc>,
    /// Current rank of the intended episode; None means fell out of top k.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rank: Option<usize>,
    pub held: bool,
    /// Used/hit episodes present in the as-healed top k but missing now.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub displaced_used: Vec<String>,
}

/// Aggregates over the recorded workload, for `ecphory stats`. Workload
/// numbers (searches, unique/top queries, zero-hit, latency percentiles)
/// cover ORGANIC entries only — tagged synthetic traffic (eval, backfill,
/// heal-replay) is counted separately so it can't cook the signal.
#[derive(Debug, Serialize)]
pub struct RecorderStats {
    pub searches: usize,
    /// Tagged non-organic searches on the tape (eval/backfill/heal-replay).
    pub synthetic: usize,
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
    /// rated_miss split by resolution state: healed misses have at least one
    /// validated resolution; outstanding ones still need work. The split is
    /// a layer over the immutable ratings — rated_miss itself never shrinks.
    pub rated_miss_outstanding: usize,
    pub rated_miss_healed: usize,
    /// Heal lifecycle: total validated resolutions (permanent), and how many
    /// failed their most recent heal-replay pass.
    pub heals: usize,
    pub heals_regressed: usize,
}

pub fn compute_stats(
    searches: &[SearchLogEntry],
    accesses: usize,
    ratings: &[RatingLogEntry],
    resolutions: &[ResolutionLogEntry],
) -> RecorderStats {
    let organic: Vec<&SearchLogEntry> = searches.iter().filter(|s| s.origin.is_organic()).collect();
    let mut latencies: Vec<u64> = organic.iter().map(|s| s.latency_us).collect();
    latencies.sort_unstable();
    let pct = |p: f64| -> u64 {
        if latencies.is_empty() {
            return 0;
        }
        let idx = ((latencies.len() as f64 * p).ceil() as usize).saturating_sub(1);
        latencies[idx.min(latencies.len() - 1)]
    };

    let mut counts: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for s in &organic {
        *counts.entry(s.query.as_str()).or_default() += 1;
    }
    let unique_queries = counts.len();
    let mut top: Vec<(String, usize)> = counts
        .into_iter()
        .map(|(q, n)| (q.to_string(), n))
        .collect();
    top.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    top.truncate(10);

    let resolved: std::collections::HashSet<&str> =
        resolutions.iter().map(|r| r.rating_id.as_str()).collect();
    let rated_miss = ratings.iter().filter(|r| r.rating == Rating::Miss).count();
    let rated_miss_healed = ratings
        .iter()
        .filter(|r| r.rating == Rating::Miss && resolved.contains(r.id.to_string().as_str()))
        .count();

    RecorderStats {
        searches: organic.len(),
        synthetic: searches.len() - organic.len(),
        unique_queries,
        zero_hit: organic.iter().filter(|s| s.result_count == 0).count(),
        accesses,
        latency_us_p50: pct(0.50),
        latency_us_p95: pct(0.95),
        latency_us_p99: pct(0.99),
        latency_us_max: latencies.last().copied().unwrap_or(0),
        oldest: organic.iter().map(|s| s.ts).min(),
        newest: organic.iter().map(|s| s.ts).max(),
        top_queries: top,
        rated: ratings.len(),
        rated_hit: ratings.iter().filter(|r| r.rating == Rating::Hit).count(),
        rated_partial: ratings
            .iter()
            .filter(|r| r.rating == Rating::Partial)
            .count(),
        rated_miss,
        rated_miss_outstanding: rated_miss - rated_miss_healed,
        rated_miss_healed,
        heals: resolutions.len(),
        heals_regressed: resolutions
            .iter()
            .filter(|r| r.last_replay.as_ref().is_some_and(|o| !o.held))
            .count(),
    }
}

/// Reads the recorder opt-out. Recording is on unless explicitly disabled.
pub fn recording_enabled() -> bool {
    !matches!(
        std::env::var("ECPHORY_SEARCH_LOG")
            .unwrap_or_default()
            .to_lowercase()
            .as_str(),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origin_parse_display_roundtrip() {
        for (raw, want) in [
            ("organic", SearchOrigin::Organic),
            ("", SearchOrigin::Organic),
            ("eval", SearchOrigin::Eval),
            ("backfill", SearchOrigin::Backfill),
            ("heal-replay", SearchOrigin::HealReplay),
            ("heal_replay", SearchOrigin::HealReplay),
            ("EVAL", SearchOrigin::Eval),
        ] {
            assert_eq!(
                raw.parse::<SearchOrigin>().unwrap(),
                want,
                "parsing {raw:?}"
            );
        }
        assert!("bogus".parse::<SearchOrigin>().is_err());
        assert_eq!(SearchOrigin::HealReplay.to_string(), "heal-replay");
        assert_eq!(
            serde_json::to_string(&SearchOrigin::HealReplay).unwrap(),
            "\"heal-replay\""
        );
    }

    #[test]
    fn pre_origin_recorder_rows_deserialize_as_organic() {
        // A verbatim pre-0.4 search-log row: no origin key, and `source` is
        // the episode-source FILTER — it must never be misread as origin.
        let raw = r#"{
            "id": "01980000-0000-7000-8000-000000000001",
            "ts": "2026-07-13T12:00:00Z",
            "query": "gavel daemon",
            "limit": 10,
            "include_deleted": false,
            "source": "claude-code",
            "result_count": 1,
            "results": [{"id": "01980000-0000-7000-8000-000000000002", "rank": 1, "score": 3.2}],
            "latency_us": 400
        }"#;
        let entry: SearchLogEntry = serde_json::from_str(raw).unwrap();
        assert_eq!(entry.origin, SearchOrigin::Organic);
        assert_eq!(entry.source.as_deref(), Some("claude-code"));

        // Organic stays implicit on the wire (old readers keep working);
        // tagged origins serialize explicitly.
        let json = serde_json::to_value(&entry).unwrap();
        assert!(json.get("origin").is_none());
        let mut tagged = entry;
        tagged.origin = SearchOrigin::Eval;
        let json = serde_json::to_value(&tagged).unwrap();
        assert_eq!(json["origin"], "eval");
    }

    #[test]
    fn pre_heal_correction_rows_deserialize() {
        // A PR #7 correction row predating displaced_used.
        let raw = r#"{
            "episode_id": "01980000-0000-7000-8000-000000000003",
            "action": "enriched",
            "after_rank": 1
        }"#;
        let c: Correction = serde_json::from_str(raw).unwrap();
        assert_eq!(c.action, CorrectionAction::Enriched);
        assert!(c.displaced_used.is_empty());
    }

    #[test]
    fn stats_split_misses_by_resolution_state() {
        let ts = Utc::now();
        let mk_rating = |id: Uuid, rating| RatingLogEntry {
            id,
            ts,
            search_id: "s".into(),
            rating,
            used_episode_ids: vec![],
            intended_episode_ids: vec![],
            corrections: vec![],
            note: None,
        };
        let healed_id = Uuid::now_v7();
        let ratings = vec![
            mk_rating(healed_id, Rating::Miss),
            mk_rating(Uuid::now_v7(), Rating::Miss),
            mk_rating(Uuid::now_v7(), Rating::Hit),
        ];
        let resolutions = vec![ResolutionLogEntry {
            id: Uuid::now_v7(),
            ts,
            rating_id: healed_id.to_string(),
            search_id: "s".into(),
            query: "q".into(),
            episode_id: "e".into(),
            validated_rank: 1,
            top_k: vec![],
            displaced_used: vec![],
            last_replay: Some(ReplayOutcome {
                ts,
                rank: None,
                held: false,
                displaced_used: vec![],
            }),
        }];

        let s = compute_stats(&[], 0, &ratings, &resolutions);
        assert_eq!(s.rated_miss, 2);
        assert_eq!(s.rated_miss_healed, 1);
        assert_eq!(s.rated_miss_outstanding, 1);
        assert_eq!(s.heals, 1);
        assert_eq!(s.heals_regressed, 1);
    }
}
