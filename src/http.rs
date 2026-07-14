use std::sync::{Arc, Mutex};

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;

use crate::model::{Episode, UpdateParams};
use crate::service::{Ecphory, SearchOptions};
use crate::store::ListOptions;

/// REST mirror of the MCP surface, served by the same daemon (one process
/// owns the redb lock). Data plane is gated by the opt-in bearer token;
/// /health stays open for probes. Loopback binding is the outer wall —
/// auth is defense in depth for when the port is ever forwarded.
pub struct AppState {
    pub svc: Arc<Mutex<Ecphory>>,
    pub token: Option<String>,
}

type Shared = Arc<AppState>;

pub fn build_router(state: Shared) -> Router {
    let data_plane = Router::new()
        .route("/memory", post(add_memory))
        .route("/memory/search", get(search))
        .route("/memory/episodes", get(list_episodes))
        .route(
            "/memory/episodes/{id}",
            get(get_episode).put(update_episode).delete(demote_episode),
        )
        .route("/memory/episodes/{id}/restore", post(restore_episode))
        .route("/memory/episodes/{id}/versions", get(episode_versions))
        .route("/memory/search-log", get(search_log))
        .route("/memory/access-log", get(access_log))
        .route("/memory/rating-log", get(rating_log))
        .route("/memory/search-rating", post(rate_search))
        .route("/memory/stats", get(stats))
        .route("/status", get(status))
        .route("/admin/import", post(admin_import))
        .route("/admin/export", post(admin_export))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            require_auth,
        ));

    Router::new()
        .route(
            "/health",
            get(|| async { Json(json!({"status": "healthy"})) }),
        )
        .nest("/api/v1", data_plane)
        .with_state(state)
}

/// Bearer auth, opt-in via ECPHORY_AUTH_TOKEN. Constant-time comparison so
/// the check leaks nothing through timing.
async fn require_auth(
    State(state): State<Shared>,
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    let Some(expected) = &state.token else {
        return next.run(req).await; // auth not configured — open
    };
    let provided = req
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));
    match provided {
        Some(token) if constant_time_eq(token.as_bytes(), expected.as_bytes()) => {
            next.run(req).await
        }
        _ => err(StatusCode::UNAUTHORIZED, "missing or invalid bearer token"),
    }
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn err(code: StatusCode, message: impl Into<String>) -> Response {
    (
        code,
        Json(json!({"success": false, "error": message.into()})),
    )
        .into_response()
}

