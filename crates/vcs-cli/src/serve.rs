//! `vcs serve` — standalone HTTP server exposing the vcs store.
//!
//! All GET endpoints match the shape already served by the Vite plugin
//! (vcs-integration/vite-plugin.js), so the existing React dashboard
//! works against either a local Vite dev server OR a remote `vcs serve` hub.
//!
//! ### Why this enables multi-project agents
//!
//! ```text
//! Project A (.vcs/)          Project B (.vcs/)
//!      │                          │
//!      │   POST /api/vcs/push     │   POST /api/vcs/push
//!      └──────────┬───────────────┘
//!                 ▼
//!         vcs serve --port 7474   (hub store)
//!         - aggregates stacks from all projects
//!         - single view → cross-project conflict detection
//! ```
//!
//! ### Multi-token ACL (P3.2)
//!
//! `vcs token add ci-agent <secret>` → write-capable token
//! `vcs token add dashboard <secret> --read-only` → read-only token
//!
//! When any token is configured:
//! - All POST (write) endpoints require a write-capable token
//! - `GET /api/vcs/export` requires any valid token
//! - Other GET endpoints remain public (metrics / dashboard use)
//!
//! ### Read endpoints (GET)
//!
//! ```
//! GET /api/vcs/status
//! GET /api/vcs/changes
//! GET /api/vcs/stacks
//! GET /api/vcs/views
//! GET /api/vcs/active-view
//! GET /api/vcs/view/:id/files
//! GET /api/vcs/view/:id/conflicts
//! GET /api/vcs/export          ← requires any valid token when ACL is on
//! ```
//!
//! ### Write endpoints (POST)
//!
//! ```
//! POST /api/vcs/stacks/open           { agent_id, base_change_id? }
//! POST /api/vcs/stacks/:id/close
//! POST /api/vcs/stacks/:id/abandon
//! POST /api/vcs/edit                  { stack_id, path, content_b64, intent }
//! POST /api/vcs/delete                { stack_id, path, intent }
//! POST /api/vcs/views/open            { base_change_id, stack_ids: [] }
//! POST /api/vcs/conflicts/:id/resolve { pick?: stack_id, merge_content_b64? }
//! POST /api/vcs/push                  HubBundle (see vcs-core::hub)
//! ```

use anyhow::Result;
use axum::{
    extract::{Path, Query, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use tower_http::cors::{Any, CorsLayer};
use vcs_core::{HubBundle, Intent, Resolution, Store};

// ── Token type (public so main.rs can build the list) ─────────────────────

/// A single API token for the hub server.
pub struct ServeToken {
    /// The secret value sent in `Authorization: Bearer <value>`
    pub value: String,
    /// If true, this token can only call GET endpoints; POST returns 403.
    pub read_only: bool,
}

// ── Shared state ───────────────────────────────────────────────────────────

struct AppState {
    db:     Arc<Mutex<Store>>,
    /// Empty = no auth required (open hub).
    tokens: Vec<ServeToken>,
}
type Db = Arc<AppState>;

// ── Error type ─────────────────────────────────────────────────────────────

struct ApiError(anyhow::Error, StatusCode);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = json!({ "error": self.0.to_string() });
        (self.1, Json(body)).into_response()
    }
}

impl<E: Into<anyhow::Error>> From<E> for ApiError {
    fn from(e: E) -> Self {
        ApiError(e.into(), StatusCode::INTERNAL_SERVER_ERROR)
    }
}

type ApiResult<T> = std::result::Result<Json<T>, ApiError>;

// ── Auth helpers ───────────────────────────────────────────────────────────

/// Extract the bearer token value from the `Authorization` header.
fn bearer(headers: &axum::http::HeaderMap) -> &str {
    headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .unwrap_or("")
}

