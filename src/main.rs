mod error;
mod eval;
mod http;
mod import;
mod index;
mod mcp;
mod model;
mod recorder;
mod service;
mod store;

use std::path::PathBuf;

use clap::{Parser, Subcommand};

use crate::model::{Episode, UpdateParams};
use crate::service::{Ecphory, SearchOptions};
use crate::store::ListOptions;

/// ecphory — recall, measured. A lexical-first memory store for AI agents.
#[derive(Parser)]
#[command(name = "ecphory", version, about)]
struct Cli {
    /// Database path. Defaults to $ECPHORY_DB, then
    /// ~/.local/share/ecphory/ecphory.redb. Never relative to the cwd —
    /// that footgun corrupts stores when servers start from the wrong dir.
    #[arg(long, global = true)]
    db: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Store a new episode
    Add {
        /// Episode content (verbatim; never rewritten server-side)
        content: String,
        #[arg(long)]
        name: Option<String>,
        #[arg(long, default_value = "cli")]
        source: String,
        /// Paraphrase search cues (repeatable) — write-time lexical enrichment
        #[arg(long = "phrase")]
        phrases: Vec<String>,
        #[arg(long = "tag")]
        tags: Vec<String>,
    },
    /// BM25 search over content, names, and search phrases
    Search {
        query: String,
        #[arg(long, default_value_t = 10)]
        limit: usize,
        #[arg(long)]
        include_deleted: bool,
        #[arg(long)]
        group: Option<String>,
        #[arg(long)]
        source: Option<String>,
        #[arg(long = "tag")]
        tags: Vec<String>,
        /// One-line-per-hit output (id prefix, score, latency, name)
        #[arg(long)]
        brief: bool,
    },
    /// Fetch an episode by id or unique prefix
    Get { id: String },
    /// List episodes, newest first
    List {
        #[arg(long, default_value_t = 10)]
        limit: usize,
        #[arg(long)]
        include_deleted: bool,
    },
    /// Update fields on an episode (prior state is archived)
    Update {
        id: String,
        #[arg(long)]
        content: Option<String>,
        #[arg(long)]
        name: Option<String>,
        #[arg(long = "phrase")]
        phrases: Vec<String>,
        #[arg(long = "tag")]
        tags: Vec<String>,
    },
    /// Demote (soft-delete) an episode — recoverable via restore
    Demote { id: String },
    /// Restore a demoted episode
    Restore { id: String },
    /// Show the archived version history of an episode
    Versions { id: String },
    /// Import episodes from an engram git-export mirror directory
    Import {
        #[arg(long)]
        dir: PathBuf,
    },
    /// Rebuild the search index from the store (recovery / schema change)
    Reindex,
    /// Store status
    Status,
    /// Flight-recorder aggregates: query counts, zero-hit rate, latency percentiles
    Stats,
    /// Recent recorded searches, newest first
    SearchLog {
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    /// Recent recorded episode fetches (the used-signal), newest first
    AccessLog {
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    /// Serve MCP over stdio (single client; prefer `serve` for shared use)
    Mcp,
    /// Serve MCP over streamable HTTP on localhost — one daemon, many sessions
    Serve {
        /// Port (or $ECPHORY_PORT; default 3491)
        #[arg(long)]
        port: Option<u16>,
    },
    /// Score retrieval quality against the LIVE daemon (HTTP; no lock contention)
    Eval {
        /// Gold set: JSONL of {"query": ..., "id": ...} (id may be a prefix)
        #[arg(long)]
        gold: Option<std::path::PathBuf>,
        /// Evaluate from the flight recorder: joins fetches to preceding
        /// searches (used-signal) and reports taped vs replayed ranks
        #[arg(long)]
        from_log: bool,
        /// Daemon base URL
        #[arg(long, default_value = "http://127.0.0.1:3491")]
        url: String,
        #[arg(long, default_value_t = 5)]
        k: usize,
        /// Exit non-zero if overall gold MRR falls below this (drift guard)
        #[arg(long)]
        min_mrr: Option<f64>,
        /// Used-signal join window in seconds
        #[arg(long, default_value_t = 300)]
        window: i64,
    },
}

fn db_path(cli_flag: Option<PathBuf>) -> anyhow::Result<PathBuf> {
    if let Some(p) = cli_flag {
        return Ok(p);
    }
    if let Ok(p) = std::env::var("ECPHORY_DB") {
        return Ok(PathBuf::from(p));
    }
    // Absolute default or fail-fast. Deliberately NOT ./ecphory.redb: a
    // cwd-relative fallback once split-brained a production memory store.
    let home = std::env::var("HOME")
        .map_err(|_| anyhow::anyhow!("set --db, $ECPHORY_DB, or $HOME"))?;
    Ok(PathBuf::from(home).join(".local/share/ecphory/ecphory.redb"))
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .with_writer(std::io::stderr)
        .init();

    let cli = Cli::parse();

    // Eval is HTTP-only by design: it scores the live daemon's real client
    // path and must never open the store (redb's lock is process-exclusive).
    if let Command::Eval { gold, from_log, url, k, min_mrr, window } = &cli.command {
        let token = std::env::var("ECPHORY_AUTH_TOKEN").ok().filter(|t| !t.is_empty());
        let client = eval::EvalClient::new(url.clone(), token);
        let mut ok = true;
        if let Some(gold_path) = gold {
            let pairs = eval::parse_gold(gold_path)?;
            ok &= eval::run_gold(&client, &pairs, *k, *min_mrr)?;
        }
        if *from_log {
            ok &= eval::run_from_log(&client, *k, *window)?;
        }
        if gold.is_none() && !from_log {
            anyhow::bail!("eval needs --gold <file> and/or --from-log");
        }
        if !ok {
            std::process::exit(1);
        }
        return Ok(());
    }

    let mut svc = Ecphory::open(db_path(cli.db)?)?;

    match cli.command {
        Command::Add { content, name, source, phrases, tags } => {
            let mut ep = Episode::new(content, source);
            ep.name = name;
            ep.search_phrases = phrases;
            ep.tags = tags;
            svc.insert(&ep)?;
            println!("{}", serde_json::to_string_pretty(&ep)?);
        }
        Command::Search { query, limit, include_deleted, group, source, tags, brief } => {
            let out = svc.search(
                &query,
                &SearchOptions { limit, include_deleted, group_id: group, source, tags },
            )?;
            if brief {
                eprintln!(
                    "{} hits in {:.2}ms",
                    out.results.len(),
                    out.latency_us as f64 / 1000.0
                );
                for r in &out.results {
                    let id = r.episode.id.to_string();
                    println!(
                        "{:>2}. {}  {:>7.3}  {}",
                        r.rank,
                        &id[..8],
                        r.score,
                        r.episode.name.as_deref().unwrap_or("(unnamed)")
                    );
                }
            } else {
                let eps: Vec<_> = out
                    .results
                    .iter()
                    .map(|r| {
                        serde_json::json!({
                            "rank": r.rank,
                            "score": r.score,
                            "episode": r.episode,
                        })
                    })
                    .collect();
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "count": eps.len(),
                        "latency_ms": out.latency_us as f64 / 1000.0,
                        "results": eps,
                    }))?
                );
            }
        }
        Command::Get { id } => {
            let ep = svc.get(&id)?;
            println!("{}", serde_json::to_string_pretty(&ep)?);
        }
        Command::List { limit, include_deleted } => {
            let eps = svc.list(ListOptions { limit, include_deleted, ..Default::default() })?;
            println!("{}", serde_json::to_string_pretty(&eps)?);
        }
        Command::Update { id, content, name, phrases, tags } => {
            let params = UpdateParams {
                content,
                name,
                search_phrases: if phrases.is_empty() { None } else { Some(phrases) },
                tags: if tags.is_empty() { None } else { Some(tags) },
                ..Default::default()
            };
            let ep = svc.update(&id, params)?;
            println!("{}", serde_json::to_string_pretty(&ep)?);
        }
        Command::Demote { id } => {
            let ep = svc.demote(&id)?;
            println!("demoted {} (recoverable via restore)", ep.id);
        }
        Command::Restore { id } => {
            let ep = svc.restore(&id)?;
            println!("restored {}", ep.id);
        }
        Command::Versions { id } => {
            let versions = svc.versions(&id)?;
            println!("{}", serde_json::to_string_pretty(&versions)?);
        }
        Command::Import { dir } => {
            let started = std::time::Instant::now();
            let read = import::read_mirror(&dir)?;
            let parsed = read.episodes.len();
            let inserted = svc.import(read.episodes)?;
            println!(
                "parsed {parsed} episodes, imported {inserted} new ({} already present) in {:.1}s",
                parsed - inserted,
                started.elapsed().as_secs_f64()
            );
            for (path, reason) in &read.skipped {
                eprintln!("skipped {path}: {reason}");
            }
        }
        Command::Reindex => {
            let started = std::time::Instant::now();
            let n = svc.reindex()?;
            println!("reindexed {n} episodes in {:.2}s", started.elapsed().as_secs_f64());
        }
        Command::Status => {
            println!("episodes: {}", svc.count()?);
        }
        Command::Stats => {
            let s = svc.stats()?;
            println!("{}", serde_json::to_string_pretty(&s)?);
        }
        Command::SearchLog { limit } => {
            for e in svc.recent_searches(limit)? {
                println!(
                    "{}  {:>7.2}ms  {:>3} hits  {:?}",
                    e.ts.format("%Y-%m-%d %H:%M:%S"),
                    e.latency_us as f64 / 1000.0,
                    e.result_count,
                    e.query
                );
            }
        }
        Command::AccessLog { limit } => {
            for e in svc.recent_accesses(limit)? {
                println!("{}  {}", e.ts.format("%Y-%m-%d %H:%M:%S"), e.episode_id);
            }
        }
        Command::Mcp => {
            mcp::serve_stdio(svc)?;
        }
        Command::Serve { port } => {
            let port = port
                .or_else(|| std::env::var("ECPHORY_PORT").ok().and_then(|p| p.parse().ok()))
                .unwrap_or(3491);
            mcp::serve_http(svc, port)?;
        }
        Command::Eval { .. } => unreachable!("handled before store open"),
    }
    Ok(())
}
