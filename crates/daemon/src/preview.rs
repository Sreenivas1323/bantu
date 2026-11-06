//! Port forwarding: open a workspace's dev server from any device.
//!
//! Previews are served on their own listener rather than the daemon's API port.
//! A page's scripts could otherwise read the daemon's token from `localStorage`
//! on the shared origin, and previewed pages run code the daemon's owner may not
//! have read. A different port is a different origin, so the token never reaches
//! that page: `GET /v1/workspaces/{id}/ports` hands out a short-lived grant for
//! one port, which the preview listener swaps for a cookie scoped to that port.
//!
//! Only a workspace's own reserved ports can be reached.

use std::sync::{Arc, OnceLock};
use std::time::Duration;

use axum::body::Body;
use axum::extract::ws::{Message as AxumMessage, WebSocket, WebSocketUpgrade};
use axum::extract::{FromRequestParts, Path, Request, State};
use axum::http::{header, HeaderMap, HeaderName, HeaderValue, StatusCode};
use axum::middleware;
use axum::response::{IntoResponse, Response};
use axum::routing::any;
use axum::Router;
use bantu_protocol::{ForwardedPort, WorkspaceState};
use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message as UpstreamMessage;

use crate::ops::{CommandError, Daemon};

const PORTS_PER_WORKSPACE: u16 = 10;
const GRANT_LIFETIME_SECS: i64 = 5 * 60;
const CONNECT_TIMEOUT: Duration = Duration::from_millis(200);
const GRANT_PARAM: &str = "bantu_grant";
/// Headers that describe one connection, not the message (RFC 9110 section 7.6.1).
const HOP_BY_HOP: [&str; 8] = [
    "connection",
    "keep-alive",
    "proxy-authenticate",
    "proxy-authorization",
    "te",
    "trailer",
    "transfer-encoding",
    "upgrade",
];

pub struct Preview {
    /// Signs grants and cookies. A fresh one per run ends old previews on restart.
    secret: [u8; 32],
    /// `http://host:port` the preview listener is bound to, once it is.
    local_base: OnceLock<String>,
    client: reqwest::Client,
}

impl Default for Preview {
    fn default() -> Self {
        let mut secret = [0u8; 32];
        getrandom::getrandom(&mut secret).expect("no system randomness");
        Self {
            secret,
            local_base: OnceLock::new(),
            client: reqwest::Client::builder()
                // Redirects go back to the browser, which follows them itself.
                .redirect(reqwest::redirect::Policy::none())
                .no_proxy()
                .build()
                .expect("HTTP client"),
        }
    }
}

impl Preview {
    fn sign(&self, message: &str) -> String {
        let mut mac =
            <Hmac<Sha256> as KeyInit>::new_from_slice(&self.secret).expect("HMAC takes any key");
        mac.update(message.as_bytes());
        hex::encode(mac.finalize().into_bytes())
    }

    fn grant(&self, workspace_id: &str, port: u16, now: i64) -> String {
        let expires = now + GRANT_LIFETIME_SECS;
        let signature = self.sign(&format!("grant|{workspace_id}|{port}|{expires}"));
        format!("{expires}.{signature}")
    }

    fn check_grant(&self, workspace_id: &str, port: u16, grant: &str, now: i64) -> bool {
        let Some((expires, signature)) = grant.split_once('.') else {
            return false;
        };
        let Ok(expiry) = expires.parse::<i64>() else {
            return false;
        };
        expiry >= now
            && constant_time_eq(
                signature,
                &self.sign(&format!("grant|{workspace_id}|{port}|{expires}")),
            )
    }

    fn cookie_value(&self, workspace_id: &str, port: u16) -> String {
        self.sign(&format!("cookie|{workspace_id}|{port}"))
    }
}

fn constant_time_eq(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0u8, |diff, (x, y)| diff | (x ^ y))
            == 0
}

/// A cookie name per port, so previews of different ports don't overwrite each
/// other's cookies. Cookies aren't isolated by port, only by path and name.
fn cookie_name(port: u16) -> String {
    format!("bantu_preview_{port}")
}

impl Daemon {
    pub fn set_preview_addr(&self, addr: std::net::SocketAddr) {
        let _ = self.preview.local_base.set(format!("http://{addr}"));
    }

