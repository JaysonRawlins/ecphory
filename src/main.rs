mod error;
mod eval;
mod export;
mod harness;
mod http;
mod import;
mod index;
mod install;
mod mcp;
mod model;
mod recorder;
mod service;
mod store;
mod triggers;

use std::path::PathBuf;

use clap::{Parser, Subcommand};

use crate::model::{Episode, UpdateParams};
use crate::service::{Ecphory, PurgeManifest, SearchOptions};
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
        /// Restrict to one group. Hidden groups (ECPHORY_HIDDEN_GROUPS) are
        /// skipped unless named here.
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
    /// Operator hard-delete of DEMOTED episodes: destroys the record, its
    /// archived versions, its index entries, and its git-mirror file.
    /// Dry-run by default; CLI-only by design (no MCP/REST equivalent)
    Purge {
        /// Episode ids or unique prefixes (must already be demoted)
        #[arg(required = true)]
        ids: Vec<String>,
        /// Actually destroy. Without this, prints the manifest of what
        /// would be destroyed and exits
        #[arg(long)]
        yes: bool,
    },
    /// Show the archived version history of an episode
    Versions { id: String },
    /// Restore an archived version, archiving the displaced current state
    Rollback { id: String, version_id: String },
    /// Import episodes from an engram git-export mirror directory
    Import {
        #[arg(long)]
        dir: PathBuf,
    },
    /// Export all episodes to a git mirror (offline; the daemon also
    /// exports on a schedule when ECPHORY_EXPORT_DIR is set)
    Export {
        #[arg(long)]
        dir: PathBuf,
        /// Commit the mirror after writing
        #[arg(long)]
        commit: bool,
    },
    /// Rebuild the search index from the store (recovery / schema change)
    Reindex,
    /// Store status
    Status,
    /// Wire each installed harness to this store. Dry-run unless --apply.
    Install {
        /// Write the planned changes. Without it, install only prints the plan.
        #[arg(long)]
        apply: bool,
    },
    /// Verify that ecphory's context actually reaches each installed agent harness
    Doctor {
        /// Prove delivery by invoking each harness with a single-use canary.
        /// Costs one model call per harness — the honest price of the only
        /// claim that means anything. Without it, nothing reports DELIVERED.
        #[arg(long)]
        live: bool,
    },
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
    /// Recent explicit search ratings (the consumer's verdicts), newest first
    Ratings {
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    /// Heal resolutions (validated miss self-corrections), newest first.
    /// Ratings are immutable; these are the layer that records which misses
    /// were closed, and how the last replay pass went.
    Heals {
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
        /// Heal-replay regression pass: re-run every healed miss's original
        /// query against the live index and verify the intended episode
        /// still ranks within top k. Exits non-zero if any heal regressed.
        #[arg(long)]
        heals: bool,
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
    #[cfg(windows)]
    {
        let base = std::env::var("LOCALAPPDATA")
            .map_err(|_| anyhow::anyhow!("set --db, %ECPHORY_DB%, or %LOCALAPPDATA%"))?;
        Ok(PathBuf::from(base).join("ecphory").join("ecphory.redb"))
    }
    #[cfg(not(windows))]
    {
        let home = std::env::var("HOME")
            .map_err(|_| anyhow::anyhow!("set --db, $ECPHORY_DB, or $HOME"))?;
        Ok(PathBuf::from(home).join(".local/share/ecphory/ecphory.redb"))
    }
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .with_writer(std::io::stderr)
        .init();

    let cli = Cli::parse();

    // Eval is HTTP-only by design: it scores the live daemon's real client
    // path and must never open the store (redb's lock is process-exclusive).
    if let Command::Eval {
        gold,
        from_log,
        heals,
        url,
        k,
        min_mrr,
        window,
    } = &cli.command
    {
        let token = std::env::var("ECPHORY_AUTH_TOKEN")
            .ok()
            .filter(|t| !t.is_empty());
        let client = eval::EvalClient::new(url.clone(), token);
        let mut ok = true;
        if let Some(gold_path) = gold {
            let pairs = eval::parse_gold(gold_path)?;
            ok &= eval::run_gold(&client, &pairs, *k, *min_mrr)?;
        }
        if *from_log {
            ok &= eval::run_from_log(&client, *k, *window)?;
        }
        if *heals {
            ok &= eval::run_heals(&client, *k)?;
        }
        if gold.is_none() && !from_log && !heals {
            anyhow::bail!("eval needs --gold <file>, --from-log, and/or --heals");
        }
        if !ok {
            std::process::exit(1);
        }
        return Ok(());
    }

    // Doctor reads harness configuration from disk and must never open the
    // store: redb's lock is process-exclusive, and the daemon is normally
    // running at exactly the moment an operator wants to run doctor.
    if let Command::Install { apply } = cli.command {
        let home = std::env::var("HOME")
            .map(PathBuf::from)
            .map_err(|_| anyhow::anyhow!("HOME is not set"))?;
        let plans = install::plan(&home);
        if plans.is_empty() {
            println!(
                "no supported agent harness detected under {}",
                home.display()
            );
            return Ok(());
        }
        for p in &plans {
            println!("{}", p.render());
        }
        println!();
        if apply {
            anyhow::bail!("--apply is not implemented yet; the plan above was not written");
        }
        println!("Dry run: nothing was written. Re-run with --apply to make these changes.");
        return Ok(());
    }

    if let Command::Doctor { live } = cli.command {
        let home = std::env::var("HOME")
            .map(PathBuf::from)
            .map_err(|_| anyhow::anyhow!("HOME is not set"))?;
        let reports = if live {
            harness::live_report(&home)
        } else {
            harness::static_report(&home)
        };
        if reports.is_empty() {
            println!(
                "no supported agent harness detected under {}",
                home.display()
            );
            return Ok(());
        }
        for r in &reports {
            println!("{r}");
        }
        if !live {
            println!();
            println!(
                "Static inspection cannot prove delivery. Four of the five known \
silent-failure modes pass every static check, so a config that parses is not \
evidence that text reached the model. Run `ecphory doctor --live` to settle it."
            );
        }
        return Ok(());
    }

    let mut svc = Ecphory::open(db_path(cli.db)?)?;

    match cli.command {
        Command::Add {
            content,
            name,
            source,
            phrases,
            tags,
        } => {
            let mut ep = Episode::new(content, source);
            ep.name = name;
            ep.search_phrases = phrases;
            ep.tags = tags;
            svc.insert(&ep)?;
            println!("{}", serde_json::to_string_pretty(&ep)?);
        }
        Command::Search {
            query,
            limit,
            include_deleted,
            group,
            source,
            tags,
            brief,
        } => {
            let out = svc.search(
                &query,
                &SearchOptions {
                    limit,
                    include_deleted,
                    group_id: group,
                    source,
                    tags,
                },
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
        Command::List {
            limit,
            include_deleted,
        } => {
            let eps = svc.list(ListOptions {
                limit,
                include_deleted,
                ..Default::default()
            })?;
            println!("{}", serde_json::to_string_pretty(&eps)?);
        }
        Command::Update {
            id,
            content,
            name,
            phrases,
            tags,
        } => {
            let params = UpdateParams {
                content,
                name,
                search_phrases: if phrases.is_empty() {
                    None
                } else {
                    Some(phrases)
                },
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
        Command::Purge { ids, yes } => {
            let export_dir = std::env::var("ECPHORY_EXPORT_DIR")
                .ok()
                .filter(|d| !d.is_empty())
                .map(PathBuf::from);
            let mirror_path = |m: &PurgeManifest| {
                export_dir.as_ref().map(|d| {
                    d.join(&m.episode.group_id)
                        .join(format!("{}.md", m.episode.id))
                })
            };

            // Validate every target before destroying anything: one bad id
            // (unknown, ambiguous, not demoted) fails the whole batch with
            // nothing touched.
            let mut manifests: Vec<PurgeManifest> = Vec::with_capacity(ids.len());
            for id in &ids {
                let m = svc.purge_manifest(id)?;
                // The same episode named twice (prefix + full id) is one purge.
                if !manifests.iter().any(|seen| seen.episode.id == m.episode.id) {
                    manifests.push(m);
                }
            }

            for (i, m) in manifests.iter().enumerate() {
                if i > 0 {
                    println!();
                }
                println!("episode {}", m.episode.id);
                println!(
                    "  name:       {}",
                    m.episode.name.as_deref().unwrap_or("(unnamed)")
                );
                println!("  created_at: {}", m.episode.created_at.to_rfc3339());
                println!("  versions:   {}", m.version_count);
                println!("  indexed:    {}", if m.indexed { "yes" } else { "no" });
                match mirror_path(m) {
                    Some(p) if p.exists() => println!("  mirror:     {}", p.display()),
                    Some(p) => println!("  mirror:     {} (not present)", p.display()),
                    None => println!("  mirror:     (ECPHORY_EXPORT_DIR not set)"),
                }
            }

            if !yes {
                println!();
                println!(
                    "dry run: nothing destroyed; re-run with --yes to purge {} episode(s)",
                    manifests.len()
                );
                return Ok(());
            }

            let mut mirror_removed = false;
            for m in &manifests {
                let (ep, versions_removed) = svc.purge(&m.episode.id.to_string())?;
                println!(
                    "purged {}: record, {versions_removed} archived version(s), and index entry destroyed",
                    ep.id
                );
                if let Some(path) = mirror_path(m)
                    && path.exists()
                {
                    std::fs::remove_file(&path).map_err(|e| {
                        anyhow::anyhow!("removing mirror file {}: {e}", path.display())
                    })?;
                    println!("removed mirror file {}", path.display());
                    mirror_removed = true;
                }
            }
            // Commit the removals only when the mirror is already a git repo:
            // the daemon's scheduled export auto-commits, so a committed
            // mirror stays committed. Purge never git-inits a mirror that
            // was a plain directory.
            if let Some(dir) = &export_dir
                && mirror_removed
                && dir.join(".git").exists()
            {
                export::git_commit(dir, "ecphory purge")?;
                println!("committed mirror removal in {}", dir.display());
            }
            println!(
                "note: flight-recorder rows referencing purged ids remain (ids/ranks only, no \
                 content), and the git mirror's HISTORY still contains the content; see the \
                 README's \"Deletion story\" section"
            );
        }
        Command::Versions { id } => {
            let versions = svc.versions(&id)?;
            println!("{}", serde_json::to_string_pretty(&versions)?);
        }
        Command::Rollback { id, version_id } => {
            let ep = svc.restore_version(&id, &version_id)?;
            println!("restored {} from archived version {version_id}", ep.id);
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
        Command::Export { dir, commit } => {
            let episodes = svc.export_all()?;
            let outcome = export::write_mirror(&episodes, &dir)?;
            let committed = if commit {
                export::git_commit(&dir, "ecphory export")?
            } else {
                false
            };
            let pushed = if committed && export::push_enabled() {
                match export::git_push(&dir) {
                    Ok(p) => p,
                    Err(e) => {
                        eprintln!("push failed: {e}");
                        false
                    }
                }
            } else {
                false
            };
            if committed && let Some(engine) = svc.triggers_engine() {
                engine.fire_store("export", &dir);
            }
            println!(
                "exported {} episodes: {} written, {} unchanged, committed={committed} pushed={pushed}",
                episodes.len(),
                outcome.written,
                outcome.unchanged
            );
        }
        Command::Reindex => {
            let started = std::time::Instant::now();
            let n = svc.reindex()?;
            println!(
                "reindexed {n} episodes in {:.2}s",
                started.elapsed().as_secs_f64()
            );
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
        Command::Ratings { limit } => {
            for e in svc.recent_ratings(limit)? {
                println!(
                    "{}  {:<7}  search {}  used [{}]{}",
                    e.ts.format("%Y-%m-%d %H:%M:%S"),
                    format!("{:?}", e.rating).to_lowercase(),
                    &e.search_id[..8.min(e.search_id.len())],
                    e.used_episode_ids
                        .iter()
                        .map(|id| &id[..8.min(id.len())])
                        .collect::<Vec<_>>()
                        .join(", "),
                    e.note
                        .as_deref()
                        .map(|n| format!("  — {n}"))
                        .unwrap_or_default()
                );
            }
        }
        Command::Heals { limit } => {
            for r in svc.recent_resolutions(limit)? {
                let replay = match &r.last_replay {
                    None => "unreplayed".to_string(),
                    Some(o) if o.held => format!(
                        "held (rank {})",
                        o.rank.map(|n| n.to_string()).unwrap_or_else(|| "?".into())
                    ),
                    Some(_) => "REGRESSED".to_string(),
                };
                println!(
                    "{}  {:<20}  {:?} -> {}  validated {}{}",
                    r.ts.format("%Y-%m-%d %H:%M:%S"),
                    replay,
                    r.query,
                    &r.episode_id[..8.min(r.episode_id.len())],
                    r.validated_rank,
                    if r.displaced_used.is_empty() {
                        String::new()
                    } else {
                        format!("  displaced [{}]", r.displaced_used.join(", "))
                    }
                );
            }
        }
        Command::Mcp => {
            mcp::serve_stdio(svc)?;
        }
        Command::Serve { port } => {
            let port = port
                .or_else(|| {
                    std::env::var("ECPHORY_PORT")
                        .ok()
                        .and_then(|p| p.parse().ok())
                })
                .unwrap_or(3491);
            mcp::serve_http(svc, port)?;
        }
        Command::Eval { .. } => unreachable!("handled before store open"),
        Command::Doctor { .. } => unreachable!("handled before store open"),
        Command::Install { .. } => unreachable!("handled before store open"),
    }
    Ok(())
}
