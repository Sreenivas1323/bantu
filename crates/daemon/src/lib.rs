//! The Bantu daemon (`bantud`). See `docs/ARCHITECTURE.md`.

pub mod activity;
pub mod codex;
pub mod http;
pub mod names;
pub mod ops;
pub mod permissions;
pub mod radar;
pub mod review;
pub mod service;
pub mod sessions;
pub mod settings;
pub mod store;
pub mod terminals;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};

pub use http::AppState;
pub use ops::Daemon;

#[derive(Debug, Clone)]
pub struct Config {
    /// Daemon data: `bantu.db`, `local-token`.
    pub home: PathBuf,
    /// Worktrees go in `<workspaces_dir>/<repo>/<name>`.
    pub workspaces_dir: PathBuf,
    /// Repos added by URL are cloned into `<repos_dir>/<name>`.
    pub repos_dir: PathBuf,
    /// Built web app to serve at `/`, if any.
    pub web_dir: Option<PathBuf>,
    pub branch_prefix: String,
    /// The Claude Code CLI used for agent sessions.
    pub claude_bin: PathBuf,
    /// The OpenAI Codex CLI used for agent sessions.
    pub codex_bin: PathBuf,
    /// Extra `Host` names to accept besides loopback, e.g. a Tailscale or
    /// CloudFront hostname that forwards to this daemon.
    pub allowed_hosts: Vec<String>,
}

pub struct Started {
    pub router: axum::Router,
    pub daemon: Arc<Daemon>,
    pub token: String,
}

/// Opens the store, recovers interrupted work and builds the HTTP router.
pub async fn start(config: Config, allow_remote: bool) -> Result<Started> {
    std::fs::create_dir_all(&config.home)
        .with_context(|| format!("creating {}", config.home.display()))?;
    let token = load_or_create_token(&config.home)?;
    // Agents report real paths (macOS /tmp is /private/tmp), so store ours canonical.
    let mut config = config;
    std::fs::create_dir_all(&config.workspaces_dir)
        .with_context(|| format!("creating {}", config.workspaces_dir.display()))?;
    config.workspaces_dir = config.workspaces_dir.canonicalize()?;
    let store = store::Store::open(&config.home.join("bantu.db")).await?;
    let daemon = Arc::new(Daemon::new(store, config));
    daemon.recover_interrupted().await?;
    daemon.recover_sessions().await?;
    daemon.recover_terminals().await?;
    let router = http::router(AppState {
        daemon: daemon.clone(),
        token_hash: http::hash_token(&token).into(),
        allow_remote,
    });
    Ok(Started {
        router,
        daemon,
        token,
    })
}

/// The token local clients (desktop app, CLI, browser on this machine) use.
/// Readable only by the current user.
pub fn load_or_create_token(home: &Path) -> Result<String> {
    let path = home.join("local-token");
    if let Ok(t) = std::fs::read_to_string(&path) {
        let t = t.trim().to_string();
        if !t.is_empty() {
            return Ok(t);
        }
    }
    let token = format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    write_private(&path, &token)?;
    Ok(token)
}

#[cfg(unix)]
fn write_private(path: &Path, contents: &str) -> Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
        .with_context(|| format!("writing {}", path.display()))?;
    f.write_all(contents.as_bytes())?;
    Ok(())
}

#[cfg(not(unix))]
fn write_private(path: &Path, contents: &str) -> Result<()> {
    std::fs::write(path, contents).with_context(|| format!("writing {}", path.display()))
}
