use std::path::Path;

use tantivy::collector::TopDocs;
use tantivy::query::{BooleanQuery, BoostQuery, Occur, Query, TermQuery};
use tantivy::schema::{
    Field, IndexRecordOption, STORED, STRING, Schema, TantivyDocument, TextFieldIndexing,
    TextOptions, Value,
};
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
    // English stemming on all text fields: DuckDB's FTS stems and tantivy's
    // default tokenizer doesn't — the first dogfood divergence (2026-07-11)
    // was morphological variants under-matching. Queries stem too (the
    // QueryParser uses the field's tokenizer).
    let stemmed = || {
        TextOptions::default().set_indexing_options(
            TextFieldIndexing::default()
                .set_tokenizer("en_stem")
                .set_index_option(IndexRecordOption::WithFreqsAndPositions),
        )
    };
    let mut b = Schema::builder();
    b.add_text_field("id", STRING | STORED);
    b.add_text_field("name", stemmed());
    b.add_text_field("content", stemmed());
    // Write-time paraphrase cues get their own field so they can be boosted:
    // a match on how-you'd-ask-for-it should outrank an incidental body match.
    b.add_text_field("phrases", stemmed());
    b.add_text_field("deleted", STRING);
    b.build()
}

impl SearchIndex {
    pub fn open(dir: impl AsRef<Path>) -> Result<Self> {
        std::fs::create_dir_all(&dir)
            .map_err(|e| Error::Storage(format!("creating index dir: {e}")))?;
        let schema = schema();
        let mmap = tantivy::directory::MmapDirectory::open(&dir)
            .map_err(|e| Error::Storage(format!("opening index dir: {e}")))?;
        // Self-healing on schema change: the index is derived data, so a
        // mismatch (e.g. tokenizer upgrade) wipes and rebuilds instead of
        // migrating. The store is the source of truth; the service layer
        // reindexes when it finds an empty index over a non-empty store.
        let index = match Index::open_or_create(mmap, schema.clone()) {
            Ok(index) => index,
            Err(open_err) => {
                tracing::warn!(
                    "index schema mismatch ({open_err}); wiping derived index for rebuild"
                );
                std::fs::remove_dir_all(&dir)
                    .and_then(|_| std::fs::create_dir_all(&dir))
                    .map_err(|e| Error::Storage(format!("resetting index dir: {e}")))?;
                let mmap = tantivy::directory::MmapDirectory::open(&dir)
                    .map_err(|e| Error::Storage(format!("reopening index dir: {e}")))?;
                Index::open_or_create(mmap, schema.clone())
                    .map_err(|e| Error::Storage(format!("recreating index: {e}")))?
            }
        };
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

    /// Remove one episode's document (operator purge). Caller commits.
    pub fn remove(&mut self, id: &str) -> Result<()> {
        self.writer.delete_term(Term::from_field_text(self.id, id));
        Ok(())
    }

    /// Whether the committed index holds a document for this id — the
    /// index-presence line in the purge manifest.
    pub fn contains(&self, id: &str) -> Result<bool> {
        let reader = self
            .index
            .reader()
            .map_err(|e| Error::Storage(format!("index reader: {e}")))?;
        let searcher = reader.searcher();
        let query = TermQuery::new(Term::from_field_text(self.id, id), IndexRecordOption::Basic);
        let count = searcher
            .search(&query, &tantivy::collector::Count)
            .map_err(|e| Error::Storage(format!("id lookup: {e}")))?;
        Ok(count > 0)
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

    /// Build the search query directly from the tokenized text instead of
    /// QueryParser: even parse_query_lenient honors operator syntax, so a
    /// leading dash ("fix -25308", "--name") EXCLUDES the term the user is
    /// searching for. Queries are raw user text, never syntax; quotes and
    /// booleans get no special meaning either.
    fn plain_text_query(&self, query: &str) -> Result<BooleanQuery> {
        let mut analyzer = self
            .index
            .tokenizer_for_field(self.content)
            .map_err(|e| Error::Storage(format!("query tokenizer: {e}")))?;
        let mut tokens: Vec<String> = Vec::new();
        let mut stream = analyzer.token_stream(query);
        while stream.advance() {
            tokens.push(stream.token().text.clone());
        }

        let mut clauses: Vec<(Occur, Box<dyn Query>)> = Vec::with_capacity(tokens.len() * 3);
        for token in &tokens {
            for (field, boost) in [(self.phrases, 2.0), (self.name, 1.5), (self.content, 1.0)] {
                let tq = TermQuery::new(
                    Term::from_field_text(field, token),
                    IndexRecordOption::WithFreqs,
                );
                let q: Box<dyn Query> = if boost == 1.0 {
                    Box::new(tq)
                } else {
                    Box::new(BoostQuery::new(Box::new(tq), boost))
                };
                clauses.push((Occur::Should, q));
            }
        }
        Ok(BooleanQuery::new(clauses))
    }

    /// BM25 search across name/content/phrases with phrases boosted highest.
    /// Returns ranked (id, score).
    pub fn search(&self, query: &str, limit: usize, include_deleted: bool) -> Result<Vec<Hit>> {
        let reader = self
            .index
            .reader()
            .map_err(|e| Error::Storage(format!("index reader: {e}")))?;
        let searcher = reader.searcher();

        let query = self.plain_text_query(query)?;

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Episode;

    fn indexed(episodes: &[Episode]) -> SearchIndex {
        let dir = tempfile::tempdir().unwrap();
        let mut idx = SearchIndex::open(dir.path()).unwrap();
        for ep in episodes {
            idx.upsert(ep).unwrap();
        }
        idx.commit().unwrap();
        // Keep the tempdir alive for the test's duration by leaking it; the
        // OS reclaims /tmp and the handle is process-scoped.
        std::mem::forget(dir);
        idx
    }

    fn ep(name: &str, content: &str) -> Episode {
        let mut e = Episode::new(content, "test");
        e.name = Some(name.into());
        e
    }

    #[test]
    fn leading_dash_token_is_a_term_not_an_exclusion() {
        // The regression from the live gold eval: "fix -25308" must FIND the
        // episode containing 25308, not exclude it.
        let idx = indexed(&[
            ep(
                "granted keychain to file-backend",
                "assume writes error -25308 user interaction is not allowed; switch keyring backend to file",
            ),
            ep("unrelated", "tailscale mosh phone blink headless ssh"),
        ]);
        let hits = idx
            .search(
                "granted keychain fix -25308 user interaction not allowed",
                5,
                false,
            )
            .unwrap();
        assert!(!hits.is_empty());
        let top = &hits[0];
        let target = idx
            .search("granted keychain to file-backend", 1, false)
            .unwrap()[0]
            .id
            .clone();
        assert_eq!(top.id, target, "dash token must match, not exclude");
    }

    #[test]
    fn cli_flag_tokens_match() {
        let idx = indexed(&[
            ep(
                "ghostty naming",
                "pair the terminal tab via claude --name pid",
            ),
            ep("other", "completely different content about databases"),
        ]);
        let hits = idx
            .search("Ghostty --name pid terminal tab", 5, false)
            .unwrap();
        assert!(!hits.is_empty());
        // Before the fix this query returned only the non-matching doc set.
        let top_doc = &hits[0];
        let by_name = idx.search("ghostty naming", 1, false).unwrap();
        assert_eq!(top_doc.id, by_name[0].id);
    }

    #[test]
    fn operators_and_quotes_are_plain_text() {
        let idx = indexed(&[ep("a", "alpha AND beta OR \"gamma\" field:value")]);
        for q in ["alpha AND beta", "\"gamma\"", "field:value", "(alpha)"] {
            assert!(
                !idx.search(q, 5, false).unwrap().is_empty(),
                "query {q:?} should match as plain text"
            );
        }
    }

    #[test]
    fn empty_and_whitespace_queries_return_nothing() {
        let idx = indexed(&[ep("a", "some content")]);
        assert!(idx.search("", 5, false).unwrap().is_empty());
        assert!(idx.search("   ", 5, false).unwrap().is_empty());
        assert!(idx.search("--- ::: !!!", 5, false).unwrap().is_empty());
    }

    #[test]
    fn phrase_field_boost_still_applies() {
        let mut with_phrase = ep("ep-with-phrase", "body about databases");
        with_phrase.search_phrases = vec!["release page has nothing to download".into()];
        let content_only = ep(
            "ep-content-only",
            "the release page has nothing to download today",
        );
        let idx = indexed(&[with_phrase, content_only]);
        let hits = idx
            .search("release page has nothing to download", 2, false)
            .unwrap();
        assert_eq!(hits.len(), 2);
        let name_of_top = idx.search("ep-with-phrase", 1, false).unwrap()[0]
            .id
            .clone();
        assert_eq!(
            hits[0].id, name_of_top,
            "a search_phrases match must outrank an incidental content match"
        );
    }
}
