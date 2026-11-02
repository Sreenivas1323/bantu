use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use bantud::Config;
use clap::{Parser, Subcommand};

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
    /// Where repos added by URL are cloned. Defaults to ~/bantu/repos.
    #[arg(long, env = "BANTU_REPOS_DIR")]
    repos_dir: Option<PathBuf>,
    /// Built web app to serve at `/`.
    #[arg(long, env = "BANTU_WEB_DIR")]
    web_dir: Option<PathBuf>,
    #[arg(long, env = "BANTU_BRANCH_PREFIX", default_value = "bantu/")]
    branch_prefix: String,
    /// Claude Code CLI used for agent sessions.
    #[arg(long, env = "BANTU_CLAUDE_BIN", default_value = "claude")]
    claude_bin: PathBuf,
    /// OpenAI Codex CLI used for agent sessions.
    #[arg(long, env = "BANTU_CODEX_BIN", default_value = "codex")]
    codex_bin: PathBuf,
    /// Accept non-loopback binds and any Host header. Prefer --allowed-host.
    #[arg(long)]
    allow_remote: bool,
    /// Accept requests for this hostname too (repeatable), e.g. the name a
    /// Tailscale Serve, Cloudflare Tunnel or CloudFront proxy forwards.
    #[arg(
        long = "allowed-host",
        env = "BANTU_ALLOWED_HOSTS",
        value_delimiter = ','
    )]
    allowed_hosts: Vec<String>,
    /// URL other devices use to reach this daemon (e.g. https://box.tail1234.ts.net).
    /// Its host is allowed automatically and `bantud url` prints it.
    #[arg(long, env = "BANTU_PUBLIC_URL")]
    public_url: Option<String>,
    /// Print the local access token and exit.
    #[arg(long)]
    print_token: bool,
    #[command(subcommand)]
    command: Option<Cmd>,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Run bantud in the background at login (launchd on macOS, systemd on Linux).
    Service {
        #[command(subcommand)]
        action: ServiceAction,
    },
    /// Print the web app URL with this machine's token (`--open` to open it).
    Url {
        #[arg(long)]
        open: bool,
        /// Also print a QR code to scan with a phone.
        #[arg(long)]
        qr: bool,
    },
}

#[derive(Subcommand, Debug)]
enum ServiceAction {
    /// Register and start the service (replacing any previous one).
    Install,
    /// Stop and remove the service; data in ~/.bantu is kept.
    Uninstall,
    /// Show whether the service is running.
    Status,
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

    match &args.command {
        Some(Cmd::Service { action }) => {
            return match action {
                ServiceAction::Install => {
                    let exe = std::env::current_exe()?.canonicalize()?;
                    bantud::service::install(&exe, &home)
                }
                ServiceAction::Uninstall => bantud::service::uninstall(),
                ServiceAction::Status => bantud::service::status(),
            };
        }
        Some(Cmd::Url { open, qr }) => {
            std::fs::create_dir_all(&home)?;
            let token = bantud::load_or_create_token(&home)?;
            let base = args
                .public_url
                .clone()
                .unwrap_or_else(|| format!("http://127.0.0.1:{}", args.port));
            let url = format!("{}/#token={token}", base.trim_end_matches('/'));
            println!("{url}");
            if *qr {
                println!("\n{}", qr_code(&url)?);
                if args.public_url.is_none() {
                    println!(
                        "This is a loopback URL; set BANTU_PUBLIC_URL to pair another device."
                    );
                }
            }
            if *open {
                let opener = if cfg!(target_os = "macos") {
                    "open"
                } else {
                    "xdg-open"
                };
                let _ = std::process::Command::new(opener).arg(&url).status();
            }
            return Ok(());
        }
        None => {}
    }
    if args.print_token {
        std::fs::create_dir_all(&home)?;
        println!("{}", bantud::load_or_create_token(&home)?);
        return Ok(());
    }
    if !args.host.is_loopback() && !args.allow_remote {
        bail!("refusing to bind {} without --allow-remote", args.host);
    }

    let mut allowed_hosts = args.allowed_hosts.clone();
    if let Some(url) = &args.public_url {
        if let Some(host) = url_host(url) {
            allowed_hosts.push(host);
        }
    }
    let config = Config {
        home,
        workspaces_dir: args
            .workspaces_dir
            .unwrap_or_else(|| home_dir.join("bantu").join("workspaces")),
        repos_dir: args
            .repos_dir
            .unwrap_or_else(|| home_dir.join("bantu").join("repos")),
        web_dir: args.web_dir.or_else(default_web_dir),
        branch_prefix: args.branch_prefix,
        claude_bin: args.claude_bin,
        codex_bin: args.codex_bin,
        allowed_hosts,
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
        // Inside the macOS app bundle: Contents/MacOS/bantud → Contents/Resources/web.
        exe_dir.join("../Resources/web"),
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

/// `https://box.example.ts.net:8443/x` → `box.example.ts.net`.
fn url_host(url: &str) -> Option<String> {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let authority = rest.split('/').next()?;
    let host = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    let host = if let Some(v6) = host.strip_prefix('[') {
        v6.split(']').next()?
    } else {
        host.split(':').next()?
    };
    (!host.is_empty()).then(|| host.to_ascii_lowercase())
}

/// A QR code drawn with half-block characters so it fits in a terminal.
fn qr_code(text: &str) -> Result<String> {
    let code = qrcode::QrCode::new(text.as_bytes()).context("encoding the QR code")?;
    let width = code.width();
    let dark = |x: i64, y: i64| {
        x >= 0
            && y >= 0
            && (x as usize) < width
            && (y as usize) < width
            && code[(x as usize, y as usize)] == qrcode::Color::Dark
    };
    let mut out = String::new();
    // One quiet-zone module around the code; two rows per text line.
    for y in (-1..=width as i64).step_by(2) {
        for x in -1..=width as i64 {
            out.push(match (dark(x, y), dark(x, y + 1)) {
                (true, true) => ' ',
                (true, false) => '▄',
                (false, true) => '▀',
                (false, false) => '█',
            });
        }
        out.push('\n');
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_hosts() {
        assert_eq!(
            url_host("https://Box.tail1234.ts.net").as_deref(),
            Some("box.tail1234.ts.net")
        );
        assert_eq!(
            url_host("https://d111.cloudfront.net:443/app").as_deref(),
            Some("d111.cloudfront.net")
        );
        assert_eq!(url_host("http://[::1]:47800").as_deref(), Some("::1"));
        assert_eq!(url_host("https://"), None);
    }
}