fn map_err(e: crate::error::Error) -> Response {
    use crate::error::Error;
    match &e {
        Error::NotFound(_) => err(StatusCode::NOT_FOUND, e.to_string()),
        Error::AmbiguousPrefix(_) | Error::HardDeleteRefused => {
            err(StatusCode::BAD_REQUEST, e.to_string())
        }
        _ => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

#[derive(Deserialize)]
struct AddMemoryBody {
    content: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    search_phrases: Vec<String>,
    #[serde(default)]
    source: String,
    #[serde(default)]
    source_model: String,
    #[serde(default)]
    source_description: String,
    #[serde(default)]
    group_id: String,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    metadata: serde_json::Value,
}

async fn add_memory(State(state): State<Shared>, Json(body): Json<AddMemoryBody>) -> Response {
    let source = if body.source.is_empty() {
        "http".to_string()
    } else {
        body.source
    };
    let mut ep = Episode::new(body.content, source);
    ep.name = none_if_empty(body.name);
    ep.search_phrases = body.search_phrases;
    ep.source_model = none_if_empty(body.source_model);
    ep.source_description = none_if_empty(body.source_description);
    if !body.group_id.is_empty() {
        ep.group_id = body.group_id;
    }
    ep.tags = body.tags;
    ep.metadata = body.metadata;

    let mut svc = state.svc.lock().expect("service lock");
    match svc.insert(&ep) {
        Ok(()) => (
            StatusCode::CREATED,
            Json(json!({"success": true, "episode": ep})),
        )
            .into_response(),
        Err(e) => map_err(e),
    }
}

fn none_if_empty(s: String) -> Option<String> {
    if s.is_empty() { None } else { Some(s) }
}

#[derive(Deserialize)]
struct SearchQuery {
    query: String,
    #[serde(default)]
    max_results: usize,
    #[serde(default)]
    group_id: Option<String>,
    #[serde(default)]
    source: Option<String>,
    /// Comma-separated; all must be present.
    #[serde(default)]
    tags: Option<String>,
    #[serde(default)]
    include_deleted: bool,
    /// Replay/benchmark bypass: skip the search log (eval replays and bulk
    /// sweeps are not workload signal and must not pollute the tape).
    #[serde(default)]
    no_record: bool,
}

async fn search(State(state): State<Shared>, Query(q): Query<SearchQuery>) -> Response {
    let tags = q
        .tags
        .map(|t| {
            t.split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default();
    let opts = SearchOptions {
        limit: if q.max_results == 0 {
            10
        } else {
            q.max_results
        },
        include_deleted: q.include_deleted,
        group_id: q.group_id.filter(|s| !s.is_empty()),
        source: q.source.filter(|s| !s.is_empty()),
        tags,
    };
    let svc = state.svc.lock().expect("service lock");
    let result = if q.no_record {
        svc.search_unrecorded(&q.query, &opts)
    } else {
        svc.search(&q.query, &opts)
    };
    match result {
        Ok(out) => {
            let results: Vec<_> = out
                .results
                .iter()
                .map(|r| json!({"rank": r.rank, "score": r.score, "episode": r.episode}))
                .collect();
            Json(json!({
                "count": results.len(),
                "latency_ms": out.latency_us as f64 / 1000.0,
                "search_id": out.search_id,
                "results": results,
            }))
            .into_response()
        }
        Err(e) => map_err(e),
    }
}

#[derive(Deserialize)]
struct ListQuery {
    #[serde(default)]
    max_results: usize,
    #[serde(default)]
    include_deleted: bool,
}

async fn list_episodes(State(state): State<Shared>, Query(q): Query<ListQuery>) -> Response {
    let svc = state.svc.lock().expect("service lock");
    match svc.list(ListOptions {
        limit: if q.max_results == 0 {
            10
        } else {
            q.max_results
        },
        include_deleted: q.include_deleted,
        ..Default::default()
    }) {
        Ok(eps) => Json(json!({"count": eps.len(), "episodes": eps})).into_response(),
        Err(e) => map_err(e),
    }
}

#[derive(Deserialize)]
struct GetQuery {
    /// Bulk-maintenance bypass: skip the access log (reads that are not
    /// usage signal must not pollute the used-signal join).
    #[serde(default)]
    no_record: bool,
}

async fn get_episode(
    State(state): State<Shared>,
    Path(id): Path<String>,
    Query(q): Query<GetQuery>,
) -> Response {
    let svc = state.svc.lock().expect("service lock");
    let result = if q.no_record {
        svc.get_unrecorded(&id)
    } else {
        svc.get(&id)
    };
    match result {
        Ok(ep) => Json(ep).into_response(),
        Err(e) => map_err(e),
    }
}

#[derive(Deserialize)]
struct UpdateBody {
    #[serde(default)]
    content: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    search_phrases: Vec<String>,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    metadata: serde_json::Value,
}

async fn update_episode(
    State(state): State<Shared>,
    Path(id): Path<String>,
    Json(body): Json<UpdateBody>,
) -> Response {
    let mut svc = state.svc.lock().expect("service lock");
    match svc.update(
        &id,
        UpdateParams {
            content: none_if_empty(body.content),
            name: none_if_empty(body.name),
            search_phrases: if body.search_phrases.is_empty() {
                None
            } else {
                Some(body.search_phrases)
            },
            tags: if body.tags.is_empty() {
                None
            } else {
                Some(body.tags)
            },
            expired_at: None,
            metadata: if body.metadata.is_null() {
                None
            } else {
                Some(body.metadata)
            },
        },
    ) {
        Ok(ep) => Json(ep).into_response(),
        Err(e) => map_err(e),
    }
}

async fn demote_episode(State(state): State<Shared>, Path(id): Path<String>) -> Response {
    let mut svc = state.svc.lock().expect("service lock");
    match svc.demote(&id) {
        Ok(ep) => Json(json!({
            "success": true,
            "id": ep.id,
            "message": "episode demoted (recoverable via restore)",
        }))
        .into_response(),
        Err(e) => map_err(e),
    }
}

async fn restore_episode(State(state): State<Shared>, Path(id): Path<String>) -> Response {
    let mut svc = state.svc.lock().expect("service lock");
    match svc.restore(&id) {
        Ok(ep) => Json(json!({"success": true, "id": ep.id})).into_response(),
        Err(e) => map_err(e),
    }
}

async fn episode_versions(State(state): State<Shared>, Path(id): Path<String>) -> Response {
    let svc = state.svc.lock().expect("service lock");
    match svc.versions(&id) {
        Ok(vs) => Json(json!({"count": vs.len(), "versions": vs})).into_response(),
        Err(e) => map_err(e),
    }
}

#[derive(Deserialize)]
struct LimitQuery {
    #[serde(default)]
    limit: usize,
}

async fn search_log(State(state): State<Shared>, Query(q): Query<LimitQuery>) -> Response {
    let svc = state.svc.lock().expect("service lock");
    match svc.recent_searches(q.limit.clamp(0, 1000)) {
        Ok(entries) => Json(json!({"count": entries.len(), "searches": entries})).into_response(),
        Err(e) => map_err(e),
    }
}

async fn access_log(State(state): State<Shared>, Query(q): Query<LimitQuery>) -> Response {
    let svc = state.svc.lock().expect("service lock");
    match svc.recent_accesses(q.limit.clamp(0, 1000)) {
        Ok(entries) => Json(json!({"count": entries.len(), "accesses": entries})).into_response(),
        Err(e) => map_err(e),
    }
}

async fn rating_log(State(state): State<Shared>, Query(q): Query<LimitQuery>) -> Response {
    let svc = state.svc.lock().expect("service lock");
    match svc.recent_ratings(q.limit.clamp(0, 1000)) {
        Ok(entries) => Json(json!({"count": entries.len(), "ratings": entries})).into_response(),
        Err(e) => map_err(e),
    }
}

#[derive(Deserialize)]
struct RateSearchBody {
    search_id: String,
    /// "hit" | "partial" | "miss"
    rating: String,
    #[serde(default)]
    used_episode_ids: Vec<String>,
    /// Miss/partial ground truth: episodes that should have surfaced.
    /// Triggers self-correction (enrich → redo → validate) per target.
    #[serde(default)]
    intended_episode_ids: Vec<String>,
    #[serde(default)]
    note: String,
}

async fn rate_search(State(state): State<Shared>, Json(body): Json<RateSearchBody>) -> Response {
    let rating: crate::recorder::Rating = match body.rating.parse() {
        Ok(r) => r,
        Err(e) => return err(StatusCode::BAD_REQUEST, e),
    };
    let mut svc = state.svc.lock().expect("service lock");
    match svc.rate_search(
        &body.search_id,
        rating,
        body.used_episode_ids,
        body.intended_episode_ids,
        none_if_empty(body.note),
    ) {
        Ok(entry) => (
            StatusCode::CREATED,
            Json(json!({"success": true, "rating": entry})),
        )
            .into_response(),
        Err(e) => map_err(e),
    }
}

async fn stats(State(state): State<Shared>) -> Response {
    let svc = state.svc.lock().expect("service lock");
    match svc.stats() {
        Ok(s) => Json(s).into_response(),
        Err(e) => map_err(e),
    }
}

#[derive(Deserialize)]
struct ImportBody {
    /// Absolute path to a mirror directory (engram or ecphory format).
    dir: String,
}

/// Delta import through the daemon — no more stop → import → kickstart
/// dance; the daemon owns the lock, so the daemon does the importing.
async fn admin_import(State(state): State<Shared>, Json(body): Json<ImportBody>) -> Response {
    let read = match crate::import::read_mirror(std::path::Path::new(&body.dir)) {
        Ok(r) => r,
        Err(e) => return map_err(e),
    };
    let parsed = read.episodes.len();
    let skipped_files = read.skipped.len();
    let mut svc = state.svc.lock().expect("service lock");
    match svc.import(read.episodes) {
        Ok(imported) => Json(json!({
            "success": true,
            "parsed": parsed,
            "imported": imported,
            "already_present": parsed - imported,
            "skipped_files": skipped_files,
        }))
        .into_response(),
        Err(e) => map_err(e),
    }
}

#[derive(Deserialize)]
struct ExportBody {
    /// Mirror directory; falls back to ECPHORY_EXPORT_DIR.
    #[serde(default)]
    dir: String,
    #[serde(default)]
    commit: bool,
}

async fn admin_export(State(state): State<Shared>, Json(body): Json<ExportBody>) -> Response {
    let dir = if body.dir.is_empty() {
        match std::env::var("ECPHORY_EXPORT_DIR") {
            Ok(d) if !d.is_empty() => d,
            _ => {
                return err(
                    StatusCode::BAD_REQUEST,
                    "no dir given and ECPHORY_EXPORT_DIR unset",
                );
            }
        }
    } else {
        body.dir
    };
    let episodes = {
        let svc = state.svc.lock().expect("service lock");
        match svc.export_all() {
            Ok(eps) => eps,
            Err(e) => return map_err(e),
        }
    };
    let path = std::path::Path::new(&dir);
    match crate::export::write_mirror(&episodes, path) {
        Ok(outcome) => {
            let committed = if body.commit {
                match crate::export::git_commit(path, "ecphory export") {
                    Ok(c) => c,
                    Err(e) => return map_err(e),
                }
            } else {
                false
            };
            Json(json!({
                "success": true,
                "episodes": episodes.len(),
                "written": outcome.written,
                "unchanged": outcome.unchanged,
                "committed": committed,
            }))
            .into_response()
        }
        Err(e) => map_err(e),
    }
}

async fn status(State(state): State<Shared>) -> Response {
    let svc = state.svc.lock().expect("service lock");
    let count = svc.count().unwrap_or(0);
    Json(json!({
        "status": "operational",
        "version": env!("CARGO_PKG_VERSION"),
        "episodes": count,
    }))
    .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constant_time_eq_basics() {
        assert!(constant_time_eq(b"secret", b"secret"));
        assert!(!constant_time_eq(b"secret", b"secreT"));
        assert!(!constant_time_eq(b"secret", b"secre"));
        assert!(!constant_time_eq(b"", b"x"));
        assert!(constant_time_eq(b"", b""));
    }
}
