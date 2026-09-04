use std::path::{Path, PathBuf};
use std::time::Instant;

use crate::error::{Error, Result};
use crate::index::{Hit, SearchIndex};
use crate::model::{Episode, UpdateParams};
use crate::recorder::{
    self, AccessLogEntry, Correction, CorrectionAction, LoggedHit, Rating, RatingLogEntry,
    RecorderStats, ReplayOutcome, ResolutionLogEntry, SearchLogEntry, SearchOrigin,
};
use crate::store::{ListOptions, Store};

/// Validation window for self-correction: a target ranking within the top
/// CORRECTION_K counts as retrievable (matches the eval's default k).
const CORRECTION_K: usize = 5;

/// Phrase-count ceiling per episode. search_phrases are boosted 2x, so
/// unbounded miss-driven accumulation turns a much-missed episode into
/// lexical mass that crowds out its siblings.
const MAX_SEARCH_PHRASES: usize = 8;

/// Store + index, kept in sync. All writes go through here so the index can
/// never silently drift from the source of truth (and if it ever does,
/// `reindex` rebuilds it wholesale).
pub struct Ecphory {
    store: Store,
    index: SearchIndex,
    recording: bool,
    /// Post-write triggers (ECPHORY_TRIGGERS_FILE). Loaded at open so every
    /// entry point — MCP, HTTP, CLI — fires them; no per-caller wiring.
    triggers: Option<std::sync::Arc<crate::triggers::TriggerEngine>>,
    /// Groups an unscoped search skips (ECPHORY_HIDDEN_GROUPS). Naming the
    /// group in SearchOptions opts back in: hidden is "ask for it", never
    /// "invisible". Exists because a group used as a structured store (todos)
    /// is short, imperative and keyword-dense, so it outranks real memories
    /// on queries it has nothing to do with.
    hidden_groups: Vec<String>,
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

/// Everything a purge of one episode would destroy. Built read-only: the
/// dry-run path assembles these and must mutate nothing, not even the
/// access log (manifest reads are maintenance, not usage signal).
#[derive(Debug)]
pub struct PurgeManifest {
    pub episode: Episode,
    pub version_count: usize,
    pub indexed: bool,
}

#[derive(Debug)]
pub struct SearchOutcome {
    pub results: Vec<SearchResult>,
    pub latency_us: u128,
    /// Recorder entry id for this search, when it was recorded. The handle
    /// a consumer passes to rate_search to log its verdict.
    pub search_id: Option<uuid::Uuid>,
}

/// One heal re-verified by a replay pass.
#[derive(Debug, serde::Serialize)]
pub struct HealReplayEntry {
    pub resolution_id: String,
    pub rating_id: String,
    pub query: String,
    pub episode_id: String,
    /// Rank at validation time — the baseline this replay is held against.
    pub validated_rank: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rank: Option<usize>,
    pub held: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub displaced_used: Vec<String>,
}

/// Heal-replay regression pass over every recorded resolution.
#[derive(Debug, serde::Serialize)]
pub struct HealReplayReport {
    pub total: usize,
    pub held: usize,
    pub regressed: usize,
    pub k: usize,
    pub entries: Vec<HealReplayEntry>,
}

/// ECPHORY_HIDDEN_GROUPS: comma-separated, trimmed, empties dropped. Read
/// once at open so MCP, REST and CLI all agree on what is hidden.
fn hidden_groups_from_env() -> Vec<String> {
    std::env::var("ECPHORY_HIDDEN_GROUPS")
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
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
            index.rebuild(
                store
                    .list(ListOptions {
                        include_deleted: true,
                        include_expired: true,
                        limit: 0,
                    })?
                    .iter(),
            )?;
        }

        // Recorder maintenance at open: prune past retention. Never fatal.
        let cutoff = chrono::Utc::now() - chrono::Duration::days(recorder::retention_days());
        if let Err(e) = store.prune_logs(cutoff) {
            tracing::warn!("recorder prune failed: {e}");
        }

        // A malformed triggers file disables the feature loudly rather than
        // failing open: memory availability outranks a derived artifact, and
        // a crash-looping launchd service would take every agent down with it.
        let triggers = match crate::triggers::TriggerEngine::from_env() {
            Ok(engine) => engine.map(std::sync::Arc::new),
            Err(e) => {
                tracing::error!("triggers DISABLED: {e}");
                None
            }
        };

