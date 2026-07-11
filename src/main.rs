mod error;
mod model;
mod store;

use std::path::PathBuf;

use clap::{Parser, Subcommand};

use crate::model::{Episode, UpdateParams};
use crate::store::{ListOptions, Store};

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
    /// Store status
    Status,
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
    let store = Store::open(db_path(cli.db)?)?;

    match cli.command {
        Command::Add { content, name, source, phrases, tags } => {
            let mut ep = Episode::new(content, source);
            ep.name = name;
            ep.search_phrases = phrases;
            ep.tags = tags;
            store.insert(&ep)?;
            println!("{}", serde_json::to_string_pretty(&ep)?);
        }
        Command::Get { id } => {
            let ep = store.get(&id)?;
            println!("{}", serde_json::to_string_pretty(&ep)?);
        }
        Command::List { limit, include_deleted } => {
            let eps = store.list(ListOptions { limit, include_deleted, ..Default::default() })?;
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
            let ep = store.update(&id, params)?;
            println!("{}", serde_json::to_string_pretty(&ep)?);
        }
        Command::Demote { id } => {
            let ep = store.demote(&id)?;
            println!("demoted {} (recoverable via restore)", ep.id);
        }
        Command::Restore { id } => {
            let ep = store.restore(&id)?;
            println!("restored {}", ep.id);
        }
        Command::Versions { id } => {
            let versions = store.versions(&id)?;
            println!("{}", serde_json::to_string_pretty(&versions)?);
        }
        Command::Status => {
            println!("episodes: {}", store.count()?);
        }
    }
    Ok(())
}
