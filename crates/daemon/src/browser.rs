//! An opt-in headless browser for agents, so they can check their own app.
//!
//! When a chat has `browser` on, its agent also gets the Playwright MCP server
//! (`npx @playwright/mcp@latest`), started by the agent CLI itself over stdio.
//! The browser is headless, keeps its profile in memory, and may only reach
//! this workspace's reserved ports on `localhost` and `127.0.0.1`.
//!
//! Playwright's `--allowed-origins` is not a security boundary: it filters
//! requests the page makes but not redirects or WebSockets. So every other
//! request is also sent to a proxy that isn't there, with only the
//! workspace's ports exempt. A redirect to the internet or to another
//! workspace's port then fails with `ERR_PROXY_CONNECTION_FAILED`.

use std::path::{Path, PathBuf};

use bantu_protocol::{ChatItem, Session, Workspace};
use serde_json::{json, Value};

use crate::ops::Daemon;

/// The MCP server's name, so its tools are `mcp__browser__browser_navigate` etc.
pub const SERVER_NAME: &str = "browser";
pub const PACKAGE: &str = "@playwright/mcp@latest";
/// Nothing listens here, so requests sent to this "proxy" fail.
const DEAD_PROXY: &str = "http://127.0.0.1:9";
/// Ports reserved for each workspace, starting at `port_base` (`$BANTU_PORT`).
const PORTS: u16 = 10;
const HOSTS: [&str; 2] = ["localhost", "127.0.0.1"];

/// One chat's browser: the Playwright MCP server, run through `npx`.
#[derive(Debug, Clone, PartialEq)]
pub struct BrowserServer {
    /// Absolute path to `npx`. ACP requires an absolute command.
    pub command: PathBuf,
    pub args: Vec<String>,
    pub port_base: u16,
}

impl BrowserServer {
    /// `None` when `npx` can't be found.
    pub fn new(npx: &Path, ws: &Workspace) -> Option<Self> {
        let output = std::env::temp_dir().join("bantu-browser").join(&ws.id);
        Some(Self {
            command: find_program(npx)?,
            args: args(ws.port_base, &output),
            port_base: ws.port_base,
        })
    }

    /// An entry for Claude Code's `--mcp-config` `mcpServers`.
    pub fn claude_entry(&self) -> Value {
        json!({
            "type": "stdio",
            "command": self.command.to_string_lossy(),
            "args": self.args,
        })
    }

    /// An ACP `McpServer` (stdio, which every ACP agent supports).
    pub fn acp_entry(&self) -> Value {
        json!({
            "name": SERVER_NAME,
            "command": self.command.to_string_lossy(),
            "args": self.args,
            "env": [],
        })
    }

    /// One line for the agent's briefing: where its app runs.
    pub fn note(&self) -> String {
        let p = self.port_base;
        format!(
            "A headless browser is available through the `{SERVER_NAME}` MCP tools (Playwright). \
             Run the dev server on http://localhost:{p} ($BANTU_PORT) and open that URL to check \
             your work; the browser can only reach localhost and 127.0.0.1 on ports {p}-{}.",
            p + PORTS - 1
        )
    }
}

/// `npx` arguments for the Playwright MCP server, limited to one workspace's ports.
pub fn args(port_base: u16, output_dir: &Path) -> Vec<String> {
    vec![
        "-y".into(),
        PACKAGE.into(),
        "--headless".into(),
        "--isolated".into(),
        "--allowed-origins".into(),
        allowed_origins(port_base).join(";"),
        "--proxy-server".into(),
        DEAD_PROXY.into(),
        "--proxy-bypass".into(),
        allowed_hosts(port_base).join(","),
        // Screenshots would otherwise land in the worktree, as `.playwright-mcp/`.
        "--output-dir".into(),
        output_dir.to_string_lossy().into_owned(),
    ]
}

/// `http` and `https` origins for each of the workspace's ports.
pub fn allowed_origins(port_base: u16) -> Vec<String> {
    allowed_hosts(port_base)
        .iter()
        .flat_map(|host| ["http", "https"].map(|scheme| format!("{scheme}://{host}")))
        .collect()
}

/// `localhost:<port>` and `127.0.0.1:<port>` for each of the workspace's ports.
fn allowed_hosts(port_base: u16) -> Vec<String> {
    (0..PORTS)
        .filter_map(|i| port_base.checked_add(i))
        .flat_map(|port| HOSTS.map(|host| format!("{host}:{port}")))
        .collect()
}

/// Adds the browser to a `--mcp-config` JSON string.
pub fn with_browser(config: Option<String>, browser: Option<&BrowserServer>) -> Option<String> {
    let Some(browser) = browser else {
        return config;
    };
    let mut value: Value = config
        .and_then(|c| serde_json::from_str(&c).ok())
        .unwrap_or_else(|| json!({}));
    if !value["mcpServers"].is_object() {
        value["mcpServers"] = json!({});
    }
    value["mcpServers"][SERVER_NAME] = browser.claude_entry();
    Some(value.to_string())
}

/// The workspace briefing, plus a line about the browser when there is one.
pub fn with_note(context: String, browser: Option<&BrowserServer>) -> String {
    match browser {
        Some(b) => format!("{context}\n- {}", b.note()),
        None => context,
    }
}

fn missing_message(npx: &Path) -> String {
    format!(
        "No browser for this chat: `{}` isn't on the daemon's PATH. Install Node.js (it \
         includes npx) and the next message will have one.",
        npx.display()
    )
}