    /// What's listening on a workspace's ports. `via_loopback` says the client
    /// is on this machine, so the local preview listener can serve it directly.
    pub async fn forwarded_ports(
        &self,
        workspace_id: &str,
        via_loopback: bool,
    ) -> Result<Vec<ForwardedPort>, CommandError> {
        let ws = self.require_workspace(workspace_id).await?;
        if ws.state != WorkspaceState::Ready {
            return Ok(vec![]);
        }
        let base = if via_loopback {
            self.preview.local_base.get().cloned()
        } else {
            self.config.preview_url.clone()
        };
        let now = chrono::Utc::now().timestamp();
        let mut out = Vec::new();
        for offset in 0..PORTS_PER_WORKSPACE {
            let port = ws.port_base + offset;
            let connect = tokio::net::TcpStream::connect(("127.0.0.1", port));
            if !matches!(
                tokio::time::timeout(CONNECT_TIMEOUT, connect).await,
                Ok(Ok(_))
            ) {
                continue;
            }
            let url = base.as_ref().map(|base| {
                format!(
                    "{}/p/{}/{port}/?{GRANT_PARAM}={}",
                    base.trim_end_matches('/'),
                    ws.id,
                    self.preview.grant(&ws.id, port, now)
                )
            });
            out.push(ForwardedPort { port, url });
        }
        Ok(out)
    }
}

/// The router for the preview listener. It serves nothing but previews.
pub fn router(state: crate::http::AppState) -> Router {
    Router::new()
        .route("/p/{workspace}/{port}", any(proxy_root))
        .route("/p/{workspace}/{port}/", any(proxy_root))
        .route("/p/{workspace}/{port}/{*path}", any(proxy_path))
        .fallback(any(redirect_root_path))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            crate::http::check_host,
        ))
        .with_state(state)
}

/// Apps often use root-absolute URLs (`/assets/app.js`, `/@vite/client`). Those
/// lose the `/p/...` prefix, but the `Referer` says which preview they came from.
async fn redirect_root_path(req: Request) -> Response {
    let referer = req
        .headers()
        .get(header::REFERER)
        .and_then(|h| h.to_str().ok());
    let Some(prefix) = referer.and_then(preview_prefix) else {
        return (StatusCode::NOT_FOUND, "not a preview").into_response();
    };
    let path = req.uri().path_and_query().map_or("/", |p| p.as_str());
    let target = format!("{prefix}{path}");
    match HeaderValue::from_str(&target) {
        // 307 keeps the method and body.
        Ok(location) => (
            StatusCode::TEMPORARY_REDIRECT,
            [(header::LOCATION, location)],
        )
            .into_response(),
        Err(_) => (StatusCode::BAD_REQUEST, "bad path").into_response(),
    }
}

/// `/p/<workspace>/<port>` from a URL inside a preview.
fn preview_prefix(url: &str) -> Option<String> {
    let path = url.split("://").nth(1)?;
    let path = &path[path.find('/')?..];
    let mut parts = path.split(['/', '?', '#']).skip(1);
    if parts.next()? != "p" {
        return None;
    }
    let workspace = parts.next()?;
    let port = parts.next()?;
    let safe = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric());
    (safe(workspace) && safe(port)).then(|| format!("/p/{workspace}/{port}"))
}

async fn proxy_root(
    State(state): State<crate::http::AppState>,
    Path((workspace, port)): Path<(String, u16)>,
    req: Request,
) -> Response {
    proxy(state, workspace, port, req).await
}

async fn proxy_path(
    State(state): State<crate::http::AppState>,
    Path((workspace, port, _)): Path<(String, u16, String)>,
    req: Request,
) -> Response {
    proxy(state, workspace, port, req).await
}

fn query_param<'a>(query: &'a str, name: &str) -> Option<&'a str> {
    query
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .find(|(k, _)| *k == name)
        .map(|(_, v)| v)
}

fn has_cookie(headers: &HeaderMap, name: &str, value: &str) -> bool {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|h| h.to_str().ok())
        .flat_map(|h| h.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .any(|(k, v)| k == name && constant_time_eq(v, value))
}

/// The request's `Cookie` header without ours, which the app has no use for.
fn cookies_for_app(headers: &HeaderMap, ours: &str) -> Option<String> {
    let kept: Vec<String> = headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|h| h.to_str().ok())
        .flat_map(|h| h.split(';'))
        .map(str::trim)
        .filter(|pair| !pair.is_empty() && pair.split('=').next() != Some(ours))
        .map(str::to_string)
        .collect();
    (!kept.is_empty()).then(|| kept.join("; "))
}

