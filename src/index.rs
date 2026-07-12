use std::path::Path;

use tantivy::collector::TopDocs;
use tantivy::query::QueryParser;
use tantivy::schema::{Field, Schema, TantivyDocument, Value, STORED, STRING, TEXT};
use tantivy::{Index, IndexWriter, Term};

use crate::error::{Error, Result};
use crate::model::Episode;

/// BM25 index over episode text. Derived data: fully rebuildable from the
/// redb store, so index-format changes are a reindex, never a migration.
pub struct SearchIndex {
    index: Index,
    writer: IndexWriter,
    id: Field,
    name: Field,
    content: Field,
    phrases: Field,
    deleted: Field,
}

/// A ranked hit: episode id + BM25 score. The service layer joins back to
/// the store for the authoritative episode (and post-filters).
#[derive(Debug, Clone)]
pub struct Hit {
    pub id: String,
    pub score: f32,
}

fn schema() -> Schema {
    let mut b = Schema::builder();
    b.add_text_field("id", STRING | STORED);
    b.add_text_field("name", TEXT);
    b.add_text_field("content", TEXT);
    // Write-time paraphrase cues get their own field so they can be boosted:
    // a match on how-you'd-ask-for-it should outrank an incidental body match.
    b.add_text_field("phrases", TEXT);
    b.add_text_field("deleted", STRING);
    b.build()
}

impl SearchIndex {
    pub fn open(dir: impl AsRef<Path>) -> Result<Self> {
        std::fs::create_dir_all(&dir)
            .map_err(|e| Error::Storage(format!("creating index dir: {e}")))?;
        let schema = schema();
        let dir = tantivy::directory::MmapDirectory::open(&dir)
            .map_err(|e| Error::Storage(format!("opening index dir: {e}")))?;
        let index = Index::open_or_create(dir, schema.clone())
            .map_err(|e| Error::Storage(format!("opening index: {e}")))?;
        let writer = index
            .writer(50_000_000)
            .map_err(|e| Error::Storage(format!("creating index writer: {e}")))?;

        let f = |n: &str| schema.get_field(n).expect("schema field");
        Ok(Self {
            id: f("id"),
            name: f("name"),
            content: f("content"),
            phrases: f("phrases"),
            deleted: f("deleted"),
            index,
            writer,
        })
    }

    fn doc_for(&self, ep: &Episode) -> TantivyDocument {
        let mut doc = TantivyDocument::default();
        doc.add_text(self.id, ep.id.to_string());
        if let Some(name) = &ep.name {
            doc.add_text(self.name, name);
        }
        doc.add_text(self.content, &ep.content);
        for phrase in &ep.search_phrases {
            doc.add_text(self.phrases, phrase);
        }
        doc.add_text(self.deleted, if ep.is_deleted() { "1" } else { "0" });
        doc
    }

    /// Insert or replace one episode's document. Caller decides when to
    /// commit (mutations commit immediately; bulk import commits once).
    pub fn upsert(&mut self, ep: &Episode) -> Result<()> {
        self.writer
            .delete_term(Term::from_field_text(self.id, &ep.id.to_string()));
        self.writer
            .add_document(self.doc_for(ep))
            .map_err(|e| Error::Storage(format!("indexing episode: {e}")))?;
        Ok(())
    }

    pub fn commit(&mut self) -> Result<()> {
        self.writer
            .commit()
            .map_err(|e| Error::Storage(format!("committing index: {e}")))?;
        Ok(())
    }

    /// Drop everything and reindex from the given episodes (the store is the
    /// source of truth; this is the recovery path for any index drift).
    pub fn rebuild<'a>(&mut self, episodes: impl Iterator<Item = &'a Episode>) -> Result<usize> {
        self.writer
            .delete_all_documents()
            .map_err(|e| Error::Storage(format!("clearing index: {e}")))?;
        let mut n = 0;
        for ep in episodes {
            self.writer
                .add_document(self.doc_for(ep))
                .map_err(|e| Error::Storage(format!("indexing episode: {e}")))?;
            n += 1;
        }
        self.commit()?;
        Ok(n)
    }

    /// BM25 search across name/content/phrases with phrases boosted highest.
    /// Returns ranked (id, score); lenient parsing so raw user text with
    /// stray syntax characters never errors.
    pub fn search(&self, query: &str, limit: usize, include_deleted: bool) -> Result<Vec<Hit>> {
        let reader = self
            .index
            .reader()
            .map_err(|e| Error::Storage(format!("index reader: {e}")))?;
        let searcher = reader.searcher();

        let mut parser =
            QueryParser::for_index(&self.index, vec![self.name, self.content, self.phrases]);
        parser.set_field_boost(self.phrases, 2.0);
        parser.set_field_boost(self.name, 1.5);
        let (query, _errors) = parser.parse_query_lenient(query);

        let top = searcher
            .search(&query, &TopDocs::with_limit(limit.max(1)).order_by_score())
            .map_err(|e| Error::Storage(format!("search: {e}")))?;

        let mut hits = Vec::with_capacity(top.len());
        for (score, addr) in top {
            let doc: TantivyDocument = searcher
                .doc(addr)
                .map_err(|e| Error::Storage(format!("fetching doc: {e}")))?;
            let id = doc
                .get_first(self.id)
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            let deleted = doc
                .get_first(self.deleted)
                .and_then(|v| v.as_str())
                .unwrap_or("0")
                == "1";
            if deleted && !include_deleted {
                continue;
            }
            hits.push(Hit { id, score });
        }
        Ok(hits)
    }
}
