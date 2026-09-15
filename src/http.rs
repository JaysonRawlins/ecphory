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
        .route(
            "/memory/episodes/{id}/versions/{version_id}/restore",
            post(restore_episode_version),
        )
        .route("/memory/search-log", get(search_log))
        .route("/memory/access-log", get(access_log))
        .route("/memory/rating-log", get(rating_log))
        .route("/memory/resolution-log", get(resolution_log))
        .route("/memory/search-rating", post(rate_search))
        .route("/memory/heal-replay", post(heal_replay))
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
        Error::NotFound(_) | Error::VersionNotFound { .. } => {
            err(StatusCode::NOT_FOUND, e.to_string())
        }
        Error::AmbiguousPrefix(_)
        | Error::AmbiguousVersionPrefix { .. }
        | Error::HardDeleteRefused => err(StatusCode::BAD_REQUEST, e.to_string()),
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
    /// Hidden groups (ECPHORY_HIDDEN_GROUPS) are skipped when unset and
    /// returned normally when named.
    #[serde(default)]
    group_id: Option<String>,
    #[serde(default)]
    source: Option<String>,
    /// Comma-separated; all must be present.
    #[serde(default)]
    tags: Option<String>,
    #[serde(default)]
    include_deleted: bool,
    /// Replay/benchmark bypass: skip the search log entirely. Prefer
    /// `origin` for jobs that should stay visible on the tape.
    #[serde(default)]
    no_record: bool,
    /// Tape provenance for synthetic traffic: organic (default) | eval |
    /// backfill | heal-replay. Tagged entries are recorded but excluded
    /// from top-queries and the workload aggregates.
    #[serde(default)]
    origin: Option<String>,
}

/// `a,b` -> `["a", "b"]`. The wire convention for tag filters across the
/// REST surface: serde_urlencoded has no sequence support, so a repeated
/// `?tag=` would not deserialize.
fn csv_tags(raw: Option<String>) -> Vec<String> {
    raw.map(|t| {
        t.split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect()
    })
    .unwrap_or_default()
}

