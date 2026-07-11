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

pub struct Store {
    db: Database,
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
        {
            tx.open_table(EPISODES)?;
            tx.open_table(VERSIONS)?;
        }
        tx.commit()?;
        Ok(Self { db })
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
    fn demote_hides_from_list_but_get_still_returns() {
        let (store, _dir) = temp_store();
        let ep = sample("demote me");
        store.insert(&ep).unwrap();

        store.demote(&ep.id.to_string()).unwrap();

        let listed = store.list(ListOptions::default()).unwrap();
        assert!(listed.iter().all(|e| e.id != ep.id), "demoted episode listed");

        // Handoff asymmetry: get-by-id still returns it, flagged.
        let got = store.get(&ep.id.to_string()).unwrap();
        assert!(got.is_deleted());

        let listed_all = store
            .list(ListOptions { include_deleted: true, ..Default::default() })
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
    fn list_newest_first_with_limit() {
        let (store, _dir) = temp_store();
        for i in 0..5 {
            store.insert(&sample(&format!("episode {i}"))).unwrap();
        }
        let listed = store
            .list(ListOptions { limit: 3, ..Default::default() })
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
            .list(ListOptions { include_expired: true, ..Default::default() })
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