async fn proxy(
    state: crate::http::AppState,
    workspace_id: String,
    port: u16,
    req: Request,
) -> Response {
    let daemon = state.daemon.clone();
    let preview = &daemon.preview;
    let Ok(ws) = daemon.require_workspace(&workspace_id).await else {
        return (StatusCode::NOT_FOUND, "no such workspace").into_response();
    };
    if port < ws.port_base || port >= ws.port_base + PORTS_PER_WORKSPACE {
        return (
            StatusCode::FORBIDDEN,
            format!(
                "only this workspace's ports ({}-{}) can be previewed",
                ws.port_base,
                ws.port_base + PORTS_PER_WORKSPACE - 1
            ),
        )
            .into_response();
    }
    let query = req.uri().query().unwrap_or_default().to_string();
    let name = cookie_name(port);
    let prefix = format!("/p/{workspace_id}/{port}");
    // The path as it was sent: decoding `%2F` or `%3F` would change what it means.
    let path = req
        .uri()
        .path()
        .strip_prefix(&prefix)
        .filter(|rest| !rest.is_empty())
        .unwrap_or("/")
        .to_string();

    // A grant from the daemon's API becomes a cookie, and leaves the address bar.
    if let Some(grant) = query_param(&query, GRANT_PARAM) {
        if !preview.check_grant(&workspace_id, port, grant, chrono::Utc::now().timestamp()) {
            return (
                StatusCode::UNAUTHORIZED,
                "this preview link has expired; open it from Bantu again",
            )
                .into_response();
        }
        let secure = req
            .headers()
            .get("x-forwarded-proto")
            .is_some_and(|p| p == "https");
        let cookie = format!(
            "{name}={}; Path={prefix}; HttpOnly; SameSite=Lax{}",
            preview.cookie_value(&workspace_id, port),
            if secure { "; Secure" } else { "" }
        );
        let rest: Vec<&str> = query
            .split('&')
            .filter(|pair| !pair.starts_with(&format!("{GRANT_PARAM}=")) && !pair.is_empty())
            .collect();
        let mut location = format!("{prefix}{path}");
        if !rest.is_empty() {
            location = format!("{location}?{}", rest.join("&"));
        }
        return (
            StatusCode::FOUND,
            [(header::LOCATION, location), (header::SET_COOKIE, cookie)],
        )
            .into_response();
    }
    if !has_cookie(
        req.headers(),
        &name,
        &preview.cookie_value(&workspace_id, port),
    ) {
        return (
            StatusCode::UNAUTHORIZED,
            "open this preview from Bantu to sign in to it",
        )
            .into_response();
    }
    daemon.activity.touch();

    let target_path = if query.is_empty() {
        path
    } else {
        format!("{path}?{query}")
    };
    let is_websocket = req
        .headers()
        .get(header::UPGRADE)
        .is_some_and(|u| u.as_bytes().eq_ignore_ascii_case(b"websocket"));
    if is_websocket {
        return proxy_websocket(daemon.clone(), port, target_path, req).await;
    }

    let (parts, body) = req.into_parts();
    let url = format!("http://127.0.0.1:{port}{target_path}");
    let mut upstream = preview.client.request(parts.method, &url);
    for (key, value) in &parts.headers {
        if !forwardable(key) {
            continue;
        }
        upstream = upstream.header(key, value);
    }
    if let Some(cookies) = cookies_for_app(&parts.headers, &name) {
        upstream = upstream.header(header::COOKIE, cookies);
    }
    // Dev servers commonly refuse Host names they don't recognize.
    upstream = upstream.header(header::HOST, format!("localhost:{port}"));
    let response = upstream
        .body(reqwest::Body::wrap_stream(body.into_data_stream()))
        .send()
        .await;
    let response = match response {
        Ok(r) => r,
        Err(_) => {
            return (
                StatusCode::BAD_GATEWAY,
                format!("nothing answered on port {port}; is the dev server running?"),
            )
                .into_response()
        }
    };

    let mut out = Response::builder().status(response.status());
    for (key, value) in response.headers() {
        if HOP_BY_HOP.contains(&key.as_str()) {
            continue;
        }
        if key == header::LOCATION {
            if let Some(location) = value
                .to_str()
                .ok()
                .and_then(|l| rewrite_location(l, port, &prefix))
            {
                if let Ok(value) = HeaderValue::from_str(&location) {
                    out = out.header(header::LOCATION, value);
                    continue;
                }
            }
        }
        if key == header::SET_COOKIE {
            let scoped = value
                .to_str()
                .ok()
                .and_then(|c| HeaderValue::from_str(&scope_cookie(c, &prefix)).ok());
            if let Some(scoped) = scoped {
                out = out.header(key, scoped);
                continue;
            }
        }
        out = out.header(key, value);
    }
    out.body(Body::from_stream(response.bytes_stream()))
        .unwrap_or_else(|_| StatusCode::BAD_GATEWAY.into_response())
}