async fn search(State(state): State<Shared>, Query(q): Query<SearchQuery>) -> Response {
    let origin: crate::recorder::SearchOrigin =
        match q.origin.as_deref().unwrap_or_default().parse() {
            Ok(o) => o,
            Err(e) => return err(StatusCode::BAD_REQUEST, e),
        };
    let tags = csv_tags(q.tags);
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
        svc.search_tagged(&q.query, &opts, origin)
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
    /// Comma-separated; all must be present. The subject-index renderer
    /// uses `?tags=ws:<slug>` to pull one workspace's episodes without
    /// dragging the whole store across the wire.
    #[serde(default)]
    tags: Option<String>,
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
        tags: csv_tags(q.tags),
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
    source: String,
    #[serde(default)]
    source_model: String,
    #[serde(default)]
    source_description: String,
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
            source: none_if_empty(body.source),
            source_model: none_if_empty(body.source_model),
            source_description: none_if_empty(body.source_description),
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

async fn restore_episode_version(
    State(state): State<Shared>,
    Path((id, version_id)): Path<(String, String)>,
) -> Response {
    let mut svc = state.svc.lock().expect("service lock");
    match svc.restore_version(&id, &version_id) {
        Ok(ep) => Json(json!({
            "success": true,
            "id": ep.id,
            "restored_version_id": version_id,
        }))
        .into_response(),
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

async fn resolution_log(State(state): State<Shared>, Query(q): Query<LimitQuery>) -> Response {
    let svc = state.svc.lock().expect("service lock");
    match svc.recent_resolutions(q.limit.clamp(0, 1000)) {
        Ok(entries) => {
            Json(json!({"count": entries.len(), "resolutions": entries})).into_response()
        }
        Err(e) => map_err(e),
    }
}

#[derive(Deserialize)]
struct HealReplayBody {
    /// Top-k window for held/regressed; 0 means the correction default (5).
    #[serde(default)]
    k: usize,
}

/// The heal-replay regression pass runs daemon-side: it both searches and
/// writes replay outcomes back onto the resolutions, and the daemon owns
/// the store lock — the eval CLI just asks for the report.
async fn heal_replay(State(state): State<Shared>, Json(body): Json<HealReplayBody>) -> Response {
    let svc = state.svc.lock().expect("service lock");
    match svc.replay_heals(body.k) {
        Ok(report) => Json(report).into_response(),
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
        "hidden_groups": svc.hidden_groups(),
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

    // ---- heal lifecycle, end to end over the real daemon path -----------------
    //
    // Everything below runs against a REAL axum server on an ephemeral
    // loopback port with a temp store — the same router, extractors, and
    // service locking production uses. Never the live store.

    async fn spawn_server() -> (String, tempfile::TempDir) {
        let (base, dir, _state) = spawn_server_with_state().await;
        (base, dir)
    }

    /// Same server, but hands back the shared service so a test can compare
    /// what came over the wire against the store's own answer.
    async fn spawn_server_with_state() -> (String, tempfile::TempDir, Shared) {
        let dir = tempfile::tempdir().expect("tempdir");
        let svc = Ecphory::open_with(dir.path().join("e2e.redb"), true).expect("open");
        let state = Arc::new(AppState {
            svc: Arc::new(Mutex::new(svc)),
            token: None,
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let served = Arc::clone(&state);
        tokio::spawn(async move {
            axum::serve(listener, build_router(served)).await.unwrap();
        });
        (format!("http://{addr}"), dir, state)
    }

    fn get(base: &str, path: &str) -> serde_json::Value {
        ureq::get(format!("{base}{path}"))
            .call()
            .unwrap_or_else(|e| panic!("GET {path}: {e}"))
            .body_mut()
            .read_json()
            .unwrap()
    }

    fn post(base: &str, path: &str, body: serde_json::Value) -> serde_json::Value {
        ureq::post(format!("{base}{path}"))
            .send_json(body)
            .unwrap_or_else(|e| panic!("POST {path}: {e}"))
            .body_mut()
            .read_json()
            .unwrap()
    }

    fn put(base: &str, path: &str, body: serde_json::Value) -> serde_json::Value {
        ureq::put(format!("{base}{path}"))
            .send_json(body)
            .unwrap_or_else(|e| panic!("PUT {path}: {e}"))
            .body_mut()
            .read_json()
            .unwrap()
    }

    fn add(base: &str, content: &str) -> String {
        let resp = post(base, "/api/v1/memory", json!({"content": content}));
        resp["episode"]["id"].as_str().unwrap().to_string()
    }

    fn search(base: &str, query: &str) -> serde_json::Value {
        get(
            base,
            &format!("/api/v1/memory/search?query={}", query.replace(' ', "%20")),
        )
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn heal_lifecycle_end_to_end() {
        let (base, _dir) = spawn_server().await;

        // The displacement rig: five episodes match the contested query with
        // growing padding, so `crowded` sits at rank 5; the miss target
        // shares no vocabulary with the query.
        for pad in [
            "",
            "with extra notes about unrelated calibration steps",
            "with extra notes about unrelated calibration steps and a long tail of \
             miscellaneous observations",
            "xenolith with extra notes about unrelated calibration steps and a long tail \
             of miscellaneous observations gathered over several sessions",
        ] {
            add(&base, &format!("quartz crystal resonance {pad}"));
        }
        let crowded = add(
            &base,
            "quartz crystal resonance padparadscha with the longest padding of them all, \
             extra notes about unrelated calibration steps and a long tail of miscellaneous \
             observations gathered over several sessions plus appendices nobody reads",
        );
        let target = add(
            &base,
            "piezoelectric oscillator drift measured on the bench meter",
        );

        // Prior rating marks `crowded` as used — protecting it.
        let out = search(&base, "padparadscha");
        assert_eq!(out["results"][0]["episode"]["id"], json!(crowded));
        post(
            &base,
            "/api/v1/memory/search-rating",
            json!({
                "search_id": out["search_id"],
                "rating": "hit",
                "used_episode_ids": [crowded],
            }),
        );

        // The miss: real query, zero overlap with the target's vocabulary.
        let out = search(&base, "quartz crystal resonance");
        assert!(
            !out["results"]
                .as_array()
                .unwrap()
                .iter()
                .any(|r| r["episode"]["id"] == json!(target)),
            "target must genuinely miss before the heal"
        );

        // Rate it miss with the intended id: the heal fires, validates, and
        // flags the collateral (crowded displaced from top k).
        let rated = post(
            &base,
            "/api/v1/memory/search-rating",
            json!({
                "search_id": out["search_id"],
                "rating": "miss",
                "intended_episode_ids": [target],
            }),
        );
        let correction = &rated["rating"]["corrections"][0];
        assert_eq!(correction["action"], json!("enriched"));
        assert_eq!(correction["after_rank"], json!(1));
        assert_eq!(correction["displaced_used"], json!([crowded]));

        // The resolution exists as its own record; the rating is immutable.
        let resolutions = get(&base, "/api/v1/memory/resolution-log?limit=10");
        assert_eq!(resolutions["count"], json!(1));
        let resolution = &resolutions["resolutions"][0];
        assert_eq!(resolution["rating_id"], rated["rating"]["id"]);
        assert_eq!(resolution["query"], json!("quartz crystal resonance"));
        assert_eq!(resolution["episode_id"], json!(target));
        assert_eq!(resolution["validated_rank"], json!(1));
        assert_eq!(resolution["displaced_used"], json!([crowded]));

        // Replay pass over the real endpoint: the heal holds.
        let report = post(&base, "/api/v1/memory/heal-replay", json!({"k": 0}));
        assert_eq!(report["total"], json!(1));
        assert_eq!(report["held"], json!(1));
        assert_eq!(report["regressed"], json!(0));
        assert_eq!(report["entries"][0]["rank"], json!(1));

        // The eval CLI path drives the same endpoint and agrees.
        let client = crate::eval::EvalClient::new(base.clone(), None);
        assert!(crate::eval::run_heals(&client, 5).unwrap());

        // Status splits the miss: rated, healed, nothing outstanding — and
        // the replay searches are tagged synthetic, not workload.
        let stats = get(&base, "/api/v1/memory/stats");
        assert_eq!(stats["rated_miss"], json!(1));
        assert_eq!(stats["rated_miss_healed"], json!(1));
        assert_eq!(stats["rated_miss_outstanding"], json!(0));
        assert_eq!(stats["heals"], json!(1));
        assert_eq!(stats["heals_regressed"], json!(0));
        assert!(
            stats["synthetic"].as_u64().unwrap() >= 2,
            "replay searches are tagged"
        );
        let replayed_in_top = stats["top_queries"]
            .as_array()
            .unwrap()
            .iter()
            .find(|q| q[0] == json!("quartz crystal resonance"))
            .map(|q| q[1].as_u64().unwrap());
        assert_eq!(
            replayed_in_top,
            Some(1),
            "replays must not inflate top_queries past the one organic search"
        );

        // Regression case: the healed target vanishes from search; the next
        // replay pass flags it, in the report and in status.
        ureq::delete(format!("{base}/api/v1/memory/episodes/{target}"))
            .call()
            .expect("demote target");
        let report = post(&base, "/api/v1/memory/heal-replay", json!({"k": 0}));
        assert_eq!(report["held"], json!(0));
        assert_eq!(report["regressed"], json!(1));
        assert!(
            !crate::eval::run_heals(&client, 5).unwrap(),
            "regressed pass must fail"
        );
        let stats = get(&base, "/api/v1/memory/stats");
        assert_eq!(stats["heals_regressed"], json!(1));
        // The original miss stays healed ground truth — replays never
        // rewrite the rating layer.
        assert_eq!(stats["rated_miss_healed"], json!(1));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn search_origin_param_tags_the_tape_and_rejects_garbage() {
        let (base, _dir) = spawn_server().await;
        add(&base, "origin fodder about kingfishers");

        search(&base, "kingfishers"); // organic
        get(&base, "/api/v1/memory/search?query=kingfishers&origin=eval");
        get(
            &base,
            "/api/v1/memory/search?query=kingfishers&no_record=true",
        );

        let log = get(&base, "/api/v1/memory/search-log?limit=10");
        assert_eq!(log["count"], json!(2), "no_record still bypasses the tape");
        assert_eq!(log["searches"][0]["origin"], json!("eval"));
        assert!(
            log["searches"][1].get("origin").is_none(),
            "organic stays implicit"
        );

        let err = ureq::get(format!("{base}/api/v1/memory/search?query=x&origin=bogus")).call();
        match err {
            Err(ureq::Error::StatusCode(code)) => assert_eq!(code, 400),
            other => panic!("expected 400 for bogus origin, got {other:?}"),
        }
    }

    // ---- the CLI log views' HTTP path ----------------------------------------
    //
    // `heals`, `ratings`, `search-log` and `access-log` read through the
    // daemon because redb's lock is process-exclusive (issue #15). The thing
    // that can silently rot is the ROW SHAPE: these decode into the canonical
    // recorder entries, and the tempting reuse — eval's `HealEntry` — is a
    // flatter report shape that has no `last_replay` at all. So this asserts
    // the wire answer is byte-equal to the store's own, with a replay outcome
    // present precisely because that is the field a narrower shape would eat.

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn cli_log_views_match_the_store_over_http() {
        use crate::eval::EvalClient;
        use crate::recorder::{AccessLogEntry, RatingLogEntry, ResolutionLogEntry, SearchLogEntry};

        let (base, _dir, state) = spawn_server_with_state().await;

        // A miss worth healing: the target shares no vocabulary with the query.
        add(&base, "quartz crystal resonance measured in the lab");
        let target = add(&base, "piezoelectric oscillator drift on the bench meter");
        get(&base, &format!("/api/v1/memory/episodes/{target}")); // an access-log row

        let out = search(&base, "quartz crystal resonance");
        post(
            &base,
            "/api/v1/memory/search-rating",
            json!({
                "search_id": out["search_id"],
                "rating": "miss",
                "intended_episode_ids": [target],
                "note": "round-trip fixture",
            }),
        );
        // Replay so `last_replay` is populated rather than None.
        post(&base, "/api/v1/memory/heal-replay", json!({ "k": 5 }));

        let client = EvalClient::new(base.clone(), None);
        assert!(client.reachable(), "the daemon we just started must answer");
        assert!(
            !EvalClient::new("http://127.0.0.1:1".to_string(), None).reachable(),
            "a dead port must read as unreachable, or the CLI never falls back"
        );

        let resolutions: Vec<ResolutionLogEntry> = client.resolution_log(10).unwrap();
        assert_eq!(resolutions.len(), 1, "the heal should have resolved");
        let r = &resolutions[0];
        assert!(!r.query.is_empty(), "the renderer prints the query");
        assert!(
            r.last_replay.is_some(),
            "replay outcome must survive the wire — this is what a flatter \
             row shape silently drops"
        );

        let searches: Vec<SearchLogEntry> = client.search_log(10).unwrap();
        let accesses: Vec<AccessLogEntry> = client.access_log(10).unwrap();
        let ratings: Vec<RatingLogEntry> = client.rating_log(10).unwrap();
        assert!(!searches.is_empty() && !accesses.is_empty() && !ratings.is_empty());
        assert_eq!(
            ratings[0].note.as_deref(),
            Some("round-trip fixture"),
            "`ratings` renders the note; it must not be projected away"
        );

        // The load-bearing claim: HTTP and a direct store read are the same rows.
        let svc = state.svc.lock().expect("service lock");
        for (over_http, from_store) in [
            (
                serde_json::to_value(&resolutions).unwrap(),
                serde_json::to_value(svc.recent_resolutions(10).unwrap()).unwrap(),
            ),
            (
                serde_json::to_value(&searches).unwrap(),
                serde_json::to_value(svc.recent_searches(10).unwrap()).unwrap(),
            ),
            (
                serde_json::to_value(&accesses).unwrap(),
                serde_json::to_value(svc.recent_accesses(10).unwrap()).unwrap(),
            ),
            (
                serde_json::to_value(&ratings).unwrap(),
                serde_json::to_value(svc.recent_ratings(10).unwrap()).unwrap(),
            ),
        ] {
            assert_eq!(over_http, from_store);
        }
    }

    // ---- subject-index support: listing scoped to a workspace tag ----------
    //
    // The renderer needs one workspace's episodes without pulling the whole
    // store across the wire on every write. It reads over HTTP (the daemon
    // holds the redb lock), so the filter has to live here.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn list_episodes_filters_by_tag() {
        let (base, _dir) = spawn_server().await;
        post(
            &base,
            "/api/v1/memory",
            json!({"content": "mine", "tags": ["ws:-work-x", "gotcha"]}),
        );
        post(
            &base,
            "/api/v1/memory",
            json!({"content": "theirs", "tags": ["ws:-work-y"]}),
        );

        let all = get(&base, "/api/v1/memory/episodes?max_results=50");
        assert_eq!(all["count"].as_u64().unwrap(), 2);

        let scoped = get(
            &base,
            "/api/v1/memory/episodes?max_results=50&tags=ws:-work-x",
        );
        assert_eq!(
            scoped["count"].as_u64().unwrap(),
            1,
            "tag filter did not scope the listing: {scoped}"
        );
        assert_eq!(scoped["episodes"][0]["content"].as_str().unwrap(), "mine");
    }
    // ---- provenance is correctable after the fact (issue #39) -------------
    //
    // `source`, `source_model` and `source_description` were settable at write
    // time and unreachable afterwards, so a bad value was permanent for the
    // life of the episode. This drives the real repair over the real PUT with
    // the corruption the issue was filed for: an Opus 4.6/4.7-era agent closed
    // the `source` tag inside the value and the store took it verbatim, as
    // designed. The intended values are recoverable from the text, so the fix
    // must be an update, not a delete-and-re-add that mints a new id.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn update_repairs_a_mangled_source_without_minting_a_new_id() {
        let (base, _dir) = spawn_server().await;
        let mangled = "claude-code</source>\n<parameter name=\"source_model\">opus-4.7";

        let created = post(
            &base,
            "/api/v1/memory",
            json!({
                "content": "balcony security hub wiring notes",
                "source": mangled,
                "source_description": "leaked prose</source_description>",
            }),
        );
        let id = created["episode"]["id"].as_str().unwrap().to_string();
        assert_eq!(created["episode"]["source"].as_str().unwrap(), mangled);

        let updated = put(
            &base,
            &format!("/api/v1/memory/episodes/{id}"),
            json!({
                "source": "claude-code",
                "source_model": "opus-4.7",
                "source_description": "captured by the coding agent",
            }),
        );
        assert_eq!(
            updated["source"].as_str().unwrap(),
            "claude-code",
            "source must be correctable: {updated}"
        );
        assert_eq!(updated["source_model"].as_str().unwrap(), "opus-4.7");
        assert_eq!(
            updated["source_description"].as_str().unwrap(),
            "captured by the coding agent"
        );

        // The id is the whole point: wikilinks, index rows and the access log
        // all reference it, which is why delete-and-re-add was not the cure.
        assert_eq!(updated["id"].as_str().unwrap(), id);
        assert_eq!(
            updated["content"].as_str().unwrap(),
            "balcony security hub wiring notes",
            "an update naming only provenance must leave content alone"
        );

        // The converse guarantee: an update that does not name provenance must
        // not blank it. Every field here is "empty means leave unchanged", so
        // the absent `source` must not reach the store as "".
        let after_content_edit = put(
            &base,
            &format!("/api/v1/memory/episodes/{id}"),
            json!({"content": "balcony security hub wiring notes, revised"}),
        );
        assert_eq!(
            after_content_edit["source"].as_str().unwrap(),
            "claude-code",
            "a content-only update wiped provenance: {after_content_edit}"
        );
        assert_eq!(
            after_content_edit["source_model"].as_str().unwrap(),
            "opus-4.7"
        );
        assert_eq!(
            after_content_edit["source_description"].as_str().unwrap(),
            "captured by the coding agent"
        );

        // And the mangled value is still recoverable, like every other update.
        let versions = get(&base, &format!("/api/v1/memory/episodes/{id}/versions"));
        assert_eq!(versions["count"], json!(2));
        assert_eq!(
            versions["versions"][0]["episode"]["source"]
                .as_str()
                .unwrap(),
            mangled,
            "the prior provenance must be archived, not discarded"
        );
    }
}