        Ok(Self {
            store,
            index,
            recording,
            triggers,
            hidden_groups: hidden_groups_from_env(),
        })
    }

    /// Replace the hidden-group set — tests only, so they need no env vars
    /// (process-global and racy under the parallel test runner).
    #[cfg(test)]
    pub(crate) fn with_hidden_groups(mut self, groups: Vec<String>) -> Self {
        self.hidden_groups = groups;
        self
    }

    /// The groups an unscoped search skips.
    pub fn hidden_groups(&self) -> &[String] {
        &self.hidden_groups
    }

    /// Never fails the write that fired it — triggers are observers.
    fn fire_trigger(&self, event: &'static str, ep: &Episode) {
        if let Some(engine) = &self.triggers {
            engine.fire(event, &ep.id.to_string(), &ep.tags);
        }
    }

    /// Inject an engine directly — tests only, so they need no env vars
    /// (process-global and racy under the parallel test runner).
    #[cfg(test)]
    pub(crate) fn set_triggers_for_test(&mut self, engine: crate::triggers::TriggerEngine) {
        self.triggers = Some(std::sync::Arc::new(engine));
    }

    /// Handle for firing store-wide trigger events (e.g. `export`) from
    /// callers that operate outside episode writes.
    pub fn triggers_engine(&self) -> Option<std::sync::Arc<crate::triggers::TriggerEngine>> {
        self.triggers.clone()
    }

    pub fn insert(&mut self, ep: &Episode) -> Result<()> {
        self.store.insert(ep)?;
        self.index.upsert(ep)?;
        self.index.commit()?;
        self.fire_trigger("insert", ep);
        Ok(())
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
        self.store.list(ListOptions {
            include_deleted: true,
            include_expired: true,
            limit: 0,
        })
    }

    pub fn list(&self, opts: ListOptions) -> Result<Vec<Episode>> {
        self.store.list(opts)
    }

    pub fn update(&mut self, id: &str, params: UpdateParams) -> Result<Episode> {
        let ep = self.store.update(id, params)?;
        self.index.upsert(&ep)?;
        self.index.commit()?;
        self.fire_trigger("update", &ep);
        Ok(ep)
    }

    pub fn demote(&mut self, id: &str) -> Result<Episode> {
        let ep = self.store.demote(id)?;
        self.index.upsert(&ep)?;
        self.index.commit()?;
        self.fire_trigger("demote", &ep);
        Ok(ep)
    }

    pub fn restore(&mut self, id: &str) -> Result<Episode> {
        let ep = self.store.restore(id)?;
        self.index.upsert(&ep)?;
        self.index.commit()?;
        self.fire_trigger("restore", &ep);
        Ok(ep)
    }

    pub fn restore_version(&mut self, id: &str, version_id: &str) -> Result<Episode> {
        let ep = self.store.restore_version(id, version_id)?;
        self.index.upsert(&ep)?;
        self.index.commit()?;
        self.fire_trigger("restore_version", &ep);
        Ok(ep)
    }

    pub fn versions(&self, id: &str) -> Result<Vec<crate::model::EpisodeVersion>> {
        self.store.versions(id)
    }

    /// Validate and describe one purge target without touching anything.
    /// Refuses non-demoted episodes here too, so a dry run reports exactly
    /// the same refusals an execute would.
    pub fn purge_manifest(&self, id: &str) -> Result<PurgeManifest> {
        let episode = self.store.get(id)?;
        if !episode.is_deleted() {
            return Err(Error::NotDemoted(episode.id.to_string()));
        }
        let canonical = episode.id.to_string();
        let version_count = self.store.versions(&canonical)?.len();
        let indexed = self.index.contains(&canonical)?;
        Ok(PurgeManifest {
            episode,
            version_count,
            indexed,
        })
    }

    /// Hard-delete a demoted episode: store record + archived versions
    /// (one transaction), then its index document. Store first — it is the
    /// source of truth; a crash in between leaves a dangling index doc that
    /// search already tolerates (the store join skips it) and reindex repairs.
    /// Returns the purged episode and the number of versions destroyed.
    pub fn purge(&mut self, id: &str) -> Result<(Episode, usize)> {
        let (episode, versions_removed) = self.store.purge(id)?;
        self.index.remove(&episode.id.to_string())?;
        self.index.commit()?;
        Ok((episode, versions_removed))
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
        self.search_impl(query, opts, Some(SearchOrigin::Organic))
    }

    /// Search without touching the recorder — for internal probes and jobs
    /// that shouldn't appear on the tape at all. The burst-polluted tape of
    /// 2026-07-13 (978/1000 entries synthetic) taught this.
    pub fn search_unrecorded(&self, query: &str, opts: &SearchOptions) -> Result<SearchOutcome> {
        self.search_impl(query, opts, None)
    }

    /// Search recorded under an explicit origin tag — the generalized
    /// recorder bypass. Synthetic jobs that want tape visibility (eval
    /// replays, backfills, heal replays) tag themselves here; workload
    /// aggregates and top-queries only count organic entries.
    pub fn search_tagged(
        &self,
        query: &str,
        opts: &SearchOptions,
        origin: SearchOrigin,
    ) -> Result<SearchOutcome> {
        self.search_impl(query, opts, Some(origin))
    }

    fn search_impl(
        &self,
        query: &str,
        opts: &SearchOptions,
        origin: Option<SearchOrigin>,
    ) -> Result<SearchOutcome> {
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
            // Hidden groups drop out of UNSCOPED searches only; naming the
            // group above is the opt-in.
            if opts.group_id.is_none() && self.hidden_groups.iter().any(|g| g == &ep.group_id) {
                continue;
            }
            if opts.source.as_ref().is_some_and(|s| &ep.source != s) {
                continue;
            }
            if !opts.tags.iter().all(|t| ep.tags.contains(t)) {
                continue;
            }
            let rank = results.len() + 1;
            results.push(SearchResult {
                episode: ep,
                score: hit.score,
                rank,
            });
            if results.len() >= limit {
                break;
            }
        }

        let mut outcome = SearchOutcome {
            results,
            latency_us: started.elapsed().as_micros(),
            search_id: None,
        };

        // Record the search. Empty queries are browses, not retrieval events;
        // logging them would drown the workload signal.
        if let Some(origin) = origin
            && self.recording
            && !query.trim().is_empty()
        {
            let search_id = uuid::Uuid::now_v7();
            self.store.log_search(&SearchLogEntry {
                id: search_id,
                ts: chrono::Utc::now(),
                query: query.to_string(),
                limit,
                include_deleted: opts.include_deleted,
                group_id: opts.group_id.clone(),
                source: opts.source.clone(),
                tags: opts.tags.clone(),
                origin,
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
            outcome.search_id = Some(search_id);
        }

        Ok(outcome)
    }

    /// Record the consumer's verdict on a recorded search. The search_id
    /// must reference a real search-log entry — garbage ids are refused so
    /// the rating stream stays joinable.
    ///
    /// A non-hit rating with explicit intended targets triggers the
    /// self-correction loop per target: enrich → redo → validate. Used ids
    /// are consumption telemetry only and never mutate episodes.
    pub fn rate_search(
        &mut self,
        search_id: &str,
        rating: Rating,
        used_episode_ids: Vec<String>,
        intended_episode_ids: Vec<String>,
        note: Option<String>,
    ) -> Result<RatingLogEntry> {
        let search = self.store.get_search_entry(search_id)?;
        // The rating id exists before the corrections run so a validated
        // heal can reference it in its resolution record.
        let rating_id = uuid::Uuid::now_v7();

        let mut corrections = Vec::new();
        let mut resolutions: Vec<ResolutionLogEntry> = Vec::new();
        if rating != Rating::Hit {
            let mut targets: Vec<String> = Vec::new();
            for target in &intended_episode_ids {
                if !targets.contains(target) {
                    targets.push(target.clone());
                }
            }
            let protected = self.protected_episode_ids()?;
            for target in &targets {
                let (correction, top_k) = self.self_correct(&search.query, target, &protected);
                // A miss whose target verifiably ranks within k is resolved:
                // Enriched is a validated heal; AlreadyRanks means the gap
                // closed by other means (stale rating) — either way there is
                // nothing outstanding, and the (query, target) pair becomes a
                // permanent replay regression test. The rating itself is
                // never touched: the miss stays ground truth for first-
                // contact failure rate.
                if rating == Rating::Miss
                    && matches!(
                        correction.action,
                        CorrectionAction::Enriched | CorrectionAction::AlreadyRanks
                    )
                    && let Some(validated_rank) = correction.after_rank
                {
                    resolutions.push(ResolutionLogEntry {
                        id: uuid::Uuid::now_v7(),
                        ts: chrono::Utc::now(),
                        rating_id: rating_id.to_string(),
                        search_id: search.id.to_string(),
                        query: search.query.clone(),
                        episode_id: correction.episode_id.clone(),
                        validated_rank,
                        top_k: top_k.unwrap_or_default(),
                        displaced_used: correction.displaced_used.clone(),
                        last_replay: None,
                    });
                }
                corrections.push(correction);
            }
        }

        let entry = RatingLogEntry {
            id: rating_id,
            ts: chrono::Utc::now(),
            search_id: search.id.to_string(),
            rating,
            used_episode_ids,
            intended_episode_ids,
            corrections,
            note,
        };
        self.store.log_rating(&entry)?;
        for resolution in &resolutions {
            self.store.log_resolution(resolution)?;
        }
        Ok(entry)
    }

    /// Enrich → redo → validate for one missed target. Deliberately NOT
    /// applied when the target already ranks (stale rating) or when the
    /// query is already a phrase (that miss is crowding or a bug — more
    /// lexical mass won't fix it and phrase inflation crowds siblings).
    ///
    /// Also returns the post-validation top-k ids (when the target ranks)
    /// so the caller can snapshot them on the resolution record, and runs
    /// the collateral-damage check: enrichment that displaces previously
    /// used/hit episodes out of the top k gets flagged on the correction —
    /// a warning, never a rollback.
    fn self_correct(
        &mut self,
        query: &str,
        id_or_prefix: &str,
        protected: &[String],
    ) -> (Correction, Option<Vec<String>>) {
        let mk = |id: &str, action, before, after| Correction {
            episode_id: id.to_string(),
            action,
            before_rank: before,
            after_rank: after,
            displaced_used: Vec::new(),
        };

        let ep = match self.store.get(id_or_prefix) {
            Ok(ep) => ep,
            Err(_) => {
                return (
                    mk(id_or_prefix, CorrectionAction::TargetNotFound, None, None),
                    None,
                );
            }
        };
        let id = ep.id.to_string();

        let before_ids = self.top_k_unrecorded(query);
        let before = rank_in(&before_ids, &id);
        if before.is_some() {
            return (
                mk(&id, CorrectionAction::AlreadyRanks, before, before),
                Some(before_ids),
            );
        }

        let normalized = query.trim().to_lowercase();
        if ep
            .search_phrases
            .iter()
            .any(|p| p.trim().to_lowercase() == normalized)
        {
            return (
                mk(&id, CorrectionAction::DuplicatePhrase, before, None),
                None,
            );
        }
        if ep.search_phrases.len() >= MAX_SEARCH_PHRASES {
            return (
                mk(&id, CorrectionAction::PhraseCapReached, before, None),
                None,
            );
        }

        let mut phrases = ep.search_phrases.clone();
        phrases.push(query.trim().to_string());
        if self
            .update(
                &id,
                UpdateParams {
                    search_phrases: Some(phrases),
                    ..Default::default()
                },
            )
            .is_err()
        {
            return (
                mk(&id, CorrectionAction::TargetNotFound, before, None),
                None,
            );
        }

        let after_ids = self.top_k_unrecorded(query);
        let after = rank_in(&after_ids, &id);
        let action = if after.is_some() {
            CorrectionAction::Enriched
        } else {
            CorrectionAction::EnrichedStillLow
        };
        let mut correction = mk(&id, action, before, after);
        correction.displaced_used = displaced_protected(&before_ids, &after_ids, &id, protected);
        if !correction.displaced_used.is_empty() {
            tracing::warn!(
                "heal collateral: enriching {} for {query:?} displaced previously used \
                 episodes out of top {CORRECTION_K}: {:?}",
                &id[..8],
                correction.displaced_used
            );
        }
        (correction, after.is_some().then_some(after_ids))
    }

    /// Top-CORRECTION_K episode ids for `query`, unrecorded so correction
    /// probes never pollute the workload tape.
    fn top_k_unrecorded(&self, query: &str) -> Vec<String> {
        let opts = SearchOptions {
            limit: CORRECTION_K,
            ..Default::default()
        };
        match self.search_unrecorded(query, &opts) {
            Ok(out) => out
                .results
                .iter()
                .map(|r| r.episode.id.to_string())
                .collect(),
            Err(_) => Vec::new(),
        }
    }

    /// Episodes with proven consumer value: everything prior ratings marked
    /// as used, plus targets of validated heals. Displacing one of these out
    /// of a top k is the collateral the heal lifecycle warns about. Entries
    /// may be prefixes (used_episode_ids accepts them), so matching is
    /// starts_with, same as everywhere else ids travel.
    fn protected_episode_ids(&self) -> Result<Vec<String>> {
        let mut protected: Vec<String> = Vec::new();
        for rating in self.store.recent_ratings(0)? {
            protected.extend(rating.used_episode_ids);
        }
        for resolution in self.store.recent_resolutions(0)? {
            protected.push(resolution.episode_id);
        }
        protected.sort();
        protected.dedup();
        Ok(protected)
    }

    /// Re-run every heal's original query against the live index and verify
    /// the intended episode still ranks within top k — the healed-miss
    /// regression pass. Each resolution's last_replay is updated in place
    /// (the resolution facts themselves stay write-once), so a regressed
    /// heal shows up in stats without re-searching at status time. Replay
    /// searches are taped under the heal-replay origin: visible, never
    /// counted as workload.
    pub fn replay_heals(&self, k: usize) -> Result<HealReplayReport> {
        let k = if k == 0 { CORRECTION_K } else { k };
        let resolutions = self.store.recent_resolutions(0)?;
        let protected = self.protected_episode_ids()?;

        let mut entries = Vec::with_capacity(resolutions.len());
        for mut resolution in resolutions {
            let opts = SearchOptions {
                limit: k,
                ..Default::default()
            };
            let out = self.search_tagged(&resolution.query, &opts, SearchOrigin::HealReplay)?;
            let ids: Vec<String> = out
                .results
                .iter()
                .map(|r| r.episode.id.to_string())
                .collect();
            let rank = rank_in(&ids, &resolution.episode_id);
            let held = rank.is_some();
            // Collateral on replay: current top k vs the as-healed snapshot.
            let displaced_used =
                displaced_protected(&resolution.top_k, &ids, &resolution.episode_id, &protected);
            if !held {
                tracing::warn!(
                    "heal regressed: {:?} no longer ranks {} in top {k}",
                    resolution.query,
                    &resolution.episode_id[..8]
                );
            }
            if !displaced_used.is_empty() {
                tracing::warn!(
                    "heal-replay collateral: {:?} lost previously used episodes from \
                     top {k}: {displaced_used:?}",
                    resolution.query
                );
            }

            resolution.last_replay = Some(ReplayOutcome {
                ts: chrono::Utc::now(),
                rank,
                held,
                displaced_used: displaced_used.clone(),
            });
            self.store.log_resolution(&resolution)?;

            entries.push(HealReplayEntry {
                resolution_id: resolution.id.to_string(),
                rating_id: resolution.rating_id,
                query: resolution.query,
                episode_id: resolution.episode_id,
                validated_rank: resolution.validated_rank,
                rank,
                held,
                displaced_used,
            });
        }

        let held = entries.iter().filter(|e| e.held).count();
        Ok(HealReplayReport {
            total: entries.len(),
            held,
            regressed: entries.len() - held,
            k,
            entries,
        })
    }

    pub fn recent_searches(&self, limit: usize) -> Result<Vec<SearchLogEntry>> {
        self.store.recent_searches(limit)
    }

    pub fn recent_accesses(&self, limit: usize) -> Result<Vec<AccessLogEntry>> {
        self.store.recent_accesses(limit)
    }

    pub fn recent_ratings(&self, limit: usize) -> Result<Vec<RatingLogEntry>> {
        self.store.recent_ratings(limit)
    }

    pub fn recent_resolutions(&self, limit: usize) -> Result<Vec<ResolutionLogEntry>> {
        self.store.recent_resolutions(limit)
    }

    pub fn stats(&self) -> Result<RecorderStats> {
        let searches = self.store.recent_searches(0)?;
        let accesses = self.store.recent_accesses(0)?;
        let ratings = self.store.recent_ratings(0)?;
        let resolutions = self.store.recent_resolutions(0)?;
        Ok(recorder::compute_stats(
            &searches,
            accesses.len(),
            &ratings,
            &resolutions,
        ))
    }
}

