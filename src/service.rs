use std::path::{Path, PathBuf};
use std::time::Instant;

use crate::error::Result;
use crate::index::{Hit, SearchIndex};
use crate::model::{Episode, UpdateParams};
use crate::recorder::{self, AccessLogEntry, LoggedHit, RecorderStats, SearchLogEntry};
use crate::store::{ListOptions, Store};

/// Store + index, kept in sync. All writes go through here so the index can
/// never silently drift from the source of truth (and if it ever does,
/// `reindex` rebuilds it wholesale).
pub struct Ecphory {
    store: Store,
    index: SearchIndex,
    recording: bool,
}

#[derive(Debug, Default, Clone)]
pub struct SearchOptions {
    pub limit: usize,
    pub include_deleted: bool,
    pub group_id: Option<String>,
    pub source: Option<String>,
    pub tags: Vec<String>,
}

#[derive(Debug)]
pub struct SearchResult {
    pub episode: Episode,
    pub score: f32,
    pub rank: usize,
}

#[derive(Debug)]
pub struct SearchOutcome {
    pub results: Vec<SearchResult>,
    pub latency_us: u128,
}

impl Ecphory {
    /// The index lives beside the database file: <db_dir>/index/.
    pub fn open(db_path: impl AsRef<Path>) -> Result<Self> {
        Self::open_with(db_path, recorder::recording_enabled())
    }

    /// Open with an explicit recorder switch (tests; embedders that manage
    /// their own config). `open` reads ECPHORY_SEARCH_LOG.
    pub fn open_with(db_path: impl AsRef<Path>, recording: bool) -> Result<Self> {
        let store = Store::open(&db_path)?;
        let index_dir: PathBuf = db_path
            .as_ref()
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("index");
        let mut index = SearchIndex::open(index_dir)?;

        // Cold-start convergence: an empty index with a non-empty store means
        // the index dir is new (or was deleted for recovery) — rebuild now so
        // search is never silently empty. (FTS warm-start lesson from engram.)
        if store.count()? > 0 && index.search("*", 1, true)?.is_empty() {
            index.rebuild(store.list(ListOptions { include_deleted: true, include_expired: true, limit: 0 })?.iter())?;
        }

        // Recorder maintenance at open: prune past retention. Never fatal.
        let cutoff = chrono::Utc::now() - chrono::Duration::days(recorder::retention_days());
        if let Err(e) = store.prune_logs(cutoff) {
            tracing::warn!("recorder prune failed: {e}");
        }

        Ok(Self { store, index, recording })
    }

    pub fn insert(&mut self, ep: &Episode) -> Result<()> {
        self.store.insert(ep)?;
        self.index.upsert(ep)?;
        self.index.commit()
    }

    /// Bulk insert with one index commit at the end. Returns count inserted
    /// (existing ids are skipped — idempotent import).
    pub fn import(&mut self, episodes: Vec<Episode>) -> Result<usize> {
        let mut inserted = 0;
        for ep in episodes {
            if self.store.get(&ep.id.to_string()).is_ok() {
                continue;
            }
            self.store.insert(&ep)?;
            self.index.upsert(&ep)?;
            inserted += 1;
        }
        self.index.commit()?;
        Ok(inserted)
    }

    /// Fetch by id/prefix — the used-signal. Logs the CANONICAL id (post
    /// prefix-resolution) so the search-log join never misses on notation.
    pub fn get(&self, id: &str) -> Result<Episode> {
        let ep = self.store.get(id)?;
        if self.recording {
            self.store.log_access(&AccessLogEntry {
                id: uuid::Uuid::now_v7(),
                ts: chrono::Utc::now(),
                episode_id: ep.id.to_string(),
            });
        }
        Ok(ep)
    }

    /// Fetch without touching the access log — for bulk maintenance
    /// (enrichment backfills, sync jobs) whose reads are not usage signal.
    /// The 691-read backfill pollution taught this lesson.
    pub fn get_unrecorded(&self, id: &str) -> Result<Episode> {
        self.store.get(id)
    }

    /// Every episode, including demoted and expired — the export set.
    pub fn export_all(&self) -> Result<Vec<Episode>> {
        self.store.list(ListOptions { include_deleted: true, include_expired: true, limit: 0 })
    }