/// Check whether the request carries any valid token.
/// Returns `Ok(true)` if the matching token is read-only, `Ok(false)` if
/// it's write-capable.  Returns `Err(401)` when auth is configured and no
/// matching token is supplied.
fn check_auth_level(
    state: &AppState,
    headers: &axum::http::HeaderMap,
) -> std::result::Result<bool, ApiError> {
    if state.tokens.is_empty() {
        return Ok(false); // open hub — no auth required
    }
    let provided = bearer(headers);
    match state.tokens.iter().find(|t| t.value == provided) {
        Some(t) => Ok(t.read_only),
        None => Err(ApiError(anyhow::anyhow!("unauthorized"), StatusCode::UNAUTHORIZED)),
    }
}

/// Require any valid token (for sensitive GET endpoints like `/export`).
fn require_any_token(
    state: &AppState,
    headers: &axum::http::HeaderMap,
) -> std::result::Result<(), ApiError> {
    check_auth_level(state, headers).map(|_| ())
}

/// Require a write-capable token for all POST/mutation endpoints.
/// Returns `Err(401)` if no token is supplied and auth is configured.
/// Returns `Err(403)` if the token is read-only.
fn require_write(
    state: &AppState,
    headers: &axum::http::HeaderMap,
) -> std::result::Result<(), ApiError> {
    if check_auth_level(state, headers)? {
        Err(ApiError(
            anyhow::anyhow!("token is read-only — write operations not permitted"),
            StatusCode::FORBIDDEN,
        ))
    } else {
        Ok(())
    }
}

// ── Request bodies ─────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct OpenStackBody {
    agent_id: String,
    base_change_id: Option<String>,
}

#[derive(Deserialize)]
struct IntentBody {
    reason: String,
    task_ref: Option<String>,
}

#[derive(Deserialize)]
struct EditBody {
    stack_id: String,
    path: String,
    content_b64: String,
    intent: IntentBody,
}

#[derive(Deserialize)]
struct DeleteBody {
    stack_id: String,
    path: String,
    intent: IntentBody,
}

#[derive(Deserialize)]
struct OpenViewBody {
    base_change_id: String,
    stack_ids: Vec<String>,
}

#[derive(Deserialize)]
struct ResolveBody {
    pick: Option<String>,
    merge_content_b64: Option<String>,
}

#[derive(Deserialize)]
struct ExportQuery {
    project_id: Option<String>,
}

// ── GET handlers ───────────────────────────────────────────────────────────

async fn get_status(State(db): State<Db>) -> ApiResult<Value> {
    let store = db.db.lock().unwrap();
    let token_count = db.tokens.len();
    let ro_count = db.tokens.iter().filter(|t| t.read_only).count();
    Ok(Json(json!({
        "initialised":    true,
        "storePath":      store.store_path().display().to_string(),
        "mode":           "hub",
        "auth":           token_count > 0,
        "token_count":    token_count,
        "ro_token_count": ro_count,
    })))
}

async fn get_changes(State(db): State<Db>) -> ApiResult<Value> {
    let store = db.db.lock().unwrap();
    Ok(Json(serde_json::to_value(store.list_changes()?)?))
}

async fn get_edits(State(db): State<Db>) -> ApiResult<Value> {
    let store = db.db.lock().unwrap();
    Ok(Json(serde_json::to_value(store.list_edit_metadata()?)?))
}

async fn get_stacks(State(db): State<Db>) -> ApiResult<Value> {
    let store = db.db.lock().unwrap();
    Ok(Json(serde_json::to_value(store.list_stacks()?)?))
}

async fn get_views(State(db): State<Db>) -> ApiResult<Value> {
    let store = db.db.lock().unwrap();
    Ok(Json(serde_json::to_value(store.list_views()?)?))
}

async fn get_active_view(State(db): State<Db>) -> ApiResult<Value> {
    let store = db.db.lock().unwrap();
    Ok(Json(serde_json::to_value(store.latest_view()?)?))
}

async fn get_view_files(State(db): State<Db>, Path(view_id): Path<String>) -> ApiResult<Value> {
    let store = db.db.lock().unwrap();
    Ok(Json(json!(store.list_files(&view_id)?)))
}

