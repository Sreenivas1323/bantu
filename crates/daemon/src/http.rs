//! HTTP API: `/v1/health`, `/v1/state`, `/v1/commands`, `/v1/events` (SSE),
//! plus the built web app at `/`.

use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

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

pub const DAEMON_VERSION: &str = env!("CARGO_PKG_VERSION");
const REPLAY_BATCH: i64 = 500;

#[derive(Clone)]
pub struct AppState {
    pub daemon: Arc<Daemon>,
    pub token_hash: Arc<[u8]>,
    pub allow_remote: bool,
}

pub fn router(state: AppState) -> Router {
    let api = Router::new()
        .route("/v1/state", get(get_state))
        .route("/v1/commands", post(post_command))
        .route("/v1/events", get(get_events))
        .route("/v1/workspaces/{id}/history", get(get_history))
        .route("/v1/workspaces/{id}/diff", get(get_diff))
        .route("/v1/workspaces/{id}/pr", get(get_pr))
        .route_layer(middleware::from_fn_with_state(state.clone(), require_token));

    let mut app = Router::new()
        .route("/v1/health", get(health))
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
    .layer(middleware::from_fn_with_state(state, check_host))
}

pub fn hash_token(token: &str) -> Vec<u8> {
    Sha256::digest(token.as_bytes()).to_vec()
}

/// Rejects requests whose Host isn't loopback, blocking DNS-rebinding attacks
/// from web pages, unless the daemon was started with `--allow-remote`.
async fn check_host(State(state): State<AppState>, req: Request, next: Next) -> Response {
    if !state.allow_remote {
        let host = req
            .headers()
            .get(header::HOST)
            .and_then(|h| h.to_str().ok())
            .unwrap_or("");
        let hostname = host
            .strip_prefix('[')
            .and_then(|h| h.split(']').next())
            .unwrap_or_else(|| host.split(':').next().unwrap_or(""));
        if !matches!(hostname, "127.0.0.1" | "localhost" | "::1") {
            return (StatusCode::FORBIDDEN, "host not allowed").into_response();
        }
    }
    next.run(req).await
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
    }))
}

async fn get_diff(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    match state.daemon.workspace_diff(&id).await {
        Ok(diff) => Json(diff).into_response(),
        Err(e) => command_error(e),
    }
}

/// `null` when the branch has no PR, the repo has no remote, or `gh` is missing.
async fn get_pr(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    match state.daemon.pull_request(&id).await {
        Ok(pr) => Json(pr).into_response(),
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

    tokio::spawn(async move {
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
                match live.recv().await {
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
