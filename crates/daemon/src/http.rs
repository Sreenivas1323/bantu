//! HTTP API: `/v1/health`, `/v1/state`, `/v1/commands`, `/v1/events` (SSE),
//! plus the built web app at `/`.

use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, Request, State};
use axum::http::{header, HeaderMap, HeaderValue, Method, StatusCode};
use axum::middleware::{self, Next};
use axum::response::sse::{Event as SseEvent, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use bantu_protocol::{Command, CommandResponse, Event, Health, StateSnapshot, PROTOCOL_VERSION};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tower_http::cors::{Any, CorsLayer};
use tower_http::services::{ServeDir, ServeFile};

use crate::ops::{CommandError, Daemon};
use crate::terminals::Term;

pub const DAEMON_VERSION: &str = env!("CARGO_PKG_VERSION");
const REPLAY_BATCH: i64 = 500;

#[derive(Clone)]
pub struct AppState {
    pub daemon: Arc<Daemon>,
    pub token_hash: Arc<[u8]>,
    pub allow_remote: bool,
    /// Set when the daemon sits behind Cloudflare Access.
    pub access: Option<Arc<crate::access::AccessVerifier>>,
}

pub fn router(state: AppState) -> Router {
    let api = Router::new()
        .route("/v1/state", get(get_state))
        .route("/v1/commands", post(post_command))
        .route("/v1/events", get(get_events))
        .route("/v1/activity", get(get_activity))
        .route("/v1/agents", get(get_agents))
        .route("/v1/repo-suggestions", get(get_repo_suggestions))
        .route("/v1/push/key", get(get_push_key))
        .route("/v1/workspaces/{id}/history", get(get_history))
        .route("/v1/workspaces/{id}/diff", get(get_diff))
        .route("/v1/workspaces/{id}/pr", get(get_pr))
        .route("/v1/workspaces/{id}/checks", get(get_checks))
        .route("/v1/workspaces/{id}/ports", get(get_ports))
        .route("/v1/workspaces/{id}/children", get(get_children))
        .route("/v1/repos/{id}/conflicts", post(scan_conflicts))
        .route("/v1/terminals/{id}/ws", get(terminal_ws))
        .route_layer(middleware::from_fn_with_state(state.clone(), require_token));

    let mut app = Router::new()
        .route("/v1/health", get(health))
        .route("/v1/hooks/permission", post(permission_hook))
        .route("/v1/mcp", post(mcp).get(mcp_get))
        .merge(api)
        .with_state(state.clone());

    if let Some(dir) = state.daemon.config.web_dir.clone() {
        let index = dir.join("index.html");
        app = app.fallback_service(ServeDir::new(dir).fallback(ServeFile::new(index)));
    }

    app.layer(
        // Bearer tokens (not cookies) carry auth, so any origin may call us.
        CorsLayer::new()
            .allow_origin(Any)
            .allow_methods([Method::GET, Method::POST])
            .allow_headers([
                header::AUTHORIZATION,
                header::CONTENT_TYPE,
                "idempotency-key".parse().unwrap(),
                "last-event-id".parse().unwrap(),
            ]),
    )
    .layer(middleware::from_fn_with_state(state.clone(), check_access))
    .layer(middleware::from_fn_with_state(state, check_host))
}

pub fn hash_token(token: &str) -> Vec<u8> {
    Sha256::digest(token.as_bytes()).to_vec()
}

/// Rejects requests whose Host isn't loopback or explicitly allowed, blocking
/// DNS-rebinding attacks from web pages. `--allow-remote` accepts any host.
pub(crate) async fn check_host(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> Response {
    if !state.allow_remote {
        let hostname = request_hostname(&req);
        let allowed = is_loopback(&hostname)
            || state
                .daemon
                .config
                .allowed_hosts
                .iter()
                .any(|h| h.eq_ignore_ascii_case(&hostname));
        if !allowed {
            return (StatusCode::FORBIDDEN, "host not allowed").into_response();
        }
    }
    next.run(req).await
}

/// The request's `Host`, without its port, lowercased.
fn request_hostname(req: &Request) -> String {
    let host = req
        .headers()
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");
    host.strip_prefix('[')
        .and_then(|h| h.split(']').next())
        .unwrap_or_else(|| host.split(':').next().unwrap_or(""))
        .to_ascii_lowercase()
}

fn is_loopback(hostname: &str) -> bool {
    matches!(hostname, "127.0.0.1" | "localhost" | "::1")
}

/// Behind Cloudflare Access, requests for a public hostname must carry a valid
/// Access token. Without one, the Access setup has a gap, so this fails closed.
pub(crate) async fn check_access(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> Response {
    let Some(access) = &state.access else {
        return next.run(req).await;
    };
    if is_loopback(&request_hostname(&req)) {
        return next.run(req).await;
    }
    let token = req
        .headers()
        .get("cf-access-jwt-assertion")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");
    let now = chrono::Utc::now().timestamp();
    match access.verify(token, now).await {
        Ok(_) => next.run(req).await,
        Err(e) => {
            tracing::warn!("refused a request without a valid Cloudflare Access sign-in: {e:#}");
            (
                StatusCode::FORBIDDEN,
                "This daemon requires a Cloudflare Access sign-in, and this request didn't have a valid one.",
            )
                .into_response()
        }
    }
}

#[derive(Deserialize)]
struct TokenQuery {
    token: Option<String>,
}

async fn require_token(
    State(state): State<AppState>,
    Query(q): Query<TokenQuery>,
    req: Request,
    next: Next,
) -> Response {
    let bearer = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "))
        .map(str::to_string);
    // EventSource can't set headers, so `?token=` is accepted too.
    let presented = bearer.or(q.token);
    match presented {
        Some(t) if hash_token(&t).as_slice() == &*state.token_hash => next.run(req).await,
        _ => (StatusCode::UNAUTHORIZED, "missing or invalid token").into_response(),
    }
}

async fn get_activity(
    State(state): State<AppState>,
) -> Result<Json<bantu_protocol::Activity>, ApiError> {
    Ok(Json(state.daemon.activity().await?))
}

/// Installed agent CLIs and the accounts they're signed in to.
async fn get_agents(State(state): State<AppState>) -> Json<Vec<bantu_protocol::AgentStatus>> {
    Json(state.daemon.agent_statuses().await)
}

/// Git repos found in common code folders under $HOME, newest first.
async fn get_repo_suggestions(
    State(state): State<AppState>,
) -> Result<Json<Vec<crate::discover::RepoSuggestion>>, ApiError> {
    Ok(Json(state.daemon.suggest_repos().await?))
}

/// The public key browsers need to subscribe to this daemon's alerts.
async fn get_push_key(State(state): State<AppState>) -> Json<serde_json::Value> {
    Json(serde_json::json!({"public_key": state.daemon.push_public_key()}))
}

async fn health() -> Json<Health> {
    Json(Health {
        ok: true,
        protocol_version: PROTOCOL_VERSION,
        daemon_version: DAEMON_VERSION.into(),
    })
}

async fn get_state(State(state): State<AppState>) -> Result<Json<StateSnapshot>, ApiError> {
    let store = &state.daemon.store;
    // Read last_seq first: anything committed after it will arrive via events.
    let last_seq = store.last_seq().await?;
    Ok(Json(StateSnapshot {
        protocol_version: PROTOCOL_VERSION,
        daemon_version: DAEMON_VERSION.into(),
        last_seq,
        repos: store.repos().await?,
        workspaces: store.workspaces().await?,
        sessions: store.sessions().await?,
        comments: store.comments().await?,
        terminals: state.daemon.terminals().await,
        permissions: state.daemon.pending_permissions().await,
        inbox: state.daemon.inbox_items().await,
        conflicts: state.daemon.all_conflicts().await,
        spotlights: store.spotlights().await?,
        loops: store.loops().await?,
        preferences: store.preferences().await?,
    }))
}

#[derive(Deserialize)]
struct HookQuery {
    session: String,
}

/// Bantu's MCP server (Streamable HTTP, JSON responses only).
async fn mcp(
    State(state): State<AppState>,
    Query(q): Query<HookQuery>,
    headers: HeaderMap,
    Json(msg): Json<serde_json::Value>,
) -> Response {
    let presented = headers
        .get(header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "));
    if presented != Some(state.daemon.hook_token()) {
        return (StatusCode::UNAUTHORIZED, "bad token").into_response();
    }
    let session = match state.daemon.store.session(&q.session).await {
        Ok(Some((s, _))) => s,
        _ => return (StatusCode::NOT_FOUND, "no such session").into_response(),
    };
    match state.daemon.mcp_handle(&session, &msg).await {
        Some(reply) => Json(reply).into_response(),
        // Notifications get no body.
        None => StatusCode::ACCEPTED.into_response(),
    }
}

/// No server-initiated stream; clients fall back to plain request/response.
async fn mcp_get() -> StatusCode {
    StatusCode::METHOD_NOT_ALLOWED
}

/// Claude Code's `PermissionRequest` HTTP hook. Authenticated with the hook
/// token (not the client token) and blocks until the user answers.
async fn permission_hook(
    State(state): State<AppState>,
    Query(q): Query<HookQuery>,
    headers: HeaderMap,
    Json(input): Json<serde_json::Value>,
) -> Response {
    let presented = headers
        .get(header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "));
    if presented != Some(state.daemon.hook_token()) {
        return (StatusCode::UNAUTHORIZED, "bad hook token").into_response();
    }
    let session = match state.daemon.store.session(&q.session).await {
        Ok(Some((s, _))) => s,
        _ => return (StatusCode::NOT_FOUND, "no such session").into_response(),
    };
    Json(state.daemon.request_permission(&session, &input).await).into_response()
}

/// Raw PTY bytes both ways. Binary frames are input/output; text frames carry
/// JSON control messages: `{"type":"resize","cols":120,"rows":40}`.
async fn terminal_ws(
    State(state): State<AppState>,
    Path(id): Path<String>,
    upgrade: WebSocketUpgrade,
) -> Response {
    let Some(term) = state.daemon.terminal(&id).await else {
        return (StatusCode::NOT_FOUND, "no such terminal").into_response();
    };
    let daemon = state.daemon.clone();
    upgrade.on_upgrade(move |socket| serve_terminal(socket, term, daemon))
}

async fn serve_terminal(socket: WebSocket, term: Arc<Term>, daemon: Arc<Daemon>) {
    let _client = daemon.activity.client();
    use futures::{SinkExt, StreamExt};
    let (mut sink, mut stream) = socket.split();
    let (backlog, mut output) = term.attach();
    if !backlog.is_empty() && sink.send(Message::Binary(backlog.into())).await.is_err() {
        return;
    }
    let to_client = async {
        loop {
            match output.recv().await {
                Ok(bytes) => {
                    if sink.send(Message::Binary(bytes.into())).await.is_err() {
                        break;
                    }
                }
                // A slow viewer missed some output; keep streaming.
                Err(RecvError::Lagged(_)) => continue,
                Err(RecvError::Closed) => break,
            }
        }
    };
    let from_client = async {
        while let Some(Ok(msg)) = stream.next().await {
            match msg {
                Message::Binary(bytes) => {
                    let _ = term.write(&bytes);
                }
                Message::Text(text) => {
                    let Ok(v) = serde_json::from_str::<serde_json::Value>(text.as_str()) else {
                        continue;
                    };
                    match v["type"].as_str() {
                        Some("resize") => term.resize(
                            v["cols"].as_u64().unwrap_or(80) as u16,
                            v["rows"].as_u64().unwrap_or(24) as u16,
                        ),
                        Some("input") => {
                            let _ = term.write(v["data"].as_str().unwrap_or("").as_bytes());
                        }
                        _ => {}
                    }
                }
                Message::Close(_) => break,
                _ => {}
            }
        }
    };
    tokio::select! {
        _ = to_client => {}
        _ = from_client => {}
    }
}

async fn get_diff(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    match state.daemon.workspace_diff(&id).await {
        Ok(diff) => Json(diff).into_response(),
        Err(e) => command_error(e),
    }
}

/// Runs the conflict radar for a repo now and returns the result.
async fn scan_conflicts(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Vec<bantu_protocol::WorkspaceConflict>>, ApiError> {
    Ok(Json(state.daemon.scan_conflicts(&id).await?))
}

/// `null` when the branch has no PR, the repo has no remote, or `gh` is missing.
async fn get_pr(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    match state.daemon.pull_request(&id).await {
        Ok(pr) => Json(pr).into_response(),
        Err(e) => command_error(e),
    }
}

/// CI runs, deployments, PR comments and todos.
async fn get_checks(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    match state.daemon.workspace_checks(&id).await {
        Ok(checks) => Json(checks).into_response(),
        Err(e) => command_error(e),
    }
}

/// Servers listening on a workspace's ports, with links to preview them.
async fn get_ports(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Response {
    let host = headers
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .unwrap_or_default();
    let via_loopback = ["127.0.0.1", "localhost", "[::1]"]
        .iter()
        .any(|local| host == *local || host.starts_with(&format!("{local}:")));
    match state.daemon.forwarded_ports(&id, via_loopback).await {
        Ok(ports) => Json(ports).into_response(),
        Err(e) => command_error(e),
    }
}

/// The workspaces this one's agent started, for its board.
async fn get_children(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    match state.daemon.workspace_children(&id).await {
        Ok(children) => Json(children).into_response(),
        Err(e) => command_error(e),
    }
}

#[derive(Deserialize)]
struct HistoryQuery {
    limit: Option<i64>,
}

/// Recent events for one workspace (logs, chat items, updates), oldest first.
/// Clients call this when opening a workspace, then rely on the live stream.
async fn get_history(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<HistoryQuery>,
) -> Result<Json<Vec<Event>>, ApiError> {
    let limit = q.limit.unwrap_or(2000).clamp(1, 10_000);
    Ok(Json(
        state.daemon.store.workspace_history(&id, limit).await?,
    ))
}

async fn post_command(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(cmd): Json<Command>,
) -> Result<Response, ApiError> {
    let key = headers
        .get("idempotency-key")
        .and_then(|h| h.to_str().ok())
        .map(str::to_string);
    let store = &state.daemon.store;
    if let Some(key) = &key {
        if let Some((status, body)) = store.idempotent_response(key).await? {
            return Ok(json_response(status, body));
        }
    }

    let (status, body) = match state.daemon.execute(cmd).await {
        Ok(result) => (
            StatusCode::OK,
            CommandResponse {
                ok: true,
                result: Some(result),
                error: None,
            },
        ),
        Err(e) => {
            let status = error_status(&e);
            (
                status,
                CommandResponse {
                    ok: false,
                    result: None,
                    error: Some(format!("{e:#}")),
                },
            )
        }
    };
    let body = serde_json::to_string(&body).map_err(anyhow::Error::from)?;
    // Server errors may be transient, so only remember definitive answers.
    if let (Some(key), false) = (&key, status.is_server_error()) {
        store
            .save_idempotent_response(key, status.as_u16(), &body)
            .await?;
    }
    Ok(json_response(status.as_u16(), body))
}

fn json_response(status: u16, body: String) -> Response {
    let mut res = (StatusCode::from_u16(status).unwrap_or(StatusCode::OK), body).into_response();
    res.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    res
}

#[derive(Deserialize)]
struct EventsQuery {
    since: Option<i64>,
}

/// Replays events after `since` (or `Last-Event-ID`) from the DB, then follows
/// live events. Subscribing before replaying and de-duplicating by `seq` means
/// nothing committed in between is lost.
async fn get_events(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<EventsQuery>,
) -> impl IntoResponse {
    let last_event_id = headers
        .get("last-event-id")
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.parse::<i64>().ok());
    let since = last_event_id.or(q.since).unwrap_or(0);
    let (tx, rx) = mpsc::channel::<Result<SseEvent, Infallible>>(256);
    let daemon = state.daemon.clone();

    // Deltas are fire-and-forget: forwarded while connected, never replayed.
    let mut deltas = daemon.subscribe_deltas();
    let delta_tx = tx.clone();
    tokio::spawn(async move {
        loop {
            match deltas.recv().await {
                Ok(d) => {
                    let ev = SseEvent::default()
                        .event("delta")
                        .data(serde_json::to_string(&d).unwrap_or_default());
                    if delta_tx.send(Ok(ev)).await.is_err() {
                        return;
                    }
                }
                Err(RecvError::Lagged(_)) => continue,
                Err(RecvError::Closed) => return,
            }
        }
    });
    tokio::spawn(async move {
        let _client = daemon.activity.client();
        let mut live = daemon.store.subscribe();
        let mut last = since;
        'replay: loop {
            loop {
                let batch = match daemon.store.events_since(last, REPLAY_BATCH).await {
                    Ok(b) => b,
                    Err(e) => {
                        tracing::warn!("event replay failed: {e:#}");
                        return;
                    }
                };
                let done = (batch.len() as i64) < REPLAY_BATCH;
                for ev in batch {
                    last = ev.seq;
                    if tx.send(Ok(to_sse(&ev))).await.is_err() {
                        return;
                    }
                }
                if done {
                    break;
                }
            }
            loop {
                let next = tokio::select! {
                    ev = live.recv() => ev,
                    // The client went away; stop counting it as connected.
                    _ = tx.closed() => return,
                };
                match next {
                    Ok(ev) if ev.seq <= last => continue,
                    Ok(ev) => {
                        last = ev.seq;
                        if tx.send(Ok(to_sse(&ev))).await.is_err() {
                            return;
                        }
                    }
                    // Fell behind the broadcast buffer: catch up from the DB.
                    Err(RecvError::Lagged(_)) => continue 'replay,
                    Err(RecvError::Closed) => return,
                }
            }
        }
    });

    Sse::new(ReceiverStream::new(rx)).keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
}

fn to_sse(ev: &Event) -> SseEvent {
    SseEvent::default()
        .id(ev.seq.to_string())
        .data(serde_json::to_string(ev).unwrap_or_default())
}

pub struct ApiError(anyhow::Error);

impl<E: Into<anyhow::Error>> From<E> for ApiError {
    fn from(e: E) -> Self {
        Self(e.into())
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        tracing::error!("request failed: {:#}", self.0);
        let body = CommandResponse {
            ok: false,
            result: None,
            error: Some(format!("{:#}", self.0)),
        };
        (StatusCode::INTERNAL_SERVER_ERROR, Json(body)).into_response()
    }
}

fn error_status(e: &CommandError) -> StatusCode {
    match e {
        CommandError::BadRequest(_) => StatusCode::BAD_REQUEST,
        CommandError::NotFound(_) => StatusCode::NOT_FOUND,
        CommandError::Conflict(_) => StatusCode::CONFLICT,
        CommandError::Internal(err) => {
            tracing::error!("request failed: {err:#}");
            StatusCode::INTERNAL_SERVER_ERROR
        }
    }
}

fn command_error(e: CommandError) -> Response {
    let body = CommandResponse {
        ok: false,
        result: None,
        error: Some(format!("{e:#}")),
    };
    (error_status(&e), Json(body)).into_response()
}
