use std::sync::{Arc, Mutex};

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{Implementation, ServerCapabilities, ServerInfo};
use rmcp::{tool, tool_handler, tool_router, ErrorData, ServerHandler};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::model::{Episode, UpdateParams};
use crate::service::{Ecphory, SearchOptions};
use crate::store::ListOptions;

/// MCP surface. Tool names and shapes mirror engram's so client muscle
/// memory transfers; the deliberate differences are `search_phrases` on
/// writes (write-time lexical enrichment) and no vector/hybrid modes.
pub struct McpServer {
    // ONE Ecphory per process (redb's lock is process-exclusive — the M4
    // live gate proved two processes cannot share the store). The HTTP
    // transport creates a handler per client session; they all share this.
    // Tool handlers take &self; index writes need &mut — a plain mutex,
    // never held across an await.
    svc: Arc<Mutex<Ecphory>>,
    tool_router: ToolRouter<Self>,
}

fn internal(e: impl std::fmt::Display) -> ErrorData {
    ErrorData::internal_error(e.to_string(), None)
}

fn not_found(e: impl std::fmt::Display) -> ErrorData {
    ErrorData::invalid_params(e.to_string(), None)
}

fn to_json<T: serde::Serialize>(v: &T) -> Result<String, ErrorData> {
    serde_json::to_string(v).map_err(internal)
}

// Request structs use plain defaulted types, NEVER Option<T>: schemars
// (draft 2020-12, hardcoded by rmcp per the MCP spec) renders Option as
// `"type": ["string","null"]` union arrays, which Claude Code's schema
// validator rejects — tools silently vanish ("tools fetch failed").
// Empty string / empty vec / zero / Null mean "not provided".

#[derive(Deserialize, JsonSchema)]
pub struct AddMemoryRequest {
    /// The memory content, stored verbatim (never rewritten server-side).
    pub content: String,
    /// Short human-readable name for the episode.
    #[serde(default)]
    pub name: String,
    /// 2-3 paraphrase search cues: how someone would ask for this when they
    /// don't know the answer's vocabulary. These are boosted in search.
    #[serde(default)]
    pub search_phrases: Vec<String>,
    /// Originating system, e.g. "claude-code".
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub source_model: String,
    #[serde(default)]
    pub source_description: String,
    #[serde(default)]
    pub group_id: String,
    #[serde(default)]
    pub tags: Vec<String>,
    /// Arbitrary JSON metadata object.
    #[serde(default)]
    pub metadata: serde_json::Value,
}

#[derive(Deserialize, JsonSchema)]
pub struct SearchRequest {
    /// Query text (BM25 over content, names, and search phrases).
    pub query: String,
    /// Maximum results (default 10).
    #[serde(default)]
    pub max_results: usize,
    #[serde(default)]
    pub group_id: String,
    #[serde(default)]
    pub source: String,
    /// All listed tags must be present.
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub include_deleted: bool,
}

#[derive(Deserialize, JsonSchema)]
pub struct IdRequest {
    /// Full episode UUID or a unique prefix (8+ chars is usually enough).
    pub id: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct GetEpisodeRequest {
    /// Full episode UUID or a unique prefix (8+ chars is usually enough).
    pub id: String,
    /// Set true for bulk/maintenance reads (backfills, sync sweeps, mass
    /// exports) so they don't pollute the used-signal in the flight recorder.
    /// Leave false when fetching an episode you actually want to read.
    #[serde(default)]
    pub no_record: bool,
}

#[derive(Deserialize, JsonSchema)]
pub struct RateSearchRequest {
    /// The search_id returned by a previous search call.
    pub search_id: String,
    /// Verdict on that search's results: "hit" (answered the question),
    /// "partial" (something useful but not the best answer or badly ranked),
    /// or "miss" (nothing relevant).
    pub rating: String,
    /// Episode ids (full or prefix) from the results that were actually used.
    #[serde(default)]
    pub used_episode_ids: Vec<String>,
    /// Optional context, e.g. what was actually being looked for on a miss.
    #[serde(default)]
    pub note: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct GetEpisodesRequest {
    /// Maximum results, newest first (default 10).
    #[serde(default)]
    pub max_results: usize,
    #[serde(default)]
    pub include_deleted: bool,
}

#[derive(Deserialize, JsonSchema)]
pub struct UpdateEpisodeRequest {
    /// Full episode UUID or a unique prefix.
    pub id: String,
    /// New content; empty means leave unchanged.
    #[serde(default)]
    pub content: String,
    /// New name; empty means leave unchanged.
    #[serde(default)]
    pub name: String,
    /// Replacement search phrases; empty means leave unchanged.
    #[serde(default)]
    pub search_phrases: Vec<String>,
    /// Replacement tags; empty means leave unchanged.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Replacement metadata; JSON null means leave unchanged.
    #[serde(default)]
    pub metadata: serde_json::Value,
}

#[derive(Deserialize, JsonSchema)]
pub struct DeleteEpisodeRequest {
    /// Full episode UUID or a unique prefix.
    pub id: String,
    /// Hard deletion is refused for agents; demote is the only agent path.
    #[serde(default)]
    pub hard: bool,
}

fn opt_str(s: String) -> Option<String> {
    if s.is_empty() { None } else { Some(s) }
}

fn opt_vec(v: Vec<String>) -> Option<Vec<String>> {
    if v.is_empty() { None } else { Some(v) }
}

#[tool_router]
impl McpServer {
    pub fn new(svc: Arc<Mutex<Ecphory>>) -> Self {
        Self { svc, tool_router: Self::tool_router() }
    }

