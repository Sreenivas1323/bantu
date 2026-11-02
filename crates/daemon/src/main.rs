use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use bantud::Config;
use clap::Parser;

/// The Bantu daemon: runs coding agents in isolated git worktrees.
#[derive(Parser, Debug)]
#[command(version, about)]
struct Args {
    /// Address to bind. Non-loopback addresses require --allow-remote.
    #[arg(long, env = "BANTUD_HOST", default_value = "127.0.0.1")]
    host: IpAddr,
    #[arg(long, env = "BANTUD_PORT", default_value_t = 47800)]
    port: u16,
    /// Data directory (database, token). Defaults to ~/.bantu.
    #[arg(long, env = "BANTU_HOME")]
    home: Option<PathBuf>,
    /// Where worktrees are created. Defaults to ~/bantu/workspaces.
    #[arg(long, env = "BANTU_WORKSPACES_DIR")]
    workspaces_dir: Option<PathBuf>,
    /// Built web app to serve at `/`.
    #[arg(long, env = "BANTU_WEB_DIR")]
    web_dir: Option<PathBuf>,
    #[arg(long, env = "BANTU_BRANCH_PREFIX", default_value = "bantu/")]
    branch_prefix: String,
    /// Claude Code CLI used for agent sessions.
    #[arg(long, env = "BANTU_CLAUDE_BIN", default_value = "claude")]
    claude_bin: PathBuf,
    /// Accept non-loopback binds and Host headers. Only use behind an
    /// authenticated tunnel (Tailscale, Cloudflare Tunnel, CloudFront).
    #[arg(long)]
    allow_remote: bool,
    /// Print the local access token and exit.
    #[arg(long)]
    print_token: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "bantud=info,tower_http=warn".into()),
        )
        .init();

    let args = Args::parse();
    if std::env::var_os("BANTU_SKIP_SHELL_ENV").is_none() {
        adopt_login_shell_path().await;
    }
    let home_dir = std::env::var_os("HOME")
        .map(PathBuf::from)
        .context("HOME is not set")?;
    let home = args.home.unwrap_or_else(|| home_dir.join(".bantu"));

    if args.print_token {
        std::fs::create_dir_all(&home)?;
        println!("{}", bantud::load_or_create_token(&home)?);
        return Ok(());
    }
    if !args.host.is_loopback() && !args.allow_remote {
        bail!("refusing to bind {} without --allow-remote", args.host);
    }

    let config = Config {
        home,
        workspaces_dir: args
            .workspaces_dir
            .unwrap_or_else(|| home_dir.join("bantu").join("workspaces")),
        web_dir: args.web_dir.or_else(default_web_dir),
        branch_prefix: args.branch_prefix,
        claude_bin: args.claude_bin,
    };
    let web_dir = config.web_dir.clone();
    let started = bantud::start(config, args.allow_remote).await?;

    let addr = SocketAddr::new(args.host, args.port);
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("binding {addr} (is another bantud running?)"))?;
    started.daemon.set_listen_addr(listener.local_addr()?);
    tracing::info!(
        "bantud {} listening on http://{addr}",
        env!("CARGO_PKG_VERSION")
    );
    match web_dir {
        Some(dir) => tracing::info!(
            "web app ({}) at http://{addr}/#token={}",
            dir.display(),
            started.token
        ),
        None => tracing::info!("no web app found; build apps/web or pass --web-dir"),
    }

    axum::serve(listener, started.router)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}

/// Looks for the web app next to the binary (packaged installs), then in the
/// source tree (development).
fn default_web_dir() -> Option<PathBuf> {
    let exe_dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
    [
        exe_dir.join("../share/bantu/web"),
        exe_dir.join("web"),
        PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../apps/web/dist")),
    ]
    .into_iter()
    .find(|p| p.join("index.html").is_file())
    .and_then(|p| p.canonicalize().ok())
}

/// Apps launched from the Dock get a minimal PATH, so agents couldn't find
/// `claude`, `bun`, `node`, etc. Borrow PATH from the user's login shell,
/// which is what they'd have in a terminal.
async fn adopt_login_shell_path() {
    const MARK: &str = "__BANTU_PATH__";
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into());
    let probe = tokio::process::Command::new(&shell)
        .args(["-ilc", &format!("printf '{MARK}%s{MARK}' \"$PATH\"")])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .output();
    let out = match tokio::time::timeout(std::time::Duration::from_secs(5), probe).await {
        Ok(Ok(out)) => String::from_utf8_lossy(&out.stdout).to_string(),
        _ => {
            tracing::warn!("couldn't read PATH from {shell}; keeping the inherited PATH");
            return;
        }
    };
    if let Some(path) = out.split(MARK).nth(1).filter(|p| !p.is_empty()) {
        // Safe: called at startup before any other threads read the environment.
        std::env::set_var("PATH", path);
        tracing::debug!("using PATH from {shell}: {path}");
    }
}