/// Resolves a program name the way process creation would, through `PATH`.
fn find_program(bin: &Path) -> Option<PathBuf> {
    let bin = crate::platform::program(bin);
    if bin.components().count() > 1 {
        return bin.is_file().then_some(bin);
    }
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|dir| dir.join(&bin))
        .find(|path| path.is_file())
}

impl Daemon {
    /// This turn's browser, if the chat has it on. When it's on but `npx`
    /// is missing, the turn still runs and the chat says why, once.
    pub(crate) async fn browser_for_turn(
        &self,
        s: &Session,
        ws: &Workspace,
        turn: u32,
    ) -> Option<BrowserServer> {
        if !s.browser {
            return None;
        }
        if let Some(browser) = BrowserServer::new(&self.config.npx_bin, ws) {
            return Some(browser);
        }
        let notice = ChatItem::Notice {
            message: missing_message(&self.config.npx_bin),
        };
        let items = self.store.session_items(&s.id).await.unwrap_or_default();
        if !items.iter().any(|(_, item)| *item == notice) {
            let _ = self.store.session_item(s, turn, notice).await;
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn server(port_base: u16) -> BrowserServer {
        BrowserServer {
            command: PathBuf::from("/usr/bin/npx"),
            args: args(port_base, Path::new("/tmp/out")),
            port_base,
        }
    }

    fn flag<'a>(args: &'a [String], name: &str) -> &'a str {
        let i = args.iter().position(|a| a == name).unwrap();
        &args[i + 1]
    }

    #[test]
    fn origins_are_only_the_workspace_ports() {
        let origins = allowed_origins(55010);
        assert_eq!(origins.len(), 40);
        for port in 55010..=55019 {
            for host in ["localhost", "127.0.0.1"] {
                assert!(origins.contains(&format!("http://{host}:{port}")));
                assert!(origins.contains(&format!("https://{host}:{port}")));
            }
        }
        assert!(origins
            .iter()
            .all(|o| !o.ends_with(":55009") && !o.ends_with(":55020")));
        // Every origin names its host and port exactly: no wildcards.
        assert!(origins.iter().all(|o| !o.contains('*')));
        // Ports near the top of the range don't wrap around.
        assert_eq!(allowed_origins(65530).len(), 24);
    }

    #[test]
    fn runs_headless_isolated_and_fenced_in() {
        let args = args(55010, Path::new("/tmp/out"));
        assert_eq!(&args[..4], ["-y", PACKAGE, "--headless", "--isolated"]);
        let origins: Vec<&str> = flag(&args, "--allowed-origins").split(';').collect();
        assert_eq!(origins, allowed_origins(55010));
        assert_eq!(flag(&args, "--proxy-server"), DEAD_PROXY);
        let bypass: Vec<&str> = flag(&args, "--proxy-bypass").split(',').collect();
        assert_eq!(bypass.len(), 20);
        assert!(bypass.contains(&"localhost:55010") && bypass.contains(&"127.0.0.1:55019"));
        assert!(!bypass.iter().any(|b| b.ends_with(":55020")));
        assert_eq!(flag(&args, "--output-dir"), "/tmp/out");
    }

    #[test]
    fn off_by_default() {
        assert!(!crate::settings::Settings::default().browser.enabled);
        let settings = crate::settings::parse("[scripts]\nsetup = \"x\"\n").unwrap();
        assert!(!settings.browser.enabled);
        let settings = crate::settings::parse("[browser]\nenabled = true\n").unwrap();
        assert!(settings.browser.enabled);
        // Without a browser, the MCP config and briefing are left alone.
        let config = Some(r#"{"mcpServers":{"bantu":{"type":"http"}}}"#.to_string());
        assert_eq!(with_browser(config.clone(), None), config);
        assert_eq!(with_note("ctx".into(), None), "ctx");
    }

    #[test]
    fn joins_the_claude_mcp_config() {
        let config = Some(r#"{"mcpServers":{"bantu":{"type":"http"}}}"#.to_string());
        let merged: Value =
            serde_json::from_str(&with_browser(config, Some(&server(55010))).unwrap()).unwrap();
        assert_eq!(merged["mcpServers"]["bantu"]["type"], "http");
        let browser = &merged["mcpServers"][SERVER_NAME];
        assert_eq!(browser["type"], "stdio");
        assert_eq!(browser["command"], "/usr/bin/npx");
        assert_eq!(browser["args"][1], PACKAGE);
        // Before the listener is bound there's no Bantu server, only the browser.
        let alone: Value =
            serde_json::from_str(&with_browser(None, Some(&server(55010))).unwrap()).unwrap();
        assert_eq!(
            alone["mcpServers"]
                .as_object()
                .unwrap()
                .keys()
                .collect::<Vec<_>>(),
            [SERVER_NAME]
        );
        let acp = server(55010).acp_entry();
        assert_eq!(acp["name"], SERVER_NAME);
        assert!(acp["env"].as_array().unwrap().is_empty());
    }

    #[test]
    fn tells_the_agent_where_its_app_runs() {
        let note = with_note("ctx".into(), Some(&server(55010)));
        assert!(note.starts_with("ctx\n- "));
        assert!(note.contains("http://localhost:55010"));
        assert!(note.contains("ports 55010-55019"));
    }

    #[test]
    fn finds_programs_on_path_or_by_path() {
        assert_eq!(find_program(Path::new("/definitely/not/npx")), None);
        assert_eq!(find_program(Path::new("bantu-no-such-program-xyz")), None);
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("npx");
        std::fs::write(&bin, "").unwrap();
        assert_eq!(find_program(&bin), Some(bin));
    }
}