/// What goes on to the app. Our credentials and the routing headers don't.
fn forwardable(name: &HeaderName) -> bool {
    !HOP_BY_HOP.contains(&name.as_str())
        && *name != header::HOST
        && *name != header::COOKIE
        && *name != header::AUTHORIZATION
}

/// Keeps an app's cookie inside its preview. Previews share an origin, so one
/// with `Path=/` would otherwise reach every other preview too.
fn scope_cookie(cookie: &str, prefix: &str) -> String {
    cookie
        .split(';')
        .map(|part| {
            let trimmed = part.trim();
            match trimmed.split_once('=') {
                Some((name, value))
                    if name.eq_ignore_ascii_case("path") && value.starts_with('/') =>
                {
                    format!(" Path={prefix}{}", value.trim_end_matches('/'))
                }
                _ => part.to_string(),
            }
        })
        .collect::<Vec<_>>()
        .join(";")
}

/// Keeps a redirect inside the preview: `/login` and `http://localhost:55000/login`
/// both become `/p/<workspace>/55000/login`. Other sites are left alone.
fn rewrite_location(location: &str, port: u16, prefix: &str) -> Option<String> {
    for origin in [
        format!("http://localhost:{port}"),
        format!("http://127.0.0.1:{port}"),
    ] {
        if let Some(rest) = location.strip_prefix(&origin) {
            return Some(format!(
                "{prefix}{}",
                if rest.is_empty() { "/" } else { rest }
            ));
        }
    }
    (location.starts_with('/') && !location.starts_with("//"))
        .then(|| format!("{prefix}{location}"))
}

async fn proxy_websocket(daemon: Arc<Daemon>, port: u16, target: String, req: Request) -> Response {
    let (mut parts, _) = req.into_parts();
    let upgrade = match WebSocketUpgrade::from_request_parts(&mut parts, &()).await {
        Ok(u) => u,
        Err(e) => return e.into_response(),
    };
    let protocols: Vec<String> = parts
        .headers
        .get(header::SEC_WEBSOCKET_PROTOCOL)
        .and_then(|p| p.to_str().ok())
        .map(|p| p.split(',').map(|s| s.trim().to_string()).collect())
        .unwrap_or_default();
    let mut request = match format!("ws://127.0.0.1:{port}{target}").into_client_request() {
        Ok(r) => r,
        Err(_) => return (StatusCode::BAD_REQUEST, "bad path").into_response(),
    };
    if let Some(value) = parts.headers.get(header::SEC_WEBSOCKET_PROTOCOL) {
        request
            .headers_mut()
            .insert(header::SEC_WEBSOCKET_PROTOCOL, value.clone());
    }
    let (upstream, _) = match tokio_tungstenite::connect_async(request).await {
        Ok(pair) => pair,
        Err(_) => {
            return (
                StatusCode::BAD_GATEWAY,
                format!("nothing accepted a WebSocket on port {port}"),
            )
                .into_response()
        }
    };
    upgrade
        .protocols(protocols)
        .on_upgrade(move |socket| async move {
            let _client = daemon.activity.client();
            pump(socket, upstream).await;
        })
        .into_response()
}

