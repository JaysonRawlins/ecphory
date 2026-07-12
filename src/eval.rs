use std::path::Path;

use serde::Deserialize;

use crate::error::{Error, Result};

/// Retrieval evaluation, run over HTTP against a LIVE daemon on purpose:
/// it scores the path real clients hit (auth included) and never contends
/// for the redb lock.
///
/// Two modes:
/// - `--gold`: hand-authored {query, id} pairs (id may be a unique prefix).
/// - `--from-log`: the flight recorder is the gold set — each by-id fetch
///   is joined to the nearest preceding search (the used-signal), and the
///   accessed episode becomes that query's label.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Bucket {
    Identifier,
    Conceptual,
    Mixed,
}

impl std::fmt::Display for Bucket {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Bucket::Identifier => write!(f, "identifier"),
            Bucket::Conceptual => write!(f, "conceptual"),
            Bucket::Mixed => write!(f, "mixed"),
        }
    }
}

/// Heuristic query classifier (the anti-pattern this guards against:
/// evaluating only on one query class and declaring victory).
pub fn classify(query: &str) -> Bucket {
    let has_identifier = query.split_whitespace().any(|w| {
        let digits = w.chars().filter(|c| c.is_ascii_digit()).count();
        digits >= 4
            || w.contains('/')
            || w.contains('_')
            || w.contains("::")
            || (w.len() >= 3 && w.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit()))
            || (w.contains('-') && digits >= 1)
    });

    let lower = query.to_lowercase();
    let first = lower.split_whitespace().next().unwrap_or("");
    let question_start = matches!(
        first,
        "how" | "why" | "what" | "when" | "where" | "can" | "cannot" | "does" | "is" | "should" | "who"
    );
    let symptom_words = [
        "cannot", "can't", "fails", "failed", "failing", "hangs", "hang", "slow", "broken",
        "not working", "unfindable", "missing", "times out", "timeout", "error", "crash",
    ];
    let has_conceptual = question_start || symptom_words.iter().any(|w| lower.contains(w));

    match (has_identifier, has_conceptual) {
        (true, true) => Bucket::Mixed,
        (true, false) => Bucket::Identifier,
        (false, true) => Bucket::Conceptual,
        // No strong signal either way: short keyword piles behave like
        // identifiers, longer natural phrases like conceptual queries.
        (false, false) => {
            if query.split_whitespace().count() <= 4 {
                Bucket::Identifier
            } else {
                Bucket::Conceptual
            }
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct GoldPair {
    pub query: String,
    pub id: String,
}

pub fn parse_gold(path: &Path) -> Result<Vec<GoldPair>> {
    let raw = std::fs::read_to_string(path)
        .map_err(|e| Error::Storage(format!("reading {}: {e}", path.display())))?;
    let mut pairs = Vec::new();
    for (n, line) in raw.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let pair: GoldPair = serde_json::from_str(line)
            .map_err(|e| Error::Storage(format!("{}:{}: {e}", path.display(), n + 1)))?;
        pairs.push(pair);
    }
    Ok(pairs)
}

// ---- HTTP client against the live daemon ------------------------------------

pub struct EvalClient {
    base: String,
    token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SearchResponse {
    latency_ms: f64,
    results: Vec<SearchHit>,
}

#[derive(Debug, Deserialize)]
struct SearchHit {
    episode: EpisodeId,
}

#[derive(Debug, Deserialize)]
struct EpisodeId {
    id: String,
}

#[derive(Debug, Deserialize)]
pub struct LoggedSearch {
    pub ts: chrono::DateTime<chrono::Utc>,
    pub query: String,
    pub result_count: usize,
    pub results: Vec<LoggedHit>,
}

#[derive(Debug, Deserialize)]
pub struct LoggedHit {
    pub id: String,
    pub rank: usize,
}

#[derive(Debug, Deserialize)]
pub struct LoggedAccess {
    pub ts: chrono::DateTime<chrono::Utc>,
    pub episode_id: String,
}

impl EvalClient {
    pub fn new(base: String, token: Option<String>) -> Self {
        Self { base: base.trim_end_matches('/').to_string(), token }
    }

    fn get_json<T: serde::de::DeserializeOwned>(&self, path_and_query: &str) -> Result<T> {
        let url = format!("{}{}", self.base, path_and_query);
        let mut req = ureq::get(&url);
        if let Some(token) = &self.token {
            req = req.header("Authorization", &format!("Bearer {token}"));
        }
        let mut resp = req
            .call()
            .map_err(|e| Error::Storage(format!("GET {url}: {e}")))?;
        resp.body_mut()
            .read_json()
            .map_err(|e| Error::Storage(format!("decoding {url}: {e}")))
    }

    pub fn search(&self, query: &str, k: usize) -> Result<(f64, Vec<String>)> {
        let encoded: String = url_encode(query);
        let resp: SearchResponse =
            self.get_json(&format!("/api/v1/memory/search?query={encoded}&max_results={k}"))?;
        Ok((resp.latency_ms, resp.results.into_iter().map(|r| r.episode.id).collect()))
    }

    pub fn search_log(&self, limit: usize) -> Result<Vec<LoggedSearch>> {
        #[derive(Deserialize)]
        struct Wrap {
            searches: Vec<LoggedSearch>,
        }
        let w: Wrap = self.get_json(&format!("/api/v1/memory/search-log?limit={limit}"))?;
        Ok(w.searches)
    }

    pub fn access_log(&self, limit: usize) -> Result<Vec<LoggedAccess>> {
        #[derive(Deserialize)]
        struct Wrap {
            accesses: Vec<LoggedAccess>,
        }
        let w: Wrap = self.get_json(&format!("/api/v1/memory/access-log?limit={limit}"))?;
        Ok(w.accesses)
    }
}

fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for byte in s.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

// ---- metrics ----------------------------------------------------------------

#[derive(Debug, Default, Clone)]
pub struct BucketMetrics {
    pub n: usize,
    pub hit1: usize,
    pub hitk: usize,
    pub mrr_sum: f64,
}

impl BucketMetrics {
    pub fn record(&mut self, rank: Option<usize>) {
        self.n += 1;
        if let Some(r) = rank {
            if r == 1 {
                self.hit1 += 1;
            }
            self.hitk += 1;
            self.mrr_sum += 1.0 / r as f64;
        }
    }

    pub fn mrr(&self) -> f64 {
        if self.n == 0 { 0.0 } else { self.mrr_sum / self.n as f64 }
    }

    pub fn line(&self, label: &str) -> String {
        if self.n == 0 {
            return format!("{label:<12} n=0");
        }
        format!(
            "{label:<12} n={:<4} hit@1 {:>5.1}%  hit@k {:>5.1}%  MRR {:.3}",
            self.n,
            100.0 * self.hit1 as f64 / self.n as f64,
            100.0 * self.hitk as f64 / self.n as f64,
            self.mrr()
        )
    }
}

/// Rank (1-based) of the gold id (full or prefix) in the returned ids.
pub fn rank_of(gold_id: &str, ids: &[String]) -> Option<usize> {
    ids.iter().position(|id| id.starts_with(gold_id)).map(|i| i + 1)
}

// ---- gold mode ---------------------------------------------------------------

pub fn run_gold(client: &EvalClient, pairs: &[GoldPair], k: usize, min_mrr: Option<f64>) -> Result<bool> {
    let mut overall = BucketMetrics::default();
    let mut by_bucket: std::collections::HashMap<Bucket, BucketMetrics> = Default::default();
    let mut latencies: Vec<f64> = Vec::new();
    let mut misses: Vec<(&GoldPair, Bucket)> = Vec::new();

    for pair in pairs {
        let bucket = classify(&pair.query);
        let (latency, ids) = client.search(&pair.query, k)?;
        latencies.push(latency);
        let rank = rank_of(&pair.id, &ids);
        overall.record(rank);
        by_bucket.entry(bucket).or_default().record(rank);
        if rank.is_none() {
            misses.push((pair, bucket));
        }
    }

    latencies.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let p50 = latencies.get(latencies.len() / 2).copied().unwrap_or(0.0);

    println!("gold eval: {} pairs, k={k}, server p50 {p50:.2}ms", pairs.len());
    println!("  {}", overall.line("overall"));
    for bucket in [Bucket::Identifier, Bucket::Conceptual, Bucket::Mixed] {
        if let Some(m) = by_bucket.get(&bucket) {
            println!("  {}", m.line(&bucket.to_string()));
        }
    }
    if !misses.is_empty() {
        println!("misses ({}):", misses.len());
        for (pair, bucket) in &misses {
            println!("  [{bucket}] {:?} -> {}", pair.query, &pair.id);
        }
    }

    if let Some(threshold) = min_mrr {
        let ok = overall.mrr() >= threshold;
        if !ok {
            println!("FAIL: MRR {:.3} < min {threshold:.3}", overall.mrr());
        }
        return Ok(ok);
    }
    Ok(true)
}

// ---- from-log mode -----------------------------------------------------------

pub fn run_from_log(client: &EvalClient, k: usize, window_secs: i64) -> Result<bool> {
    let searches = client.search_log(1000)?;
    let accesses = client.access_log(1000)?;

    // Tape overview: what does the real workload look like?
    let mut bucket_counts: std::collections::HashMap<Bucket, usize> = Default::default();
    let zero_hit = searches.iter().filter(|s| s.result_count == 0).count();
    for s in &searches {
        *bucket_counts.entry(classify(&s.query)).or_default() += 1;
    }
    println!("tape: {} searches ({} zero-hit), {} accesses", searches.len(), zero_hit, accesses.len());
    for bucket in [Bucket::Identifier, Bucket::Conceptual, Bucket::Mixed] {
        if let Some(n) = bucket_counts.get(&bucket) {
            println!("  {bucket:<12} {n}");
        }
    }

    // Used-signal join: each access labels the nearest preceding search
    // within the window. Dedupe identical (query, id) pairs.
    let mut labeled: Vec<(&LoggedSearch, &LoggedAccess)> = Vec::new();
    let mut seen: std::collections::HashSet<(String, String)> = Default::default();
    for access in &accesses {
        let candidate = searches
            .iter()
            .filter(|s| {
                s.ts <= access.ts && (access.ts - s.ts).num_seconds() <= window_secs
            })
            .max_by_key(|s| s.ts);
        if let Some(search) = candidate {
            if seen.insert((search.query.clone(), access.episode_id.clone())) {
                labeled.push((search, access));
            }
        }
    }

    if labeled.is_empty() {
        println!("used-signal: no search→fetch pairs within {window_secs}s yet — keep dogfooding");
        return Ok(true);
    }

    let mut logged = BucketMetrics::default();
    let mut fresh = BucketMetrics::default();
    println!("used-signal pairs ({}):", labeled.len());
    for (search, access) in &labeled {
        let logged_rank = search
            .results
            .iter()
            .find(|h| h.id == access.episode_id)
            .map(|h| h.rank);
        logged.record(logged_rank);

        let (_, ids) = client.search(&search.query, k)?;
        let fresh_rank = rank_of(&access.episode_id, &ids);
        fresh.record(fresh_rank);

        println!(
            "  {:?} -> {} | taped rank {} | now {}",
            truncate(&search.query, 48),
            &access.episode_id[..8],
            logged_rank.map(|r| r.to_string()).unwrap_or_else(|| "MISS".into()),
            fresh_rank.map(|r| r.to_string()).unwrap_or_else(|| "MISS".into()),
        );
    }
    println!("  {}", logged.line("as-taped"));
    println!("  {}", fresh.line("replayed-now"));
    Ok(true)
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(n).collect::<String>())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_buckets() {
        assert_eq!(classify("gavel reviewed signal PR 198"), Bucket::Identifier);
        assert_eq!(classify("aws account 842478712031 survivor"), Bucket::Identifier);
        assert_eq!(classify("DUCKDB_PATH split brain"), Bucket::Identifier);
        assert_eq!(
            classify("saved a memory but later sessions cannot find it anywhere"),
            Bucket::Conceptual
        );
        assert_eq!(classify("how do I restore a demoted episode"), Bucket::Conceptual);
        assert_eq!(
            classify("nightly job hangs with error ECPHORY_DB unset"),
            Bucket::Mixed
        );
        assert_eq!(classify("engram fork"), Bucket::Identifier); // short keyword pile
        assert_eq!(
            classify("that thing about deletion being tiering instead of destruction"),
            Bucket::Conceptual
        );
    }

    #[test]
    fn rank_of_prefix_matching() {
        let ids = vec!["aaaa1111-x".to_string(), "bbbb2222-y".to_string()];
        assert_eq!(rank_of("bbbb2222", &ids), Some(2));
        assert_eq!(rank_of("aaaa1111-x", &ids), Some(1));
        assert_eq!(rank_of("cccc", &ids), None);
    }

    #[test]
    fn metrics_math() {
        let mut m = BucketMetrics::default();
        m.record(Some(1));
        m.record(Some(2));
        m.record(None);
        assert_eq!(m.n, 3);
        assert_eq!(m.hit1, 1);
        assert_eq!(m.hitk, 2);
        assert!((m.mrr() - 0.5).abs() < 1e-9);
    }

    #[test]
    fn gold_parsing_with_comments() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("gold.jsonl");
        std::fs::write(&path, "# comment\n\n{\"query\":\"q1\",\"id\":\"abc\"}\n{\"query\":\"q2\",\"id\":\"def\"}\n").unwrap();
        let pairs = parse_gold(&path).unwrap();
        assert_eq!(pairs.len(), 2);
        assert_eq!(pairs[1].id, "def");
    }
}