/// Rank (1-based) of `id` within an ordered id list; None = not present.
fn rank_in(ids: &[String], id: &str) -> Option<usize> {
    ids.iter().position(|i| i == id).map(|p| p + 1)
}

/// The collateral-damage diff: protected episodes present in `before` but
/// pushed out of `after` (the healed target itself excluded — it moving IN
/// is the point). `protected` entries may be id prefixes.
fn displaced_protected(
    before: &[String],
    after: &[String],
    target_id: &str,
    protected: &[String],
) -> Vec<String> {
    before
        .iter()
        .filter(|id| {
            id.as_str() != target_id
                && !after.contains(id)
                && protected.iter().any(|p| id.starts_with(p.as_str()))
        })
        .cloned()
        .collect()
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
        svc.insert(&ep("the gavel daemon approves bash commands"))
            .unwrap();
        svc.insert(&ep("duckdb stores episodes in a single file"))
            .unwrap();

        let out = svc
            .search("duckdb single file", &SearchOptions::default())
            .unwrap();
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
            .search(
                "nightly data extraction job times out no error",
                &SearchOptions::default(),
            )
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
        svc.insert(&ep("the session finds the memory quickly and saves it"))
            .unwrap();
        let out = svc
            .search("finding saved memories", &SearchOptions::default())
            .unwrap();
        assert_eq!(out.results.len(), 1, "stemmed variants should match");
    }

    #[test]
    fn numeric_tokens_are_searchable() {
        // engram needed an ILIKE fallback because DuckDB FTS can't index pure
        // numeric tokens; tantivy must not share that gap.
        let (mut svc, _d) = temp();
        svc.insert(&ep(
            "AWS account 842478712031 is the org management survivor",
        ))
        .unwrap();
        let out = svc
            .search("842478712031", &SearchOptions::default())
            .unwrap();
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
            .search(
                "zebras",
                &SearchOptions {
                    include_deleted: true,
                    ..Default::default()
                },
            )
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
            UpdateParams {
                content: Some("revised text about wombats".into()),
                ..Default::default()
            },
        )
        .unwrap();

        assert!(
            svc.search("quokkas", &SearchOptions::default())
                .unwrap()
                .results
                .is_empty()
        );
        assert_eq!(
            svc.search("wombats", &SearchOptions::default())
                .unwrap()
                .results
                .len(),
            1
        );
    }

    #[test]
    fn hidden_group_excluded_unless_named() {
        let (svc, _d) = temp();
        let mut svc = svc.with_hidden_groups(vec!["todos".into()]);
        let mut visible = ep("contact the electrician about the panel");
        visible.group_id = "default".into();
        let mut hidden = ep("contact the electrician about the panel");
        hidden.group_id = "todos".into();
        svc.insert(&visible).unwrap();
        svc.insert(&hidden).unwrap();

        // Unscoped: the hidden group must not appear, however well it scores.
        let out = svc
            .search("contact electrician", &SearchOptions::default())
            .unwrap();
        let ids: Vec<_> = out.results.iter().map(|r| r.episode.id).collect();
        assert_eq!(
            ids,
            vec![visible.id],
            "unscoped search leaked a hidden group"
        );

        // Naming the group opts back in.
        let out = svc
            .search(
                "contact electrician",
                &SearchOptions {
                    group_id: Some("todos".into()),
                    ..Default::default()
                },
            )
            .unwrap();
        let ids: Vec<_> = out.results.iter().map(|r| r.episode.id).collect();
        assert_eq!(
            ids,
            vec![hidden.id],
            "scoped search must return the hidden group"
        );
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
                &SearchOptions {
                    tags: vec!["runbook".into()],
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(out.results.len(), 1);
        assert_eq!(out.results[0].episode.id, a.id);

        let out = svc
            .search(
                "kubernetes cluster",
                &SearchOptions {
                    source: Some("other-source".into()),
                    ..Default::default()
                },
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
    fn search_returns_search_id_and_rating_roundtrips() {
        let (mut svc, _d) = temp();
        let e = ep("rating target about narwhals");
        svc.insert(&e).unwrap();

        let out = svc.search("narwhals", &SearchOptions::default()).unwrap();
        let search_id = out
            .search_id
            .expect("recorded search must return search_id");

        let entry = svc
            .rate_search(
                &search_id.to_string(),
                Rating::Hit,
                vec![e.id.to_string()[..8].to_string()],
                vec![],
                Some("answered directly".into()),
            )
            .unwrap();
        assert_eq!(entry.search_id, search_id.to_string());
        assert_eq!(entry.rating, Rating::Hit);

        let ratings = svc.recent_ratings(10).unwrap();
        assert_eq!(ratings.len(), 1);
        assert_eq!(
            ratings[0].used_episode_ids,
            vec![e.id.to_string()[..8].to_string()]
        );

        // Garbage search_id is refused — the rating stream stays joinable.
        assert!(
            svc.rate_search("not-a-real-search", Rating::Miss, vec![], vec![], None)
                .is_err()
        );

        let s = svc.stats().unwrap();
        assert_eq!(s.rated, 1);
        assert_eq!(s.rated_hit, 1);
        assert_eq!(s.rated_miss, 0);
    }

    #[test]
    fn miss_with_intended_id_enriches_and_validates() {
        let (mut svc, _d) = temp();
        // Zero vocabulary overlap between query and target: guaranteed miss.
        let target = ep("circus animals marching through downtown streets");
        svc.insert(&target).unwrap();
        svc.insert(&ep("unrelated decoy about database indexes"))
            .unwrap();

        let out = svc
            .search("purple elephant parade", &SearchOptions::default())
            .unwrap();
        assert!(out.results.is_empty());
        let sid = out.search_id.unwrap().to_string();

        // Full id, not an 8-char prefix: sibling test episodes are created in
        // the same millisecond, and UUIDv7's timestamp prefix makes short
        // prefixes ambiguous between them.
        let entry = svc
            .rate_search(
                &sid,
                Rating::Miss,
                vec![],
                vec![target.id.to_string()],
                None,
            )
            .unwrap();
        assert_eq!(entry.corrections.len(), 1);
        let c = &entry.corrections[0];
        assert_eq!(c.action, CorrectionAction::Enriched);
        assert_eq!(c.before_rank, None);
        assert_eq!(c.after_rank, Some(1));

        // The enrichment is durable: the query is now a search phrase.
        let ep_after = svc.get_unrecorded(&target.id.to_string()).unwrap();
        assert!(
            ep_after
                .search_phrases
                .iter()
                .any(|p| p == "purple elephant parade")
        );
    }

    #[test]
    fn hit_rating_runs_no_corrections() {
        let (mut svc, _d) = temp();
        let e = ep("straightforward content about lighthouses");
        svc.insert(&e).unwrap();
        let out = svc
            .search("lighthouses", &SearchOptions::default())
            .unwrap();
        let sid = out.search_id.unwrap().to_string();
        let entry = svc
            .rate_search(&sid, Rating::Hit, vec![e.id.to_string()], vec![], None)
            .unwrap();
        assert!(entry.corrections.is_empty());
        assert!(
            svc.get_unrecorded(&e.id.to_string())
                .unwrap()
                .search_phrases
                .is_empty()
        );
    }

    #[test]
    fn used_episode_ids_never_trigger_self_correction() {
        let (mut svc, _d) = temp();
        let target = ep("submarine sonar calibration procedure");
        svc.insert(&target).unwrap();
        let out = svc
            .search("underwater ping tuning", &SearchOptions::default())
            .unwrap();
        assert!(out.results.is_empty());

        let entry = svc
            .rate_search(
                &out.search_id.unwrap().to_string(),
                Rating::Partial,
                vec![target.id.to_string()],
                vec![],
                None,
            )
            .unwrap();
        assert!(entry.corrections.is_empty());
        assert_eq!(entry.used_episode_ids, vec![target.id.to_string()]);
        assert!(
            svc.get_unrecorded(&target.id.to_string())
                .unwrap()
                .search_phrases
                .is_empty()
        );
    }

    #[test]
    fn already_ranking_target_is_left_alone() {
        let (mut svc, _d) = temp();
        let e = ep("alpha bravo charlie delta");
        svc.insert(&e).unwrap();
        let out = svc
            .search("alpha bravo", &SearchOptions::default())
            .unwrap();
        let sid = out.search_id.unwrap().to_string();
        // Partial rating (say, the answer needed a second episode too) must
        // not enrich a target that already ranks.
        let entry = svc
            .rate_search(&sid, Rating::Partial, vec![], vec![e.id.to_string()], None)
            .unwrap();
        assert_eq!(entry.corrections[0].action, CorrectionAction::AlreadyRanks);
        assert!(
            svc.get_unrecorded(&e.id.to_string())
                .unwrap()
                .search_phrases
                .is_empty()
        );
    }

    #[test]
    fn phrase_cap_blocks_enrichment() {
        let (mut svc, _d) = temp();
        let mut e = ep("content sharing nothing with the query vocabulary");
        e.search_phrases = (0..8)
            .map(|i| format!("existing phrase number {i}"))
            .collect();
        svc.insert(&e).unwrap();
        let out = svc
            .search("zebra quantum harmonica", &SearchOptions::default())
            .unwrap();
        let sid = out.search_id.unwrap().to_string();
        let entry = svc
            .rate_search(&sid, Rating::Miss, vec![], vec![e.id.to_string()], None)
            .unwrap();
        assert_eq!(
            entry.corrections[0].action,
            CorrectionAction::PhraseCapReached
        );
        assert_eq!(
            svc.get_unrecorded(&e.id.to_string())
                .unwrap()
                .search_phrases
                .len(),
            8
        );
    }

    #[test]
    fn unresolvable_target_is_reported_not_fatal() {
        let (mut svc, _d) = temp();
        svc.insert(&ep("anything at all")).unwrap();
        let out = svc
            .search("no such thing here", &SearchOptions::default())
            .unwrap();
        let sid = out.search_id.unwrap().to_string();
        let entry = svc
            .rate_search(
                &sid,
                Rating::Miss,
                vec![],
                vec!["ffffffff-0000".into()],
                None,
            )
            .unwrap();
        assert_eq!(
            entry.corrections[0].action,
            CorrectionAction::TargetNotFound
        );
    }

    #[test]
    fn unrecorded_search_leaves_no_tape_and_no_search_id() {
        let (mut svc, _d) = temp();
        svc.insert(&ep("stealth search fodder about ocelots"))
            .unwrap();

        let out = svc
            .search_unrecorded("ocelots", &SearchOptions::default())
            .unwrap();
        assert_eq!(out.results.len(), 1);
        assert!(out.search_id.is_none());
        assert!(svc.recent_searches(10).unwrap().is_empty());
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
        svc.search("no such thing anywhere", &SearchOptions::default())
            .unwrap();

        let s = svc.stats().unwrap();
        assert_eq!(s.searches, 5);
        assert_eq!(s.unique_queries, 2);
        assert_eq!(s.zero_hit, 1);
        assert!(s.latency_us_p50 > 0);
        assert!(s.latency_us_max >= s.latency_us_p99);
        assert_eq!(s.top_queries[0].0, "capybaras");
        assert_eq!(s.top_queries[0].1, 4);
    }

    // ---- heal lifecycle ------------------------------------------------------

    /// Rate the most recent search for `query` as a miss with an intended
    /// target — the heal trigger.
    fn rate_miss(svc: &mut Ecphory, query: &str, intended: &str) -> RatingLogEntry {
        let out = svc.search(query, &SearchOptions::default()).unwrap();
        let sid = out.search_id.unwrap().to_string();
        svc.rate_search(&sid, Rating::Miss, vec![], vec![intended.to_string()], None)
            .unwrap()
    }

    #[test]
    fn validated_heal_records_resolution_and_splits_status() {
        let (mut svc, _d) = temp();
        let target = ep("circus animals marching through downtown streets");
        svc.insert(&target).unwrap();

        let entry = rate_miss(&mut svc, "purple elephant parade", &target.id.to_string());
        assert_eq!(entry.corrections[0].action, CorrectionAction::Enriched);

        // The rating itself is untouched ground truth; the resolution is a
        // separate record carrying everything a replay needs.
        let resolutions = svc.recent_resolutions(10).unwrap();
        assert_eq!(resolutions.len(), 1);
        let r = &resolutions[0];
        assert_eq!(r.rating_id, entry.id.to_string());
        assert_eq!(r.search_id, entry.search_id);
        assert_eq!(r.query, "purple elephant parade");
        assert_eq!(r.episode_id, target.id.to_string());
        assert_eq!(r.validated_rank, 1);
        assert!(r.top_k.contains(&target.id.to_string()));
        assert!(r.last_replay.is_none());

        let s = svc.stats().unwrap();
        assert_eq!(s.rated_miss, 1);
        assert_eq!(s.rated_miss_healed, 1);
        assert_eq!(s.rated_miss_outstanding, 0);
        assert_eq!(s.heals, 1);
        assert_eq!(s.heals_regressed, 0);
    }

    #[test]
    fn unhealed_miss_stays_outstanding() {
        let (mut svc, _d) = temp();
        svc.insert(&ep("anything at all")).unwrap();

        // Miss with no intended target: nothing to heal.
        let out = svc
            .search("no such topic here", &SearchOptions::default())
            .unwrap();
        let sid = out.search_id.unwrap().to_string();
        svc.rate_search(&sid, Rating::Miss, vec![], vec![], None)
            .unwrap();

        assert!(svc.recent_resolutions(10).unwrap().is_empty());
        let s = svc.stats().unwrap();
        assert_eq!(s.rated_miss, 1);
        assert_eq!(s.rated_miss_healed, 0);
        assert_eq!(s.rated_miss_outstanding, 1);
    }

    #[test]
    fn already_ranking_miss_resolves_without_enrichment() {
        let (mut svc, _d) = temp();
        let e = ep("golf handicap scoring rules");
        svc.insert(&e).unwrap();

        // A stale miss: the target actually ranks. No phrase is added, but
        // the miss is not outstanding either — the gap is closed.
        let entry = rate_miss(&mut svc, "golf handicap", &e.id.to_string());
        assert_eq!(entry.corrections[0].action, CorrectionAction::AlreadyRanks);
        assert!(
            svc.get_unrecorded(&e.id.to_string())
                .unwrap()
                .search_phrases
                .is_empty()
        );

        let resolutions = svc.recent_resolutions(10).unwrap();
        assert_eq!(resolutions.len(), 1);
        assert_eq!(resolutions[0].validated_rank, 1);

        let s = svc.stats().unwrap();
        assert_eq!(s.rated_miss_healed, 1);
        assert_eq!(s.rated_miss_outstanding, 0);
    }

    #[test]
    fn failed_correction_leaves_miss_outstanding() {
        let (mut svc, _d) = temp();
        let mut e = ep("content sharing nothing with the query vocabulary");
        e.search_phrases = (0..8)
            .map(|i| format!("existing phrase number {i}"))
            .collect();
        svc.insert(&e).unwrap();

        let entry = rate_miss(&mut svc, "zebra quantum harmonica", &e.id.to_string());
        assert_eq!(
            entry.corrections[0].action,
            CorrectionAction::PhraseCapReached
        );

        assert!(svc.recent_resolutions(10).unwrap().is_empty());
        let s = svc.stats().unwrap();
        assert_eq!(s.rated_miss_outstanding, 1);
    }

    #[test]
    fn partial_heal_runs_corrections_but_records_no_resolution() {
        let (mut svc, _d) = temp();
        let target = ep("submarine sonar calibration procedure");
        svc.insert(&target).unwrap();

        let out = svc
            .search("underwater ping tuning", &SearchOptions::default())
            .unwrap();
        let sid = out.search_id.unwrap().to_string();
        let entry = svc
            .rate_search(
                &sid,
                Rating::Partial,
                vec![],
                vec![target.id.to_string()],
                None,
            )
            .unwrap();
        assert_eq!(entry.corrections[0].action, CorrectionAction::Enriched);

        // Resolutions track the miss lifecycle only — a partial was never
        // "outstanding", so there is nothing to mark healed.
        assert!(svc.recent_resolutions(10).unwrap().is_empty());
    }

    #[test]
    fn restore_version_updates_the_search_index() {
        let (mut svc, _d) = temp();
        let original = ep("oranges from the winter greenhouse");
        svc.insert(&original).unwrap();
        svc.update(
            &original.id.to_string(),
            UpdateParams {
                content: Some("bananas from the summer market".into()),
                ..Default::default()
            },
        )
        .unwrap();
        let version_id = svc.versions(&original.id.to_string()).unwrap()[0].version_id;

        svc.restore_version(&original.id.to_string(), &version_id.to_string())
            .unwrap();

        let oranges = svc
            .search_unrecorded("winter greenhouse", &SearchOptions::default())
            .unwrap();
        assert_eq!(oranges.results[0].episode.id, original.id);
        let bananas = svc
            .search_unrecorded("summer market", &SearchOptions::default())
            .unwrap();
        assert!(bananas.results.iter().all(|r| r.episode.id != original.id));
    }

    #[test]
    fn heal_replay_reports_held_and_stores_outcome() {
        let (mut svc, _d) = temp();
        let target = ep("circus animals marching through downtown streets");
        svc.insert(&target).unwrap();
        rate_miss(&mut svc, "purple elephant parade", &target.id.to_string());

        let report = svc.replay_heals(0).unwrap();
        assert_eq!(report.total, 1);
        assert_eq!(report.held, 1);
        assert_eq!(report.regressed, 0);
        assert_eq!(report.entries[0].rank, Some(1));
        assert!(report.entries[0].held);

        // The outcome is persisted on the resolution, so status sees it
        // without re-searching.
        let r = &svc.recent_resolutions(10).unwrap()[0];
        let replay = r.last_replay.as_ref().expect("replay outcome stored");
        assert!(replay.held);
        assert_eq!(replay.rank, Some(1));
        let s = svc.stats().unwrap();
        assert_eq!(s.heals, 1);
        assert_eq!(s.heals_regressed, 0);
    }

    #[test]
    fn heal_replay_flags_regression() {
        let (mut svc, _d) = temp();
        let target = ep("circus animals marching through downtown streets");
        svc.insert(&target).unwrap();
        rate_miss(&mut svc, "purple elephant parade", &target.id.to_string());

        // The heal held... until the target got demoted out of search.
        assert_eq!(svc.replay_heals(0).unwrap().regressed, 0);
        svc.demote(&target.id.to_string()).unwrap();

        let report = svc.replay_heals(0).unwrap();
        assert_eq!(report.total, 1);
        assert_eq!(report.held, 0);
        assert_eq!(report.regressed, 1);
        assert_eq!(report.entries[0].rank, None);

        let s = svc.stats().unwrap();
        assert_eq!(s.heals_regressed, 1);
        // The rating stream is untouched by replays: still one miss, healed.
        assert_eq!(s.rated_miss, 1);
        assert_eq!(s.rated_miss_healed, 1);
    }

    /// Six-episode displacement rig: d1..d4 and `crowded` all match the
    /// contested query, with growing padding so BM25 length normalization
    /// ranks `crowded` last (5th). Enriching a sixth episode into the top 5
    /// must push `crowded` out.
    fn displacement_rig(svc: &mut Ecphory) -> (Vec<Episode>, Episode, Episode) {
        let pad = [
            "",
            "with extra notes about unrelated calibration steps",
            "with extra notes about unrelated calibration steps and a long tail of \
             miscellaneous observations",
            "xenolith with extra notes about unrelated calibration steps and a long tail of \
             miscellaneous observations gathered over several sessions",
        ];
        let decoys: Vec<Episode> = pad
            .iter()
            .map(|p| ep(&format!("quartz crystal resonance {p}")))
            .collect();
        let crowded = ep(
            "quartz crystal resonance padparadscha with the longest padding of them all, \
             extra notes about unrelated calibration steps and a long tail of miscellaneous \
             observations gathered over several sessions plus appendices nobody reads",
        );
        let target = ep("piezoelectric oscillator drift measured on the bench meter");
        for e in decoys.iter().chain([&crowded, &target]) {
            svc.insert(e).unwrap();
        }
        (decoys, crowded, target)
    }

    #[test]
    fn heal_collateral_warns_when_used_episode_is_displaced() {
        let (mut svc, _d) = temp();
        let (_decoys, crowded, target) = displacement_rig(&mut svc);

        // Mark `crowded` as used — a prior rating proved its value.
        let out = svc
            .search("padparadscha", &SearchOptions::default())
            .unwrap();
        assert_eq!(out.results[0].episode.id, crowded.id);
        let sid = out.search_id.unwrap().to_string();
        svc.rate_search(
            &sid,
            Rating::Hit,
            vec![crowded.id.to_string()],
            vec![],
            None,
        )
        .unwrap();

        // Sanity: before the heal, `crowded` holds rank 5 for the query.
        let before = svc
            .search_unrecorded("quartz crystal resonance", &SearchOptions::default())
            .unwrap();
        assert_eq!(
            before.results[4].episode.id, crowded.id,
            "rig must place crowded at rank 5"
        );

        let entry = rate_miss(&mut svc, "quartz crystal resonance", &target.id.to_string());
        let c = &entry.corrections[0];
        assert_eq!(c.action, CorrectionAction::Enriched);
        assert_eq!(
            c.displaced_used,
            vec![crowded.id.to_string()],
            "the heal displaced a previously used episode out of top k — that must be flagged"
        );

        let r = &svc.recent_resolutions(10).unwrap()[0];
        assert_eq!(r.displaced_used, vec![crowded.id.to_string()]);
        assert!(!r.top_k.contains(&crowded.id.to_string()));
    }

    #[test]
    fn heal_collateral_ignores_unprotected_displacement() {
        let (mut svc, _d) = temp();
        let (_decoys, _crowded, target) = displacement_rig(&mut svc);

        // Same displacement, but nothing marked `crowded` as used — no
        // rating ever vouched for it, so its displacement is not collateral.
        let entry = rate_miss(&mut svc, "quartz crystal resonance", &target.id.to_string());
        let c = &entry.corrections[0];
        assert_eq!(c.action, CorrectionAction::Enriched);
        assert!(c.displaced_used.is_empty());
    }

    #[test]
    fn replay_collateral_warns_when_as_healed_topk_loses_used_episode() {
        let (mut svc, _d) = temp();
        let (decoys, _crowded, target) = displacement_rig(&mut svc);
        rate_miss(&mut svc, "quartz crystal resonance", &target.id.to_string());

        // d4 (in the as-healed top k) gets used, then vanishes from search.
        let d4 = &decoys[3];
        let out = svc.search("xenolith", &SearchOptions::default()).unwrap();
        assert_eq!(out.results[0].episode.id, d4.id);
        let sid = out.search_id.unwrap().to_string();
        svc.rate_search(&sid, Rating::Hit, vec![d4.id.to_string()], vec![], None)
            .unwrap();
        svc.demote(&d4.id.to_string()).unwrap();

        let report = svc.replay_heals(0).unwrap();
        let e = &report.entries[0];
        assert!(e.held, "the target itself still ranks");
        assert_eq!(
            e.displaced_used,
            vec![d4.id.to_string()],
            "a used episode fell out of the as-healed top k — warn"
        );
        let r = &svc.recent_resolutions(10).unwrap()[0];
        assert_eq!(
            r.last_replay.as_ref().unwrap().displaced_used,
            vec![d4.id.to_string()]
        );
    }

    // ---- source-tagged tape ---------------------------------------------------

    #[test]
    fn tagged_search_is_taped_with_origin_and_rateable() {
        let (mut svc, _d) = temp();
        let e = ep("tagged tape fodder about ospreys");
        svc.insert(&e).unwrap();

        let out = svc
            .search_tagged(
                "ospreys",
                &SearchOptions::default(),
                SearchOrigin::HealReplay,
            )
            .unwrap();
        assert!(out.search_id.is_some(), "tagged searches are on the tape");

        let searches = svc.recent_searches(10).unwrap();
        assert_eq!(searches.len(), 1);
        assert_eq!(searches[0].origin, SearchOrigin::HealReplay);
    }

    #[test]
    fn stats_count_organic_only_and_surface_synthetic() {
        let (mut svc, _d) = temp();
        svc.insert(&ep("stats fodder about capybaras")).unwrap();

        svc.search("capybaras", &SearchOptions::default()).unwrap();
        for _ in 0..3 {
            svc.search_tagged("capybaras", &SearchOptions::default(), SearchOrigin::Eval)
                .unwrap();
        }
        svc.search_tagged(
            "capybaras",
            &SearchOptions::default(),
            SearchOrigin::HealReplay,
        )
        .unwrap();

        let s = svc.stats().unwrap();
        assert_eq!(s.searches, 1, "only the organic search is workload");
        assert_eq!(s.synthetic, 4);
        assert_eq!(
            s.top_queries[0],
            ("capybaras".to_string(), 1),
            "tagged replays must not inflate top_queries"
        );
    }

    #[test]
    fn purge_manifest_refuses_non_demoted_and_destroys_nothing() {
        let (mut svc, _d) = temp();
        let e = ep("manifest fodder about ibexes");
        svc.insert(&e).unwrap();

        // Both the dry-run manifest and the execute path refuse a live episode.
        match svc.purge_manifest(&e.id.to_string()) {
            Err(Error::NotDemoted(id)) => assert_eq!(id, e.id.to_string()),
            other => panic!("expected NotDemoted, got {other:?}"),
        }
        assert!(matches!(
            svc.purge(&e.id.to_string()),
            Err(Error::NotDemoted(_))
        ));

        svc.demote(&e.id.to_string()).unwrap();
        let m = svc.purge_manifest(&e.id.to_string()).unwrap();
        assert_eq!(m.episode.id, e.id);
        assert_eq!(m.version_count, 1, "the demote archive");
        assert!(m.indexed);

        // The dry run destroyed nothing and left no usage signal.
        assert!(svc.get_unrecorded(&e.id.to_string()).is_ok());
        assert_eq!(svc.versions(&e.id.to_string()).unwrap().len(), 1);
        let out = svc
            .search(
                "ibexes",
                &SearchOptions {
                    include_deleted: true,
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(out.results.len(), 1);
        assert!(
            svc.recent_accesses(10).unwrap().is_empty(),
            "manifest reads must not pollute the access log"
        );
    }

    #[test]
    fn purge_removes_record_versions_and_index_docs() {
        let (mut svc, _d) = temp();
        let e = ep("purge target about dodos");
        svc.insert(&e).unwrap();
        svc.demote(&e.id.to_string()).unwrap();

        let (purged, versions_removed) = svc.purge(&e.id.to_string()).unwrap();
        assert_eq!(purged.id, e.id);
        assert_eq!(versions_removed, 1);

        assert!(svc.get_unrecorded(&e.id.to_string()).is_err());
        assert!(svc.versions(&e.id.to_string()).unwrap().is_empty());
        let out = svc
            .search(
                "dodos",
                &SearchOptions {
                    include_deleted: true,
                    ..Default::default()
                },
            )
            .unwrap();
        assert!(
            out.results.is_empty(),
            "purged episode must not surface even with include_deleted"
        );
        assert!(matches!(
            svc.purge_manifest(&e.id.to_string()),
            Err(Error::NotFound(_))
        ));
    }

    #[test]
    fn update_fires_matching_trigger() {
        let (mut svc, dir) = temp();
        let marker = dir.path().join("trigger-fired");
        let engine = crate::triggers::TriggerEngine::from_json(&format!(
            r#"{{"triggers": [{{"name": "render", "run": ["/usr/bin/touch", {marker:?}], "match": {{"tags_any": ["rendered-artifact"]}}}}]}}"#
        ))
        .unwrap();
        svc.set_triggers_for_test(engine);

        let mut e = ep("canonical source for a rendered file");
        e.tags = vec!["rendered-artifact".into()];
        svc.insert(&e).unwrap();
        // insert is not in the default event set — marker must not exist yet
        // (fire is async; the update below gives it ample time to be wrong).
        let updated = svc
            .update(
                &e.id.to_string(),
                UpdateParams {
                    content: Some("edited".into()),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(updated.content, "edited");

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !marker.exists() && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert!(marker.exists(), "update did not fire the trigger");
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
        assert_eq!(
            out.results.len(),
            1,
            "cold-start rebuild should restore search"
        );
    }
}
