use std::path::Path;

use chrono::Utc;
use redb::{Database, ReadableDatabase, ReadableTable, ReadableTableMetadata, TableDefinition};
use uuid::Uuid;

use crate::error::{Error, Result};
use crate::model::{Episode, EpisodeVersion, UpdateParams};

/// Canonical store. redb is the source of truth; every derived structure
/// (the tantivy index, later) must be rebuildable from these tables alone.
///
/// Keys are hyphenated lowercase UUID strings. UUIDv7 makes key order equal
/// creation order, and string keys make unique-prefix resolution a range scan.
const EPISODES: TableDefinition<&str, &[u8]> = TableDefinition::new("episodes");
/// Key: "{episode_id}/{archived_at_rfc3339}/{version_id}" — range-scannable
/// by episode, chronologically ordered within one.
const VERSIONS: TableDefinition<&str, &[u8]> = TableDefinition::new("episode_versions");
/// Flight recorder tables. Keys are UUIDv7 strings → time-ordered, so
/// retention pruning and newest-first reads are plain range scans.
const SEARCH_LOG: TableDefinition<&str, &[u8]> = TableDefinition::new("search_log");
const ACCESS_LOG: TableDefinition<&str, &[u8]> = TableDefinition::new("access_log");
/// Explicit consumer verdicts on searches (rate_search) — the preferred
/// quality signal; the access-log join is the fallback for unrated searches.
const RATING_LOG: TableDefinition<&str, &[u8]> = TableDefinition::new("rating_log");
/// Validated heals, keyed by resolution id. Deliberately NOT covered by the
/// recorder retention prune: every healed miss is a permanent regression
/// test (the entry carries its own query for exactly this reason).
const RESOLUTION_LOG: TableDefinition<&str, &[u8]> = TableDefinition::new("resolution_log");
/// Store-level scalars. Currently just `store_id` — the identity a derived
/// artifact (the tantivy index) records so it can tell whose it is. Path is
/// not identity: a store keeps its id when it is moved or renamed.
const META: TableDefinition<&str, &[u8]> = TableDefinition::new("meta");
const STORE_ID_KEY: &str = "store_id";