async fn pump<S>(socket: WebSocket, upstream: tokio_tungstenite::WebSocketStream<S>)
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    use futures::{SinkExt, StreamExt};
    let (mut client_tx, mut client_rx) = socket.split();
    let (mut upstream_tx, mut upstream_rx) = upstream.split();
    let to_upstream = async {
        while let Some(Ok(message)) = client_rx.next().await {
            let message = match message {
                AxumMessage::Text(text) => UpstreamMessage::text(text.as_str()),
                AxumMessage::Binary(bytes) => UpstreamMessage::Binary(bytes),
                AxumMessage::Ping(bytes) => UpstreamMessage::Ping(bytes),
                AxumMessage::Pong(bytes) => UpstreamMessage::Pong(bytes),
                AxumMessage::Close(_) => break,
            };
            if upstream_tx.send(message).await.is_err() {
                break;
            }
        }
    };
    let to_client = async {
        while let Some(Ok(message)) = upstream_rx.next().await {
            let message = match message {
                UpstreamMessage::Text(text) => AxumMessage::text(text.as_str()),
                UpstreamMessage::Binary(bytes) => AxumMessage::Binary(bytes),
                UpstreamMessage::Ping(bytes) => AxumMessage::Ping(bytes),
                UpstreamMessage::Pong(bytes) => AxumMessage::Pong(bytes),
                UpstreamMessage::Close(_) => break,
                UpstreamMessage::Frame(_) => continue,
            };
            if client_tx.send(message).await.is_err() {
                break;
            }
        }
    };
    tokio::select! {
        _ = to_upstream => {}
        _ = to_client => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grants_expire_and_are_scoped() {
        let p = Preview::default();
        let grant = p.grant("w1", 55000, 1000);
        assert!(p.check_grant("w1", 55000, &grant, 1000));
        assert!(p.check_grant("w1", 55000, &grant, 1000 + GRANT_LIFETIME_SECS));
        assert!(!p.check_grant("w1", 55000, &grant, 1001 + GRANT_LIFETIME_SECS));
        assert!(!p.check_grant("w1", 55001, &grant, 1000), "another port");
        assert!(
            !p.check_grant("w2", 55000, &grant, 1000),
            "another workspace"
        );
        assert!(!p.check_grant("w1", 55000, "9999999999.forged", 1000));
        assert!(!p.check_grant("w1", 55000, "garbage", 1000));
        assert_ne!(p.cookie_value("w1", 55000), p.cookie_value("w1", 55001));
        assert_ne!(Preview::default().grant("w1", 55000, 1000), grant);
    }

    #[test]
    fn keeps_redirects_inside_the_preview() {
        let prefix = "/p/w1/55000";
        let rewrite = |l| rewrite_location(l, 55000, prefix);
        assert_eq!(rewrite("/login").as_deref(), Some("/p/w1/55000/login"));
        assert_eq!(
            rewrite("http://localhost:55000/a?b=1").as_deref(),
            Some("/p/w1/55000/a?b=1")
        );
        assert_eq!(
            rewrite("http://127.0.0.1:55000").as_deref(),
            Some("/p/w1/55000/")
        );
        assert_eq!(rewrite("https://accounts.example.com/x"), None);
        assert_eq!(rewrite("//evil.example/x"), None);
        assert_eq!(rewrite("relative/path"), None);
    }

    #[test]
    fn scopes_app_cookies_to_the_preview() {
        let prefix = "/p/w1/55000";
        assert_eq!(
            scope_cookie("sid=1; Path=/; HttpOnly", prefix),
            "sid=1; Path=/p/w1/55000; HttpOnly"
        );
        assert_eq!(
            scope_cookie("sid=1; path=/app/", prefix),
            "sid=1; Path=/p/w1/55000/app"
        );
        assert_eq!(scope_cookie("sid=1; Secure", prefix), "sid=1; Secure");
    }

    #[test]
    fn finds_the_preview_from_a_referer() {
        assert_eq!(
            preview_prefix("https://box.ts.net:8443/p/abc123/55000/index.html?x=1").as_deref(),
            Some("/p/abc123/55000")
        );
        assert_eq!(
            preview_prefix("http://127.0.0.1:47801/p/abc123/55000").as_deref(),
            Some("/p/abc123/55000")
        );
        assert_eq!(preview_prefix("https://example.com/other/page"), None);
        assert_eq!(preview_prefix("https://example.com/p/../55000"), None);
        assert_eq!(preview_prefix("not a url"), None);
    }

    #[test]
    fn strips_only_our_cookie() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            "theme=dark; bantu_preview_55000=secret; session=abc"
                .parse()
                .unwrap(),
        );
        assert_eq!(
            cookies_for_app(&headers, "bantu_preview_55000").as_deref(),
            Some("theme=dark; session=abc")
        );
        assert!(has_cookie(&headers, "bantu_preview_55000", "secret"));
        assert!(!has_cookie(&headers, "bantu_preview_55000", "wrong"));
        assert!(!forwardable(&header::AUTHORIZATION));
        assert!(!forwardable(&header::COOKIE));
        assert!(forwardable(&header::ACCEPT));
    }
}