async fn get_view_conflicts(
    State(db): State<Db>,
    Path(view_id): Path<String>,
) -> ApiResult<Value> {
    let store = db.db.lock().unwrap();
    Ok(Json(serde_json::to_value(store.conflicts(&view_id)?)?))
}

/// Export a full [`HubBundle`] — protected when tokens are configured.
async fn get_export(
    State(db): State<Db>,
    headers: axum::http::HeaderMap,
    Query(query): Query<ExportQuery>,
) -> ApiResult<HubBundle> {
    require_any_token(&db, &headers)?;
    let store = db.db.lock().unwrap();
    let project_id = query.project_id.as_deref().unwrap_or("hub");
    Ok(Json(store.export_bundle(project_id)?))
}

async fn get_blob(
    State(db): State<Db>,
    Path(hash): Path<String>,
) -> std::result::Result<Response, ApiError> {
    let store = db.db.lock().unwrap();
    let data = store.get_blob(&hash)?;
    Ok((
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/octet-stream")],
        data,
    )
        .into_response())
}

// ── POST handlers ──────────────────────────────────────────────────────────
//
// Every write handler calls `require_write()` first.
// This enforces: 401 when no valid token is supplied (if tokens are configured)
//                403 when a read-only token is supplied

async fn post_stack_open(
    State(db): State<Db>,
    headers: axum::http::HeaderMap,
    Json(body): Json<OpenStackBody>,
) -> ApiResult<Value> {
    require_write(&db, &headers)?;
    let store = db.db.lock().unwrap();
    let stack_id = store.open_stack(&body.agent_id, body.base_change_id)?;
    Ok(Json(json!({ "stack_id": stack_id })))
}

async fn post_stack_close(
    State(db): State<Db>,
    headers: axum::http::HeaderMap,
    Path(stack_id): Path<String>,
) -> ApiResult<Value> {
    require_write(&db, &headers)?;
    let store = db.db.lock().unwrap();
    store.close_stack(&stack_id)?;
    Ok(Json(json!({ "ok": true })))
}

async fn post_stack_abandon(
    State(db): State<Db>,
    headers: axum::http::HeaderMap,
    Path(stack_id): Path<String>,
) -> ApiResult<Value> {
    require_write(&db, &headers)?;
    let store = db.db.lock().unwrap();
    store.abandon_stack(&stack_id)?;
    Ok(Json(json!({ "ok": true })))
}

async fn post_edit(
    State(db): State<Db>,
    headers: axum::http::HeaderMap,
    Json(body): Json<EditBody>,
) -> ApiResult<Value> {
    require_write(&db, &headers)?;
    let content = B64
        .decode(&body.content_b64)
        .map_err(|e| anyhow::anyhow!("base64 decode: {e}"))?;
    let mut intent = Intent::new(&body.intent.reason);
    if let Some(tr) = body.intent.task_ref {
        intent = intent.with_task_ref(tr);
    }
    let store = db.db.lock().unwrap();
    let change_id = store.edit(&body.stack_id, &body.path, &content, intent)?;
    Ok(Json(json!({ "change_id": change_id })))
}

async fn post_delete(
    State(db): State<Db>,
    headers: axum::http::HeaderMap,
    Json(body): Json<DeleteBody>,
) -> ApiResult<Value> {
    require_write(&db, &headers)?;
    let mut intent = Intent::new(&body.intent.reason);
    if let Some(tr) = body.intent.task_ref {
        intent = intent.with_task_ref(tr);
    }
    let store = db.db.lock().unwrap();
    let change_id = store.delete(&body.stack_id, &body.path, intent)?;
    Ok(Json(json!({ "change_id": change_id })))
}

async fn post_view_open(
    State(db): State<Db>,
    headers: axum::http::HeaderMap,
    Json(body): Json<OpenViewBody>,
) -> ApiResult<Value> {
    require_write(&db, &headers)?;
    let store = db.db.lock().unwrap();
    let view_id = store.open_view(body.base_change_id, &body.stack_ids)?;
    Ok(Json(json!({ "view_id": view_id })))
}