pub struct Store {
    db: Database,
    store_id: Uuid,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct ListOptions {
    pub include_deleted: bool,
    pub include_expired: bool,
    /// 0 means no limit.
    pub limit: usize,
}

impl Store {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        if let Some(parent) = path.as_ref().parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| Error::Storage(format!("creating {}: {e}", parent.display())))?;
        }
        let db = Database::create(path)?;
        // Create tables eagerly so first reads don't race first writes.
        let tx = db.begin_write()?;
        let store_id = {
            tx.open_table(EPISODES)?;
            tx.open_table(VERSIONS)?;
            tx.open_table(SEARCH_LOG)?;
            tx.open_table(ACCESS_LOG)?;
            tx.open_table(RATING_LOG)?;
            tx.open_table(RESOLUTION_LOG)?;
            let mut meta = tx.open_table(META)?;
            // Minted once, on the first open of a store — including an
            // existing store opened by a version that knows about ids.
            let existing = meta
                .get(STORE_ID_KEY)?
                .map(|guard| String::from_utf8_lossy(guard.value()).trim().to_string());
            match existing {
                Some(raw) => Uuid::parse_str(&raw)
                    .map_err(|e| Error::Storage(format!("bad store_id {raw}: {e}")))?,
                None => {
                    let id = Uuid::now_v7();
                    meta.insert(STORE_ID_KEY, id.to_string().as_bytes())?;
                    id
                }
            }
        };
        tx.commit()?;
        Ok(Self { db, store_id })
    }

    /// This store's durable id. Stable across opens, moves and renames.
    pub fn store_id(&self) -> Uuid {
        self.store_id
    }

    pub fn insert(&self, episode: &Episode) -> Result<()> {
        let key = episode.id.to_string();
        let value = serde_json::to_vec(episode)?;
        let tx = self.db.begin_write()?;
        {
            let mut table = tx.open_table(EPISODES)?;
            table.insert(key.as_str(), value.as_slice())?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Resolve a full UUID or a unique prefix to a canonical id.
    ///
    /// Resolution deliberately sees soft-deleted episodes too: an id
    /// reference in a handoff must never break because the episode was
    /// demoted (engram's get-returns-deleted asymmetry, kept).
    pub fn resolve_id(&self, id_or_prefix: &str) -> Result<Uuid> {
        let needle = id_or_prefix.trim().to_lowercase();
        if let Ok(id) = Uuid::parse_str(&needle) {
            return Ok(id);
        }
        if needle.is_empty() {
            return Err(Error::NotFound(id_or_prefix.to_string()));
        }

        let tx = self.db.begin_read()?;
        let table = tx.open_table(EPISODES)?;
        let mut matches: Vec<Uuid> = Vec::with_capacity(2);
        for entry in table.range(needle.as_str()..)? {
            let (key, _) = entry?;
            let key = key.value();
            if !key.starts_with(needle.as_str()) {
                break;
            }
            matches.push(
                Uuid::parse_str(key).map_err(|e| Error::Storage(format!("bad key {key}: {e}")))?,
            );
            if matches.len() > 1 {
                return Err(Error::AmbiguousPrefix(id_or_prefix.to_string()));
            }
        }
        matches
            .pop()
            .ok_or_else(|| Error::NotFound(id_or_prefix.to_string()))
    }

    /// Fetch by id or unique prefix. Returns soft-deleted episodes (with
    /// deleted_at set) — see resolve_id for why.
    pub fn get(&self, id_or_prefix: &str) -> Result<Episode> {
        let id = self.resolve_id(id_or_prefix)?;
        let tx = self.db.begin_read()?;
        let table = tx.open_table(EPISODES)?;
        let key = id.to_string();
        match table.get(key.as_str())? {
            Some(guard) => Ok(serde_json::from_slice(guard.value())?),
            None => Err(Error::NotFound(id_or_prefix.to_string())),
        }
    }

    /// List episodes, newest first (UUIDv7 key order reversed).
    pub fn list(&self, opts: ListOptions) -> Result<Vec<Episode>> {
        let now = Utc::now();
        let tx = self.db.begin_read()?;
        let table = tx.open_table(EPISODES)?;
        let mut out = Vec::new();
        for entry in table.iter()?.rev() {
            let (_, value) = entry?;
            let ep: Episode = serde_json::from_slice(value.value())?;
            if !opts.include_deleted && ep.is_deleted() {
                continue;
            }
            if !opts.include_expired && ep.expired_at.is_some_and(|expired| expired <= now) {
                continue;
            }
            out.push(ep);
            if opts.limit > 0 && out.len() >= opts.limit {
                break;
            }
        }
        Ok(out)
    }

    /// Update an episode, archiving its prior state first. Refuses an
    /// ambiguous prefix before touching anything.
    pub fn update(&self, id_or_prefix: &str, params: UpdateParams) -> Result<Episode> {
        self.mutate(id_or_prefix, "update", |ep| {
            if let Some(name) = params.name.clone() {
                ep.name = Some(name);
            }
            if let Some(content) = params.content.clone() {
                ep.content = content;
            }
            if let Some(phrases) = params.search_phrases.clone() {
                ep.search_phrases = phrases;
            }
            if let Some(tags) = params.tags.clone() {
                ep.tags = tags;
            }
            if let Some(expired_at) = params.expired_at {
                ep.expired_at = Some(expired_at);
            }
            if let Some(metadata) = params.metadata.clone() {
                ep.metadata = metadata;
            }
        })
    }

    /// Demote (soft-delete). The only deletion agents get; recoverable via
    /// restore until an operator purge (v0.2) tiers it into git.
    pub fn demote(&self, id_or_prefix: &str) -> Result<Episode> {
        self.mutate(id_or_prefix, "delete", |ep| {
            ep.deleted_at = Some(Utc::now());
        })
    }

    /// Restore a demoted episode.
    pub fn restore(&self, id_or_prefix: &str) -> Result<Episode> {
        self.mutate(id_or_prefix, "restore", |ep| {
            ep.deleted_at = None;
        })
    }

    /// Replace the current episode with an archived snapshot and archive the
    /// displaced current state in the same transaction.
    pub fn restore_version(
        &self,
        id_or_prefix: &str,
        version_id_or_prefix: &str,
    ) -> Result<Episode> {
        let id = self.resolve_id(id_or_prefix)?;
        let episode_key = id.to_string();
        let version_needle = version_id_or_prefix.trim().to_lowercase();
        if version_needle.is_empty() {
            return Err(Error::VersionNotFound {
                episode_id: episode_key,
                version: version_id_or_prefix.to_string(),
            });
        }

        let tx = self.db.begin_write()?;
        let target = {
            let prefix = format!("{id}/");
            let vtable = tx.open_table(VERSIONS)?;
            let mut matched: Option<EpisodeVersion> = None;
            for entry in vtable.range(prefix.as_str()..)? {
                let (key, value) = entry?;
                if !key.value().starts_with(prefix.as_str()) {
                    break;
                }
                let version: EpisodeVersion = serde_json::from_slice(value.value())?;
                if !version.version_id.to_string().starts_with(&version_needle) {
                    continue;
                }
                if matched.is_some() {
                    return Err(Error::AmbiguousVersionPrefix {
                        episode_id: episode_key,
                        version: version_id_or_prefix.to_string(),
                    });
                }
                matched = Some(version);
            }
            matched.ok_or_else(|| Error::VersionNotFound {
                episode_id: episode_key.clone(),
                version: version_id_or_prefix.to_string(),
            })?
        };

        if target.episode.id != id {
            return Err(Error::Storage(format!(
                "archived version {} belongs to {}, expected {id}",
                target.version_id, target.episode.id
            )));
        }

        let restored = {
            let mut table = tx.open_table(EPISODES)?;
            let current: Episode = match table.get(episode_key.as_str())? {
                Some(guard) => serde_json::from_slice(guard.value())?,
                None => return Err(Error::NotFound(id_or_prefix.to_string())),
            };

            let displaced = EpisodeVersion {
                version_id: Uuid::now_v7(),
                archived_at: Utc::now(),
                operation: "rollback".to_string(),
                episode: current,
            };
            let version_key = format!(
                "{id}/{}/{}",
                displaced.archived_at.to_rfc3339(),
                displaced.version_id
            );
            let version_value = serde_json::to_vec(&displaced)?;
            let mut vtable = tx.open_table(VERSIONS)?;
            vtable.insert(version_key.as_str(), version_value.as_slice())?;

            let restored = target.episode;
            let value = serde_json::to_vec(&restored)?;
            table.insert(episode_key.as_str(), value.as_slice())?;
            restored
        };
        tx.commit()?;
        Ok(restored)
    }

    /// Hard-delete a DEMOTED episode: the record and its entire archived
    /// version history, in one write transaction. Refuses episodes that are
    /// not already demoted (two-phase delete; no force path) — the operator
    /// purge CLI is the only caller. Returns the purged episode and the
    /// number of archived versions removed.
    pub fn purge(&self, id_or_prefix: &str) -> Result<(Episode, usize)> {
        let id = self.resolve_id(id_or_prefix)?;
        let key = id.to_string();
        let tx = self.db.begin_write()?;
        let (episode, versions_removed) = {
            let mut table = tx.open_table(EPISODES)?;
            let episode: Episode = match table.get(key.as_str())? {
                Some(guard) => serde_json::from_slice(guard.value())?,
                None => return Err(Error::NotFound(id_or_prefix.to_string())),
            };
            if !episode.is_deleted() {
                return Err(Error::NotDemoted(key));
            }
            table.remove(key.as_str())?;

            let mut vtable = tx.open_table(VERSIONS)?;
            let prefix = format!("{id}/");
            let stale: Vec<String> = vtable
                .range(prefix.as_str()..)?
                .filter_map(|e| e.ok())
                .map(|(k, _)| k.value().to_string())
                .take_while(|k| k.starts_with(prefix.as_str()))
                .collect();
            let versions_removed = stale.len();
            for vkey in stale {
                vtable.remove(vkey.as_str())?;
            }
            (episode, versions_removed)
        };
        tx.commit()?;
        Ok((episode, versions_removed))
    }

    /// Archived prior states, oldest first.
    pub fn versions(&self, id_or_prefix: &str) -> Result<Vec<EpisodeVersion>> {
        let id = self.resolve_id(id_or_prefix)?;
        let prefix = format!("{id}/");
        let tx = self.db.begin_read()?;
        let table = tx.open_table(VERSIONS)?;
        let mut out = Vec::new();
        for entry in table.range(prefix.as_str()..)? {
            let (key, value) = entry?;
            if !key.value().starts_with(prefix.as_str()) {
                break;
            }
            out.push(serde_json::from_slice(value.value())?);
        }
        Ok(out)
    }

    pub fn count(&self) -> Result<u64> {
        let tx = self.db.begin_read()?;
        let table = tx.open_table(EPISODES)?;
        Ok(table.len()?)
    }

    /// Shared mutation path: resolve, archive prior state, apply, write —
    /// all inside one write transaction so a crash can't lose the archive.
    fn mutate(
        &self,
        id_or_prefix: &str,
        operation: &str,
        apply: impl FnOnce(&mut Episode),
    ) -> Result<Episode> {
        let id = self.resolve_id(id_or_prefix)?;
        let key = id.to_string();
        let tx = self.db.begin_write()?;
        let updated = {
            let mut table = tx.open_table(EPISODES)?;
            let prior: Episode = match table.get(key.as_str())? {
                Some(guard) => serde_json::from_slice(guard.value())?,
                None => return Err(Error::NotFound(id_or_prefix.to_string())),
            };

            let version = EpisodeVersion {
                version_id: Uuid::now_v7(),
                archived_at: Utc::now(),
                operation: operation.to_string(),
                episode: prior.clone(),
            };
            let vkey = format!(
                "{id}/{}/{}",
                version.archived_at.to_rfc3339(),
                version.version_id
            );
            let vvalue = serde_json::to_vec(&version)?;
            let mut vtable = tx.open_table(VERSIONS)?;
            vtable.insert(vkey.as_str(), vvalue.as_slice())?;

            let mut updated = prior;
            apply(&mut updated);
            let value = serde_json::to_vec(&updated)?;
            table.insert(key.as_str(), value.as_slice())?;
            updated
        };
        tx.commit()?;
        Ok(updated)
    }
}

// ---- flight recorder persistence -------------------------------------------
//
// Write paths never fail the caller: the recorder is diagnostics, and a
// search that succeeded must not error because its log write didn't.

impl Store {
    pub fn log_search(&self, entry: &crate::recorder::SearchLogEntry) {
        if let Err(e) = self.try_log(SEARCH_LOG, &entry.id.to_string(), entry) {
            tracing::warn!("search log write failed: {e}");
        }
    }

    pub fn log_access(&self, entry: &crate::recorder::AccessLogEntry) {
        if let Err(e) = self.try_log(ACCESS_LOG, &entry.id.to_string(), entry) {
            tracing::warn!("access log write failed: {e}");
        }
    }

    /// Ratings are the consumer's verdict, not diagnostics — a failed write
    /// surfaces to the caller instead of being swallowed.
    pub fn log_rating(&self, entry: &crate::recorder::RatingLogEntry) -> Result<()> {
        self.try_log(RATING_LOG, &entry.id.to_string(), entry)
    }

    /// Resolutions are heal-lifecycle state, not diagnostics — failures
    /// surface. Writing an existing id overwrites in place, which is how a
    /// replay pass records its outcome on the resolution it re-verified.
    pub fn log_resolution(&self, entry: &crate::recorder::ResolutionLogEntry) -> Result<()> {
        self.try_log(RESOLUTION_LOG, &entry.id.to_string(), entry)
    }

    /// Direct search-log lookup by entry id — validates rate_search targets.
    pub fn get_search_entry(&self, id: &str) -> Result<crate::recorder::SearchLogEntry> {
        let tx = self.db.begin_read()?;
        let t = tx.open_table(SEARCH_LOG)?;
        match t.get(id.trim().to_lowercase().as_str())? {
            Some(guard) => Ok(serde_json::from_slice(guard.value())?),
            None => Err(Error::NotFound(format!("search log entry {id}"))),
        }
    }

    fn try_log<T: serde::Serialize>(
        &self,
        table: TableDefinition<&str, &[u8]>,
        key: &str,
        value: &T,
    ) -> Result<()> {
        let bytes = serde_json::to_vec(value)?;
        let tx = self.db.begin_write()?;
        {
            let mut t = tx.open_table(table)?;
            t.insert(key, bytes.as_slice())?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Newest-first search log entries.
    pub fn recent_searches(&self, limit: usize) -> Result<Vec<crate::recorder::SearchLogEntry>> {
        self.read_log(SEARCH_LOG, limit)
    }

    /// Newest-first access log entries.
    pub fn recent_accesses(&self, limit: usize) -> Result<Vec<crate::recorder::AccessLogEntry>> {
        self.read_log(ACCESS_LOG, limit)
    }

    /// Newest-first rating log entries.
    pub fn recent_ratings(&self, limit: usize) -> Result<Vec<crate::recorder::RatingLogEntry>> {
        self.read_log(RATING_LOG, limit)
    }

    /// Newest-first resolutions (validated heals).
    pub fn recent_resolutions(
        &self,
        limit: usize,
    ) -> Result<Vec<crate::recorder::ResolutionLogEntry>> {
        self.read_log(RESOLUTION_LOG, limit)
    }

    fn read_log<T: serde::de::DeserializeOwned>(
        &self,
        table: TableDefinition<&str, &[u8]>,
        limit: usize,
    ) -> Result<Vec<T>> {
        let tx = self.db.begin_read()?;
        let t = tx.open_table(table)?;
        let mut out = Vec::new();
        for entry in t.iter()?.rev() {
            let (_, value) = entry?;
            out.push(serde_json::from_slice(value.value())?);
            if limit > 0 && out.len() >= limit {
                break;
            }
        }
        Ok(out)
    }

    /// Delete recorder entries older than the cutoff. Returns entries removed.
    /// RESOLUTION_LOG is exempt on purpose: heals are permanent regression
    /// tests, not diagnostics.
    pub fn prune_logs(&self, cutoff: chrono::DateTime<chrono::Utc>) -> Result<usize> {
        let mut removed = 0;
        for table in [SEARCH_LOG, ACCESS_LOG, RATING_LOG] {
            let tx = self.db.begin_write()?;
            {
                let mut t = tx.open_table(table)?;
                // UUIDv7 keys are time-ordered, but comparing the recorded ts
                // is simpler than synthesizing a boundary key, and log sizes
                // here make a scan irrelevant.
                let stale: Vec<String> = t
                    .iter()?
                    .filter_map(|e| e.ok())
                    .filter_map(|(k, v)| {
                        let ts = serde_json::from_slice::<serde_json::Value>(v.value())
                            .ok()?
                            .get("ts")?
                            .as_str()?
                            .parse::<chrono::DateTime<chrono::Utc>>()
                            .ok()?;
                        (ts < cutoff).then(|| k.value().to_string())
                    })
                    .collect();
                for key in stale {
                    t.remove(key.as_str())?;
                    removed += 1;
                }
            }
            tx.commit()?;
        }
        Ok(removed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store() -> (Store, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = Store::open(dir.path().join("test.redb")).expect("open");
        (store, dir)
    }

    fn sample(content: &str) -> Episode {
        Episode::new(content, "test")
    }

    #[test]
    fn store_id_is_minted_once_and_survives_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("test.redb");
        let first = Store::open(&db).unwrap().store_id();
        let second = Store::open(&db).unwrap().store_id();
        assert_eq!(first, second, "a store keeps its id across opens");
        let other = Store::open(dir.path().join("other.redb"))
            .unwrap()
            .store_id();
        assert_ne!(first, other, "separate stores are separately identified");
    }

    #[test]
    fn insert_get_roundtrip() {
        let (store, _dir) = temp_store();
        let mut ep = sample("the flight recorder logs every search");
        ep.name = Some("flight recorder".into());
        ep.search_phrases = vec!["how do I see what queries were made".into()];
        ep.tags = vec!["design".into()];
        store.insert(&ep).unwrap();

        let got = store.get(&ep.id.to_string()).unwrap();
        assert_eq!(got, ep);
    }

    #[test]
    fn prefix_resolution() {
        let (store, _dir) = temp_store();
        let a = sample("alpha");
        let b = sample("beta");
        store.insert(&a).unwrap();
        store.insert(&b).unwrap();

        // Full UUID resolves even with no table scan.
        assert_eq!(store.resolve_id(&a.id.to_string()).unwrap(), a.id);

        // A long-enough prefix resolves uniquely.
        let a_str = a.id.to_string();
        let unique = &a_str[..30];
        assert_eq!(store.resolve_id(unique).unwrap(), a.id);

        // UUIDv7 ids created in the same millisecond share a long prefix, so
        // a short shared prefix must be rejected as ambiguous.
        let b_str = b.id.to_string();
        let shared: String = a_str
            .chars()
            .zip(b_str.chars())
            .take_while(|(x, y)| x == y)
            .map(|(x, _)| x)
            .collect();
        if !shared.is_empty() {
            match store.resolve_id(&shared) {
                Err(Error::AmbiguousPrefix(_)) => {}
                other => panic!("expected AmbiguousPrefix, got {other:?}"),
            }
        }

        // Unknown prefix → NotFound.
        match store.resolve_id("ffffffff") {
            Err(Error::NotFound(_)) => {}
            other => panic!("expected NotFound, got {other:?}"),
        }
    }

    #[test]
    fn update_archives_prior_state() {
        let (store, _dir) = temp_store();
        let ep = sample("original content");
        store.insert(&ep).unwrap();

        let updated = store
            .update(
                &ep.id.to_string(),
                UpdateParams {
                    content: Some("revised content".into()),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(updated.content, "revised content");

        let versions = store.versions(&ep.id.to_string()).unwrap();
        assert_eq!(versions.len(), 1);
        assert_eq!(versions[0].operation, "update");
        assert_eq!(versions[0].episode.content, "original content");
    }

    #[test]
    fn restore_version_restores_exact_snapshot_and_archives_displaced_state() {
        let (store, _dir) = temp_store();
        let original = sample("original content");
        store.insert(&original).unwrap();

        let expires = Utc::now() + chrono::Duration::days(1);
        let revised = store
            .update(
                &original.id.to_string(),
                UpdateParams {
                    name: Some("revised name".into()),
                    content: Some("revised content".into()),
                    search_phrases: Some(vec!["revised cue".into()]),
                    tags: Some(vec!["revised".into()]),
                    expired_at: Some(expires),
                    metadata: Some(serde_json::json!({"state": "revised"})),
                },
            )
            .unwrap();
        let original_version = store.versions(&original.id.to_string()).unwrap()[0].clone();

        let restored = store
            .restore_version(
                &original.id.to_string(),
                &original_version.version_id.to_string()[..12],
            )
            .unwrap();
        assert_eq!(restored, original);

        let versions = store.versions(&original.id.to_string()).unwrap();
        assert_eq!(versions.len(), 2);
        assert_eq!(versions[1].operation, "rollback");
        assert_eq!(versions[1].episode, revised);

        let revised_version = versions[1].version_id;
        let restored_again = store
            .restore_version(&original.id.to_string(), &revised_version.to_string())
            .unwrap();
        assert_eq!(restored_again, revised);
        assert_eq!(store.versions(&original.id.to_string()).unwrap().len(), 3);
    }

    #[test]
    fn missing_version_refuses_without_archiving_or_mutating() {
        let (store, _dir) = temp_store();
        let ep = sample("unchanged");
        store.insert(&ep).unwrap();

        match store.restore_version(&ep.id.to_string(), "ffffffff") {
            Err(Error::VersionNotFound { .. }) => {}
            other => panic!("expected VersionNotFound, got {other:?}"),
        }
        assert_eq!(store.get(&ep.id.to_string()).unwrap(), ep);
        assert!(store.versions(&ep.id.to_string()).unwrap().is_empty());
    }

    #[test]
    fn demote_hides_from_list_but_get_still_returns() {
        let (store, _dir) = temp_store();
        let ep = sample("demote me");
        store.insert(&ep).unwrap();

        store.demote(&ep.id.to_string()).unwrap();

        let listed = store.list(ListOptions::default()).unwrap();
        assert!(
            listed.iter().all(|e| e.id != ep.id),
            "demoted episode listed"
        );

        // Handoff asymmetry: get-by-id still returns it, flagged.
        let got = store.get(&ep.id.to_string()).unwrap();
        assert!(got.is_deleted());

        let listed_all = store
            .list(ListOptions {
                include_deleted: true,
                ..Default::default()
            })
            .unwrap();
        assert!(listed_all.iter().any(|e| e.id == ep.id));
    }

    #[test]
    fn restore_after_demote() {
        let (store, _dir) = temp_store();
        let ep = sample("restore me");
        store.insert(&ep).unwrap();
        store.demote(&ep.id.to_string()).unwrap();
        let restored = store.restore(&ep.id.to_string()).unwrap();
        assert!(!restored.is_deleted());

        let versions = store.versions(&ep.id.to_string()).unwrap();
        let ops: Vec<&str> = versions.iter().map(|v| v.operation.as_str()).collect();
        assert_eq!(ops, vec!["delete", "restore"]);
    }

    #[test]
    fn purge_refuses_non_demoted() {
        let (store, _dir) = temp_store();
        let ep = sample("still live");
        store.insert(&ep).unwrap();

        match store.purge(&ep.id.to_string()) {
            Err(Error::NotDemoted(id)) => assert_eq!(id, ep.id.to_string()),
            other => panic!("expected NotDemoted, got {other:?}"),
        }
        // Refusal destroyed nothing.
        assert!(store.get(&ep.id.to_string()).is_ok());
        assert_eq!(store.count().unwrap(), 1);
    }

    #[test]
    fn purge_removes_record_and_all_versions() {
        let (store, _dir) = temp_store();
        let ep = sample("purge me");
        store.insert(&ep).unwrap();
        store
            .update(
                &ep.id.to_string(),
                UpdateParams {
                    content: Some("revised".into()),
                    ..Default::default()
                },
            )
            .unwrap();
        store.demote(&ep.id.to_string()).unwrap();
        assert_eq!(store.versions(&ep.id.to_string()).unwrap().len(), 2);

        let (purged, versions_removed) = store.purge(&ep.id.to_string()).unwrap();
        assert_eq!(purged.id, ep.id);
        assert_eq!(versions_removed, 2);

        match store.get(&ep.id.to_string()) {
            Err(Error::NotFound(_)) => {}
            other => panic!("expected NotFound, got {other:?}"),
        }
        // Full-UUID lookup bypasses the prefix scan, so versions() still
        // runs — and must find nothing left behind.
        assert!(store.versions(&ep.id.to_string()).unwrap().is_empty());
        assert_eq!(store.count().unwrap(), 0);
    }

    #[test]
    fn purge_resolves_unique_prefix_and_refuses_ambiguous() {
        let (store, _dir) = temp_store();
        let a = sample("first sibling");
        let b = sample("second sibling");
        store.insert(&a).unwrap();
        store.insert(&b).unwrap();
        store.demote(&a.id.to_string()).unwrap();
        store.demote(&b.id.to_string()).unwrap();

        // UUIDv7 same-millisecond siblings share a long prefix; that shared
        // prefix must be refused as ambiguous with nothing destroyed.
        let a_str = a.id.to_string();
        let b_str = b.id.to_string();
        let shared: String = a_str
            .chars()
            .zip(b_str.chars())
            .take_while(|(x, y)| x == y)
            .map(|(x, _)| x)
            .collect();
        if !shared.is_empty() {
            match store.purge(&shared) {
                Err(Error::AmbiguousPrefix(_)) => {}
                other => panic!("expected AmbiguousPrefix, got {other:?}"),
            }
        }
        assert_eq!(
            store.count().unwrap(),
            2,
            "ambiguous prefix must destroy nothing"
        );

        // One character past the divergence point is unique — resolves.
        let unique = &a_str[..shared.len() + 1];
        let (purged, _) = store.purge(unique).unwrap();
        assert_eq!(purged.id, a.id);
        assert!(store.get(&b.id.to_string()).is_ok(), "sibling must survive");
    }

    #[test]
    fn list_newest_first_with_limit() {
        let (store, _dir) = temp_store();
        for i in 0..5 {
            store.insert(&sample(&format!("episode {i}"))).unwrap();
        }
        let listed = store
            .list(ListOptions {
                limit: 3,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(listed.len(), 3);
        assert_eq!(listed[0].content, "episode 4");
        assert_eq!(listed[2].content, "episode 2");
    }

    #[test]
    fn expired_hidden_by_default() {
        let (store, _dir) = temp_store();
        let mut ep = sample("already expired");
        ep.expired_at = Some(Utc::now() - chrono::Duration::hours(1));
        store.insert(&ep).unwrap();

        assert!(store.list(ListOptions::default()).unwrap().is_empty());
        let with_expired = store
            .list(ListOptions {
                include_expired: true,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(with_expired.len(), 1);
    }

    #[test]
    fn reopen_persists() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("persist.redb");
        let ep = sample("survives reopen");
        {
            let store = Store::open(&path).unwrap();
            store.insert(&ep).unwrap();
        }
        let store = Store::open(&path).unwrap();
        assert_eq!(store.get(&ep.id.to_string()).unwrap(), ep);
        assert_eq!(store.count().unwrap(), 1);
    }
}
