//! Bantu desktop shell.
//!
//! The window first shows a bundled splash page. A background thread makes sure
//! `bantud` is running (starting it if needed), reads the local token, and then
//! points the window at the web UI the daemon serves. The daemon is started in
//! its own process group, so agents keep running after the window closes.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::env;
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use tauri::{Manager, Url, WebviewWindow};

const DEFAULT_ADDR: &str = "127.0.0.1:47800";
const DAEMON_START_TIMEOUT: Duration = Duration::from_secs(15);
const TOKEN_TIMEOUT: Duration = Duration::from_secs(5);
const POLL_INTERVAL: Duration = Duration::from_millis(200);

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            let window = app
                .get_webview_window("main")
                .expect("tauri.conf.json defines the main window");
            thread::spawn(move || {
                if let Err(message) = connect(&window) {
                    eprintln!("bantu-desktop: {message}");
                    let script = format!(
                        "window.bantuShowError({})",
                        serde_json::Value::String(message)
                    );
                    let _ = window.eval(&script);
                }
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running the Bantu desktop app");
}

fn connect(window: &WebviewWindow) -> Result<(), String> {
    let addr = env::var("BANTU_DAEMON_ADDR").unwrap_or_else(|_| DEFAULT_ADDR.to_string());
    let home = bantu_home()?;

    if !daemon_healthy(&addr) {
        let bin = find_bantud().ok_or_else(|| {
            "Couldn't find the `bantud` binary. Install it next to the app or on your PATH, \
             or set BANTU_DAEMON_BIN."
                .to_string()
        })?;
        spawn_daemon(&bin, &home, &addr)?;
        wait_until(DAEMON_START_TIMEOUT, || daemon_healthy(&addr).then_some(())).ok_or_else(|| {
            format!(
                "Started {} but it didn't answer on {addr} within {}s.\nSee {}",
                bin.display(),
                DAEMON_START_TIMEOUT.as_secs(),
                home.join("logs/bantud.log").display()
            )
        })?;
    }

    let token_path = home.join("local-token");
    let token = wait_until(TOKEN_TIMEOUT, || read_token(&token_path)).ok_or_else(|| {
        format!(
            "The daemon is running but {} is missing or empty.",
            token_path.display()
        )
    })?;

    // In development BANTU_DESKTOP_URL can point at the Vite dev server instead.
    let base = env::var("BANTU_DESKTOP_URL").unwrap_or_else(|_| format!("http://{addr}/"));
    let mut url = Url::parse(&base).map_err(|e| format!("Invalid UI URL {base}: {e}"))?;
    url.set_fragment(Some(&format!("token={token}")));
    window
        .navigate(url)
        .map_err(|e| format!("Couldn't open the Bantu UI: {e}"))
}

fn bantu_home() -> Result<PathBuf, String> {
    if let Some(dir) = env::var_os("BANTU_HOME") {
        return Ok(PathBuf::from(dir));
    }
    env::var_os("HOME")
        .or_else(|| env::var_os("USERPROFILE"))
        .map(|home| PathBuf::from(home).join(".bantu"))
        .ok_or_else(|| "Couldn't determine your home directory; set BANTU_HOME.".to_string())
}

/// A dependency-free `GET /v1/health`; the endpoint needs no token.
fn daemon_healthy(addr: &str) -> bool {
    let Ok(sock) = addr.parse::<SocketAddr>() else {
        return false;
    };
    let Ok(mut stream) = TcpStream::connect_timeout(&sock, Duration::from_millis(300)) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(Duration::from_secs(1)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(1)));
    if write!(stream, "GET /v1/health HTTP/1.0\r\nHost: {addr}\r\n\r\n").is_err() {
        return false;
    }
    let mut response = String::new();
    let _ = stream.read_to_string(&mut response);
    let status_ok = response
        .lines()
        .next()
        .is_some_and(|line| line.split_whitespace().nth(1) == Some("200"));
    status_ok && response.contains("\"ok\":true")
}

fn find_bantud() -> Option<PathBuf> {
    let name = if cfg!(windows) { "bantud.exe" } else { "bantud" };
    let mut candidates = Vec::new();
    if let Some(bin) = env::var_os("BANTU_DAEMON_BIN") {
        candidates.push(PathBuf::from(bin));
    }
    if let Some(dir) = env::current_exe().ok().and_then(|exe| exe.parent().map(Path::to_path_buf)) {
        candidates.push(dir.join(name));
    }
    if let Some(path) = env::var_os("PATH") {
        candidates.extend(env::split_paths(&path).map(|dir| dir.join(name)));
    }
    if cfg!(debug_assertions) {
        candidates.push(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../target/debug").join(name));
    }
    candidates.into_iter().find(|p| p.is_file())
}

fn spawn_daemon(bin: &Path, home: &Path, addr: &str) -> Result<(), String> {
    let logs = home.join("logs");
    fs::create_dir_all(&logs).map_err(|e| format!("Couldn't create {}: {e}", logs.display()))?;
    let log_path = logs.join("bantud.log");
    let log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .map_err(|e| format!("Couldn't open {}: {e}", log_path.display()))?;
    let log_err = log.try_clone().map_err(|e| e.to_string())?;

    let mut cmd = Command::new(bin);
    cmd.stdin(Stdio::null()).stdout(log).stderr(log_err);
    // Start the daemon where we'll look for it (BANTU_DAEMON_ADDR is host:port).
    if let Some((host, port)) = addr.rsplit_once(':') {
        cmd.env("BANTUD_HOST", host.trim_matches(['[', ']']))
            .env("BANTUD_PORT", port);
    }
    if cfg!(debug_assertions) && env::var_os("BANTU_WEB_DIR").is_none() {
        // Serve the freshly built web app from the repo while developing.
        cmd.env(
            "BANTU_WEB_DIR",
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../web/dist"),
        );
    }
    #[cfg(unix)]
    {
        // Own process group: quitting the app (or Ctrl-C in `tauri dev`) must not stop agents.
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    cmd.spawn()
        .map(drop)
        .map_err(|e| format!("Couldn't start {}: {e}", bin.display()))
}

fn read_token(path: &Path) -> Option<String> {
    let token = fs::read_to_string(path).ok()?.trim().to_string();
    (!token.is_empty()).then_some(token)
}

fn wait_until<T>(timeout: Duration, mut check: impl FnMut() -> Option<T>) -> Option<T> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(value) = check() {
            return Some(value);
        }
        if Instant::now() >= deadline {
            return None;
        }
        thread::sleep(POLL_INTERVAL);
    }
}