    #[tool(
        description = "Store a new memory episode. Content is stored verbatim. Include 2-3 search_phrases: paraphrase cues for how this will be asked about later, in different vocabulary than the content."
    )]
    fn add_memory(
        &self,
        Parameters(req): Parameters<AddMemoryRequest>,
    ) -> Result<String, ErrorData> {
        let source = if req.source.is_empty() { "mcp".to_string() } else { req.source };
        let mut ep = Episode::new(req.content, source);
        ep.name = opt_str(req.name);
        ep.search_phrases = req.search_phrases;
        ep.source_model = opt_str(req.source_model);
        ep.source_description = opt_str(req.source_description);
        if !req.group_id.is_empty() {
            ep.group_id = req.group_id;
        }
        ep.tags = req.tags;
        ep.metadata = req.metadata;

        let mut svc = self.svc.lock().map_err(internal)?;
        svc.insert(&ep).map_err(internal)?;
        to_json(&serde_json::json!({ "success": true, "id": ep.id }))
    }

    #[tool(
        description = "Search memories (BM25 over content, names, and search phrases). Returns ranked episodes with scores, plus a search_id — after you've read the results and know whether they answered the question, pass that search_id to rate_search."
    )]
    fn search(&self, Parameters(req): Parameters<SearchRequest>) -> Result<String, ErrorData> {
        let svc = self.svc.lock().map_err(internal)?;
        let out = svc
            .search(
                &req.query,
                &SearchOptions {
                    limit: if req.max_results == 0 { 10 } else { req.max_results },
                    include_deleted: req.include_deleted,
                    group_id: opt_str(req.group_id),
                    source: opt_str(req.source),
                    tags: req.tags,
                },
            )
            .map_err(internal)?;
        let results: Vec<_> = out
            .results
            .iter()
            .map(|r| {
                serde_json::json!({ "rank": r.rank, "score": r.score, "episode": r.episode })
            })
            .collect();
        to_json(&serde_json::json!({
            "count": results.len(),
            "latency_ms": out.latency_us as f64 / 1000.0,
            "search_id": out.search_id,
            "results": results,
        }))
    }

    #[tool(
        description = "Rate a previous search by its search_id: was the retrieval a hit, partial, or miss? Call this after consuming search results — the moment you know whether they answered the question. Explicit ratings are the store's primary retrieval-quality signal (the query->rate->work loop); include used_episode_ids for the results you actually relied on."
    )]
    fn rate_search(
        &self,
        Parameters(req): Parameters<RateSearchRequest>,
    ) -> Result<String, ErrorData> {
        let rating: crate::recorder::Rating =
            req.rating.parse().map_err(|e: String| ErrorData::invalid_params(e, None))?;
        let svc = self.svc.lock().map_err(internal)?;
        let entry = svc
            .rate_search(&req.search_id, rating, req.used_episode_ids, opt_str(req.note))
            .map_err(not_found)?;
        to_json(&serde_json::json!({ "success": true, "rating_id": entry.id }))
    }

    #[tool(
        description = "Fetch one episode by id or unique id prefix. Returns soft-deleted episodes too (flagged with deleted_at) so id references in handoffs never break. Set no_record=true for bulk/maintenance reads that shouldn't count as usage signal."
    )]
    fn get_episode(
        &self,
        Parameters(req): Parameters<GetEpisodeRequest>,
    ) -> Result<String, ErrorData> {
        let svc = self.svc.lock().map_err(internal)?;
        let ep = if req.no_record {
            svc.get_unrecorded(&req.id).map_err(not_found)?
        } else {
            svc.get(&req.id).map_err(not_found)?
        };
        to_json(&ep)
    }

    #[tool(description = "List recent episodes, newest first.")]
    fn get_episodes(
        &self,
        Parameters(req): Parameters<GetEpisodesRequest>,
    ) -> Result<String, ErrorData> {
        let svc = self.svc.lock().map_err(internal)?;
        let eps = svc
            .list(ListOptions {
                limit: if req.max_results == 0 { 10 } else { req.max_results },
                include_deleted: req.include_deleted,
                ..Default::default()
            })
            .map_err(internal)?;
        to_json(&serde_json::json!({ "count": eps.len(), "episodes": eps }))
    }

    #[tool(
        description = "Update an episode's fields. The prior state is archived and recoverable."
    )]
    fn update_episode(
        &self,
        Parameters(req): Parameters<UpdateEpisodeRequest>,
    ) -> Result<String, ErrorData> {
        let mut svc = self.svc.lock().map_err(internal)?;
        let ep = svc
            .update(
                &req.id,
                UpdateParams {
                    content: opt_str(req.content),
                    name: opt_str(req.name),
                    search_phrases: opt_vec(req.search_phrases),
                    tags: opt_vec(req.tags),
                    expired_at: None,
                    metadata: if req.metadata.is_null() { None } else { Some(req.metadata) },
                },
            )
            .map_err(not_found)?;
        to_json(&ep)
    }

    #[tool(
        description = "Demote (soft-delete) an episode: hidden from search, recoverable via restore_episode. Hard deletion is not available to agents; it requires the operator CLI."
    )]
    fn delete_episode(
        &self,
        Parameters(req): Parameters<DeleteEpisodeRequest>,
    ) -> Result<String, ErrorData> {
        if req.hard {
            return Err(ErrorData::invalid_params(
                crate::error::Error::HardDeleteRefused.to_string(),
                None,
            ));
        }
        let mut svc = self.svc.lock().map_err(internal)?;
        let ep = svc.demote(&req.id).map_err(not_found)?;
        to_json(&serde_json::json!({
            "success": true,
            "id": ep.id,
            "message": "episode demoted (recoverable via restore_episode)",
        }))
    }

    #[tool(description = "Restore a previously demoted episode.")]
    fn restore_episode(
        &self,
        Parameters(req): Parameters<IdRequest>,
    ) -> Result<String, ErrorData> {
        let mut svc = self.svc.lock().map_err(internal)?;
        let ep = svc.restore(&req.id).map_err(not_found)?;
        to_json(&serde_json::json!({ "success": true, "id": ep.id }))
    }

    #[tool(description = "Archived prior states of an episode, oldest first.")]
    fn get_episode_versions(
        &self,
        Parameters(req): Parameters<IdRequest>,
    ) -> Result<String, ErrorData> {
        let svc = self.svc.lock().map_err(internal)?;
        let versions = svc.versions(&req.id).map_err(not_found)?;
        to_json(&serde_json::json!({ "count": versions.len(), "versions": versions }))
    }

    #[tool(description = "Store status: episode count and recorder aggregates.")]
    fn get_status(&self) -> Result<String, ErrorData> {
        let svc = self.svc.lock().map_err(internal)?;
        let stats = svc.stats().map_err(internal)?;
        to_json(&serde_json::json!({
            "status": "operational",
            "episodes": svc.count().map_err(internal)?,
            "recorder": stats,
        }))
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for McpServer {
    fn get_info(&self) -> ServerInfo {
        let mut identity = Implementation::from_build_env();
        identity.name = "ecphory".into();
        identity.version = env!("CARGO_PKG_VERSION").into();
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(identity)
            .with_instructions(
            "ecphory is a lexical-first (BM25) memory store. When storing a memory, always \
             include 2-3 search_phrases: plain-words paraphrases of how this memory will be \
             asked about later, in vocabulary DIFFERENT from the content (symptom framings, \
             questions a future session would ask). Retrieval quality depends on them. \
             Search accepts free text; episode ids resolve by unique prefix. \
             After consuming search results — the moment you know whether they answered the \
             question — call rate_search with the returned search_id (hit/partial/miss, plus \
             used_episode_ids for results you relied on): query->rate->work. For bulk or \
             maintenance reads, pass no_record=true to get_episode so they don't pollute \
             the usage signal.",
        )
    }
}

/// Parse "24h" / "30m" / "90s" / "1d" style intervals.
fn parse_interval(raw: &str) -> Option<std::time::Duration> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    let (num, unit) = raw.split_at(raw.len() - 1);
    let n: u64 = num.parse().ok()?;
    let secs = match unit {
        "s" => n,
        "m" => n * 60,
        "h" => n * 3600,
        "d" => n * 86400,
        _ => return None,
    };
    Some(std::time::Duration::from_secs(secs.max(60)))
}