    pub fn list(&self, opts: ListOptions) -> Result<Vec<Episode>> {
        self.store.list(opts)
    }

    pub fn update(&mut self, id: &str, params: UpdateParams) -> Result<Episode> {
        let ep = self.store.update(id, params)?;
        self.index.upsert(&ep)?;
        self.index.commit()?;
        Ok(ep)
    }

    pub fn demote(&mut self, id: &str) -> Result<Episode> {
        let ep = self.store.demote(id)?;
        self.index.upsert(&ep)?;
        self.index.commit()?;
        Ok(ep)
    }

    pub fn restore(&mut self, id: &str) -> Result<Episode> {
        let ep = self.store.restore(id)?;
        self.index.upsert(&ep)?;
        self.index.commit()?;
        Ok(ep)
    }

    pub fn versions(&self, id: &str) -> Result<Vec<crate::model::EpisodeVersion>> {
        self.store.versions(id)
    }

    pub fn count(&self) -> Result<u64> {
        self.store.count()
    }

    pub fn reindex(&mut self) -> Result<usize> {
        let all = self.store.list(ListOptions {
            include_deleted: true,
            include_expired: true,
            limit: 0,
        })?;
        self.index.rebuild(all.iter())
    }

    /// BM25 search joined back to the store, with post-filters. Overfetches
    /// from the index (4x) so filters don't starve the requested limit — at
    /// personal-corpus scale the overfetch cost is noise.
    pub fn search(&self, query: &str, opts: &SearchOptions) -> Result<SearchOutcome> {
        let started = Instant::now();
        let limit = if opts.limit == 0 { 10 } else { opts.limit };
        let overfetch = (limit * 4).max(50);

        let hits: Vec<Hit> = self.index.search(query, overfetch, opts.include_deleted)?;

        let now = chrono::Utc::now();
        let mut results = Vec::with_capacity(limit);
        for hit in hits {
            let ep = match self.store.get(&hit.id) {
                Ok(ep) => ep,
                // Index/store drift (e.g. crash between store write and index
                // commit): skip rather than fail; reindex is the repair.
                Err(_) => continue,
            };
            if !opts.include_deleted && ep.is_deleted() {
                continue;
            }
            if ep.expired_at.is_some_and(|t| t <= now) {
                continue;
            }
            if opts.group_id.as_ref().is_some_and(|g| &ep.group_id != g) {
                continue;
            }
            if opts.source.as_ref().is_some_and(|s| &ep.source != s) {
                continue;
            }
            if !opts.tags.iter().all(|t| ep.tags.contains(t)) {
                continue;
            }
            let rank = results.len() + 1;
            results.push(SearchResult { episode: ep, score: hit.score, rank });
            if results.len() >= limit {
                break;
            }
        }

        let outcome = SearchOutcome { results, latency_us: started.elapsed().as_micros() };

        // Record the search. Empty queries are browses, not retrieval events;
        // logging them would drown the workload signal.
        if self.recording && !query.trim().is_empty() {
            self.store.log_search(&SearchLogEntry {
                id: uuid::Uuid::now_v7(),
                ts: chrono::Utc::now(),
                query: query.to_string(),
                limit,
                include_deleted: opts.include_deleted,
                group_id: opts.group_id.clone(),
                source: opts.source.clone(),
                tags: opts.tags.clone(),
                result_count: outcome.results.len(),
                results: outcome
                    .results
                    .iter()
                    .map(|r| LoggedHit {
                        id: r.episode.id.to_string(),
                        rank: r.rank,
                        score: r.score,
                    })
                    .collect(),
                latency_us: outcome.latency_us as u64,
            });
        }

        Ok(outcome)
    }

    pub fn recent_searches(&self, limit: usize) -> Result<Vec<SearchLogEntry>> {
        self.store.recent_searches(limit)
    }

    pub fn recent_accesses(&self, limit: usize) -> Result<Vec<AccessLogEntry>> {
        self.store.recent_accesses(limit)
    }