async fn post_resolve(
    State(db): State<Db>,
    headers: axum::http::HeaderMap,
    Path(conflict_id): Path<String>,
    Json(body): Json<ResolveBody>,
) -> ApiResult<Value> {
    require_write(&db, &headers)?;
    let resolution = if let Some(sid) = body.pick {
        Resolution::Pick { stack_id: sid }
    } else if let Some(b64) = body.merge_content_b64 {
        let data = B64
            .decode(&b64)
            .map_err(|e| anyhow::anyhow!("base64 decode: {e}"))?;
        let store = db.db.lock().unwrap();
        let hash = store.put_blob(&data)?;
        store.resolve(&conflict_id, Resolution::Merge { blob_hash: hash })?;
        return Ok(Json(json!({ "ok": true })));
    } else {
        return Err(ApiError(
            anyhow::anyhow!("provide pick or merge_content_b64"),
            StatusCode::BAD_REQUEST,
        ));
    };
    let store = db.db.lock().unwrap();
    store.resolve(&conflict_id, resolution)?;
    Ok(Json(json!({ "ok": true })))
}

/// Receive a [`HubBundle`] from a remote project and ingest it.
/// Requires a write-capable token when ACL is configured.
async fn post_push(
    State(db): State<Db>,
    headers: axum::http::HeaderMap,
    Json(bundle): Json<HubBundle>,
) -> ApiResult<Value> {
    require_write(&db, &headers)?;
    let store = db.db.lock().unwrap();
    let (blobs, stacks, changes) = store.import_bundle(&bundle)?;
    Ok(Json(json!({
        "ok":              true,
        "project_id":      bundle.project_id,
        "blobs_stored":    blobs,
        "stacks_imported": stacks,
        "changes_imported": changes,
    })))
}

// ── Router ─────────────────────────────────────────────────────────────────

pub async fn run(store: Store, port: u16, tokens: Vec<ServeToken>) -> Result<()> {
    let db: Db = Arc::new(AppState {
        db: Arc::new(Mutex::new(store)),
        tokens,
    });

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        // ── read ───────────────────────────────────────────────────────────
        .route("/api/vcs/status", get(get_status))
        .route("/api/vcs/changes", get(get_changes))
        .route("/api/vcs/edits", get(get_edits))
        .route("/api/vcs/stacks", get(get_stacks))
        .route("/api/vcs/views", get(get_views))
        .route("/api/vcs/active-view", get(get_active_view))
        .route("/api/vcs/view/:id/files", get(get_view_files))
        .route("/api/vcs/view/:id/conflicts", get(get_view_conflicts))
        .route("/api/vcs/export", get(get_export))
        .route("/api/vcs/blobs/:hash", get(get_blob))
        // ── write ──────────────────────────────────────────────────────────
        .route("/api/vcs/stacks/open", post(post_stack_open))
        .route("/api/vcs/stacks/:id/close", post(post_stack_close))
        .route("/api/vcs/stacks/:id/abandon", post(post_stack_abandon))
        .route("/api/vcs/edit", post(post_edit))
        .route("/api/vcs/delete", post(post_delete))
        .route("/api/vcs/views/open", post(post_view_open))
        .route("/api/vcs/conflicts/:id/resolve", post(post_resolve))
        // ── inter-project ──────────────────────────────────────────────────
        .route("/api/vcs/push", post(post_push))
        .layer(cors)
        .with_state(db);

    let addr = format!("0.0.0.0:{port}");
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    println!("vcs hub listening on http://{addr}");
    println!("  Dashboard:  point the tanstack-vite UI at http://localhost:{port}");
    println!("  Push URL:   POST http://localhost:{port}/api/vcs/push");
    axum::serve(listener, app).await?;
    Ok(())
}