/// Serve MCP over stdio until the client disconnects. Single-client only:
/// a second process cannot open the store (redb lock). Prefer `serve`.
pub fn serve_stdio(svc: Ecphory) -> anyhow::Result<()> {
    let runtime = tokio::runtime::Runtime::new()?;
    runtime.block_on(async {
        use rmcp::ServiceExt;
        let service = McpServer::new(Arc::new(Mutex::new(svc)))
            .serve(rmcp::transport::stdio())
            .await?;
        service.waiting().await?;
        Ok(())
    })
}

/// Serve MCP over streamable HTTP + the REST mirror: one daemon owns the
/// store lock, any number of client sessions connect concurrently. This is
/// the fix for the M4 gate finding (stdio-per-session collides on redb's
/// exclusive lock).
pub fn serve_http(svc: Ecphory, port: u16) -> anyhow::Result<()> {
    use rmcp::transport::streamable_http_server::{
        session::local::LocalSessionManager, StreamableHttpServerConfig, StreamableHttpService,
    };

    let shared = Arc::new(Mutex::new(svc));
    let token = std::env::var("ECPHORY_AUTH_TOKEN").ok().filter(|t| !t.is_empty());
    if token.is_some() {
        tracing::info!("bearer auth enabled (ECPHORY_AUTH_TOKEN)");
    }
    let state = Arc::new(crate::http::AppState { svc: shared.clone(), token });

    let runtime = tokio::runtime::Runtime::new()?;
    runtime.block_on(async {
        // Scheduled git-mirror export (the durability layer): first tick
        // fires immediately (export at boot), then every interval. The
        // lock is held only to snapshot episodes, never across file/git IO.
        if let Ok(dir) = std::env::var("ECPHORY_EXPORT_DIR")
            && !dir.is_empty()
        {
            {
                let export_svc = state.svc.clone();
                let interval = parse_interval(
                    &std::env::var("ECPHORY_EXPORT_INTERVAL").unwrap_or_default(),
                )
                .unwrap_or(std::time::Duration::from_secs(24 * 3600));
                tokio::spawn(async move {
                    let mut tick = tokio::time::interval(interval);
                    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
                    loop {
                        tick.tick().await;
                        let episodes = match export_svc.lock().expect("service lock").export_all() {
                            Ok(eps) => eps,
                            Err(e) => {
                                tracing::warn!("scheduled export: snapshot failed: {e}");
                                continue;
                            }
                        };
                        let path = std::path::PathBuf::from(&dir);
                        let result = tokio::task::spawn_blocking(move || {
                            let outcome = crate::export::write_mirror(&episodes, &path)?;
                            let committed =
                                crate::export::git_commit(&path, "ecphory scheduled export")?;
                            Ok::<_, crate::error::Error>((outcome, committed, episodes.len()))
                        })
                        .await;
                        match result {
                            Ok(Ok((outcome, committed, total))) => tracing::info!(
                                "scheduled export: {total} episodes, {} written, {} unchanged, committed={committed}",
                                outcome.written, outcome.unchanged
                            ),
                            Ok(Err(e)) => tracing::warn!("scheduled export failed: {e}"),
                            Err(e) => tracing::warn!("scheduled export task panicked: {e}"),
                        }
                    }
                });
            }
        }

        // Plain JSON request/response (MCP streamable HTTP, 2025-06-18):
        // no SSE framing, no priming events, no per-session server state.
        // Simple tools need none of it, and Claude Code's health check
        // stalled on the stateful mode's SSE priming events.
        let mut config = StreamableHttpServerConfig::default();
        config.stateful_mode = false;
        config.json_response = true;
        let service = StreamableHttpService::new(
            move || Ok(McpServer::new(shared.clone())),
            Arc::new(LocalSessionManager::default()),
            config,
        );
        // Claude Code omits the spec-required Accept header on tools/list
        // (anthropics/claude-code#30426), which strict streamable-HTTP
        // servers 406 — the failure is silent client-side ("tools fetch
        // failed"). Be liberal: inject the header on every inbound request.
        let router = crate::http::build_router(state).nest_service(
            "/mcp",
            axum::Router::new().fallback_service(service).layer(
                axum::middleware::map_request(
                    |mut req: axum::http::Request<axum::body::Body>| async {
                        req.headers_mut().insert(
                            axum::http::header::ACCEPT,
                            axum::http::HeaderValue::from_static(
                                "application/json, text/event-stream",
                            ),
                        );
                        req
                    },
                ),
            ),
        );
        // Loopback only: single-user local daemon, no remote surface.
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", port)).await?;
        tracing::info!("ecphory MCP listening on http://127.0.0.1:{port}/mcp");
        axum::serve(listener, router).await?;
        Ok(())
    })
}