    pub fn stats(&self) -> Result<RecorderStats> {
        let searches = self.store.recent_searches(0)?;
        let accesses = self.store.recent_accesses(0)?;
        Ok(recorder::compute_stats(&searches, accesses.len()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp() -> (Ecphory, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("tempdir");
        let svc = Ecphory::open(dir.path().join("test.redb")).expect("open");
        (svc, dir)
    }

    fn ep(content: &str) -> Episode {
        Episode::new(content, "test")
    }

    #[test]
    fn search_finds_by_content() {
        let (mut svc, _d) = temp();
        svc.insert(&ep("the gavel daemon approves bash commands")).unwrap();
        svc.insert(&ep("duckdb stores episodes in a single file")).unwrap();

        let out = svc.search("duckdb single file", &SearchOptions::default()).unwrap();
        assert_eq!(out.results.len(), 1);
        assert!(out.results[0].episode.content.contains("duckdb"));
        assert!(out.latency_us > 0);
    }

    #[test]
    fn search_phrases_outrank_incidental_content() {
        let (mut svc, _d) = temp();
        // Body mentions the words; no phrase.
        let mut incidental = ep(
            "notes about various timeouts: the nightly job, extraction, and other errors we saw",
        );
        incidental.name = Some("misc timeout notes".into());
        // Different vocabulary in body, but the phrase matches how you'd ask.
        let mut target = ep("VPN xfrm state mismatch on the Sophos tunnel caused silent drops");
        target.search_phrases =
            vec!["nightly data extraction job hangs and times out with no error".into()];
        svc.insert(&incidental).unwrap();
        svc.insert(&target).unwrap();

        let out = svc
            .search("nightly data extraction job times out no error", &SearchOptions::default())
            .unwrap();
        assert!(!out.results.is_empty());
        assert_eq!(
            out.results[0].episode.id, target.id,
            "phrase-enriched episode should outrank incidental body match"
        );
    }

    #[test]
    fn stemming_matches_morphological_variants() {
        // First dogfood divergence: DuckDB FTS stems, tantivy default
        // didn't, so "finding memories" missed "finds the memory".
        let (mut svc, _d) = temp();
        svc.insert(&ep("the session finds the memory quickly and saves it")).unwrap();
        let out = svc.search("finding saved memories", &SearchOptions::default()).unwrap();
        assert_eq!(out.results.len(), 1, "stemmed variants should match");
    }

    #[test]
    fn numeric_tokens_are_searchable() {
        // engram needed an ILIKE fallback because DuckDB FTS can't index pure
        // numeric tokens; tantivy must not share that gap.
        let (mut svc, _d) = temp();
        svc.insert(&ep("AWS account 842478712031 is the org management survivor")).unwrap();
        let out = svc.search("842478712031", &SearchOptions::default()).unwrap();
        assert_eq!(out.results.len(), 1);
    }

    #[test]
    fn demoted_excluded_until_included() {
        let (mut svc, _d) = temp();
        let e = ep("demoted searchable episode about zebras");
        svc.insert(&e).unwrap();
        svc.demote(&e.id.to_string()).unwrap();

        let out = svc.search("zebras", &SearchOptions::default()).unwrap();
        assert!(out.results.is_empty());

        let out = svc
            .search("zebras", &SearchOptions { include_deleted: true, ..Default::default() })
            .unwrap();
        assert_eq!(out.results.len(), 1);
    }

    #[test]
    fn update_is_reflected_in_search() {
        let (mut svc, _d) = temp();
        let e = ep("original text about quokkas");
        svc.insert(&e).unwrap();
        svc.update(
            &e.id.to_string(),
            UpdateParams { content: Some("revised text about wombats".into()), ..Default::default() },
        )
        .unwrap();

        assert!(svc.search("quokkas", &SearchOptions::default()).unwrap().results.is_empty());
        assert_eq!(svc.search("wombats", &SearchOptions::default()).unwrap().results.len(), 1);
    }

    #[test]
    fn tag_and_source_filters() {
        let (mut svc, _d) = temp();
        let mut a = ep("kubernetes cluster upgrade runbook");
        a.tags = vec!["runbook".into()];
        let mut b = Episode::new("kubernetes cluster teardown story", "other-source");
        b.tags = vec!["story".into()];
        svc.insert(&a).unwrap();
        svc.insert(&b).unwrap();

        let out = svc
            .search(
                "kubernetes cluster",
                &SearchOptions { tags: vec!["runbook".into()], ..Default::default() },
            )
            .unwrap();
        assert_eq!(out.results.len(), 1);
        assert_eq!(out.results[0].episode.id, a.id);

        let out = svc
            .search(
                "kubernetes cluster",
                &SearchOptions { source: Some("other-source".into()), ..Default::default() },
            )
            .unwrap();
        assert_eq!(out.results.len(), 1);
        assert_eq!(out.results[0].episode.id, b.id);
    }

    #[test]
    fn recorder_logs_search_and_access() {
        let (mut svc, _d) = temp();
        let e = ep("recorder target about axolotls");
        svc.insert(&e).unwrap();

        let out = svc.search("axolotls", &SearchOptions::default()).unwrap();
        assert_eq!(out.results.len(), 1);
        let _ = svc.get(&e.id.to_string()[..13]).unwrap();

        let searches = svc.recent_searches(10).unwrap();
        assert_eq!(searches.len(), 1);
        let s = &searches[0];
        assert_eq!(s.query, "axolotls");
        assert_eq!(s.result_count, 1);
        assert_eq!(s.results[0].id, e.id.to_string());
        assert_eq!(s.results[0].rank, 1);
        assert!(s.latency_us > 0);

        // Access logged with the canonical id despite the prefix fetch.
        let accesses = svc.recent_accesses(10).unwrap();
        assert_eq!(accesses.len(), 1);
        assert_eq!(accesses[0].episode_id, e.id.to_string());

        // Joins inside search() must not pollute the access log.
        svc.search("axolotls", &SearchOptions::default()).unwrap();
        assert_eq!(svc.recent_accesses(10).unwrap().len(), 1);
    }

    #[test]
    fn recorder_skips_empty_query() {
        let (mut svc, _d) = temp();
        svc.insert(&ep("anything")).unwrap();
        svc.search("   ", &SearchOptions::default()).unwrap();
        assert!(svc.recent_searches(10).unwrap().is_empty());
    }

    #[test]
    fn recorder_opt_out() {
        let dir = tempfile::tempdir().unwrap();
        let mut svc = Ecphory::open_with(dir.path().join("optout.redb"), false).unwrap();

        let e = ep("silent running");
        svc.insert(&e).unwrap();
        svc.search("silent", &SearchOptions::default()).unwrap();
        svc.get(&e.id.to_string()).unwrap();
        assert!(svc.recent_searches(10).unwrap().is_empty());
        assert!(svc.recent_accesses(10).unwrap().is_empty());
    }

    #[test]
    fn recorder_stats_percentiles_and_zero_hits() {
        let (mut svc, _d) = temp();
        svc.insert(&ep("stats fodder about capybaras")).unwrap();
        for _ in 0..4 {
            svc.search("capybaras", &SearchOptions::default()).unwrap();
        }
        svc.search("no such thing anywhere", &SearchOptions::default()).unwrap();

        let s = svc.stats().unwrap();
        assert_eq!(s.searches, 5);
        assert_eq!(s.unique_queries, 2);
        assert_eq!(s.zero_hit, 1);
        assert!(s.latency_us_p50 > 0);
        assert!(s.latency_us_max >= s.latency_us_p99);
        assert_eq!(s.top_queries[0].0, "capybaras");
        assert_eq!(s.top_queries[0].1, 4);
    }

    #[test]
    fn reopen_rebuilds_missing_index() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("test.redb");
        let e = ep("persistent pelicans");
        {
            let mut svc = Ecphory::open(&db).unwrap();
            svc.insert(&e).unwrap();
        }
        // Simulate index loss (recovery path): delete the index dir entirely.
        std::fs::remove_dir_all(dir.path().join("index")).unwrap();
        let svc = Ecphory::open(&db).unwrap();
        let out = svc.search("pelicans", &SearchOptions::default()).unwrap();
        assert_eq!(out.results.len(), 1, "cold-start rebuild should restore search");
    }
}
