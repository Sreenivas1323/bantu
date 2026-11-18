//! Which agent CLIs are installed on this machine and which accounts they're
//! signed in to, and which agent a repo wants for each role. Bantu runs the
//! user's own CLIs, so turns use their login: a Claude Pro, Max or Team
//! subscription, an API key, and so on.
//!
//! What each agent is and how to probe it comes from the
//! [catalogue](crate::catalogue).

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use bantu_protocol::{
    AgentKind, AgentRole, AgentStatus, LoginMethod, Terminal, TerminalKind, Workspace,
    WorkspaceState,
};
use serde_json::Value;

use crate::catalogue::{self, AgentSpec, Login, SignIn};
use crate::ops::{CommandError, Daemon};
use crate::platform::{self, shell_quote};
use crate::terminals::TerminalSpec;

/// `--version` only labels the agent, so a CLI that hangs on it (Droid does on
/// some machines) mustn't hold up the whole list.
const VERSION_TIMEOUT: Duration = Duration::from_secs(3);
const PROBE_TIMEOUT: Duration = Duration::from_secs(5);

impl Daemon {
    /// Every agent in the catalogue, probed in parallel.
    pub async fn agent_statuses(&self) -> Vec<AgentStatus> {
        futures::future::join_all(catalogue::AGENTS.iter().map(|spec| self.agent_status(spec)))
            .await
    }

    async fn agent_status(&self, spec: &AgentSpec) -> AgentStatus {
        let bin = self.config.agent_bin(spec.kind);
        let mut status = empty(spec);
        if !on_path(&bin) {
            return status;
        }
        status.installed = true;
        if let Some((true, out)) = run_for(&bin, &["--version"], VERSION_TIMEOUT).await {
            status.version = parse_version(spec.kind, &out);
        }
        match spec.sign_in {
            SignIn::Claude => {
                if let Some((_, out)) = run(&bin, &["auth", "status", "--json"]).await {
                    if let Ok(v) = serde_json::from_str::<Value>(&out) {
                        apply_claude_auth(&mut status, &v);
                    }
                }
            }
            SignIn::Codex => {
                if let Some((ok, out)) = run(&bin, &["login", "status"]).await {
                    apply_codex_login(&mut status, ok, &out);
                }
            }
            // ACP has no standard way to ask who's signed in without starting
            // a session, so an installed agent counts as ready until a turn
            // says otherwise.
            SignIn::Assume => status.logged_in = true,
            SignIn::Files { files, env } => {
                status.logged_in = signed_in_by_files(platform::home_dir().as_deref(), files, env);
            }
            SignIn::Command { args, signed_out } => {
                status.logged_in = matches!(
                    run(&bin, args).await,
                    Some((true, out)) if !out.replace(' ', "").contains(signed_out)
                );
            }
        }
        status
    }

    /// A PTY running the agent's own sign-in flow (it opens the browser or
    /// asks for a code). Credentials go straight to the CLI, never to Bantu.
    pub async fn agent_login(
        self: &Arc<Self>,
        agent: AgentKind,
        method: Option<LoginMethod>,
    ) -> Result<Terminal, CommandError> {
        let method = method.unwrap_or(LoginMethod::Subscription);
        let spec = catalogue::spec(agent);
        let (args, title) = match (agent, method) {
            (AgentKind::Claude, LoginMethod::Subscription) => {
                ("auth login --claudeai", "Sign in to Claude".to_string())
            }
            (AgentKind::Claude, LoginMethod::Sso) => {
                ("auth login --sso", "Sign in to Claude (SSO)".to_string())
            }
            (AgentKind::Claude, LoginMethod::Console) => (
                "auth login --console",
                "Sign in to Anthropic Console".to_string(),
            ),
            (AgentKind::Codex, LoginMethod::Console) => {
                ("login --with-api-key", "Codex API key".to_string())
            }
            _ => match spec.login {
                Login::Args(args) => (args, format!("Sign in to {}", spec.name)),
                Login::Tui => ("", format!("Sign in to {}", spec.name)),
                Login::None => {
                    return Err(CommandError::BadRequest(format!(
                        "{} has no sign-in of its own; it reads provider API keys from the \
                         environment",
                        spec.name
                    )))
                }
            },
        };
        let bin = self.config.agent_bin(agent);
        let command = format!(
            "{} {args}",
            platform::call(&shell_quote(&bin.to_string_lossy()))
        )
        .trim_end()
        .to_string();
        self.spawn_terminal(
            None,
            TerminalSpec {
                command: Some(command),
                ..TerminalSpec::new(TerminalKind::Login, title)
            },
        )
        .await
    }

    /// An agent's own terminal UI in a workspace terminal. This is how
    /// terminal-only agents run, and it works for every other agent too
    /// (as a separate conversation from any chat).
    pub async fn open_agent_terminal(
        self: &Arc<Self>,
        workspace_id: &str,
        agent: AgentKind,
        cols: Option<u16>,
        rows: Option<u16>,
    ) -> Result<Terminal, CommandError> {
        let ws = self.ready_workspace(workspace_id).await?;
        let spec = catalogue::spec(agent);
        let mut parts = vec![platform::call(&shell_quote(
            &self.config.agent_bin(agent).to_string_lossy(),
        ))];
        parts.extend(spec.tui_args.iter().map(|a| a.to_string()));
        self.spawn_terminal(
            Some(&ws),
            TerminalSpec {
                command: Some(parts.join(" ")),
                cols,
                rows,
                ..TerminalSpec::new(TerminalKind::Agent, spec.name.to_string())
            },
        )
        .await
    }

    /// The agent a new chat gets when none was asked for: the repo's
    /// `[agents]` choice for `role` (read from the main checkout, like every
    /// setting the daemon acts on by itself), else Claude.
    pub(crate) async fn role_agent(&self, ws: &Workspace, role: AgentRole) -> AgentKind {
        match self.trusted_settings(ws).await {
            Ok(settings) => settings.agents.for_role(role, &settings.review),
            Err(e) => {
                tracing::warn!("couldn't read settings for {}: {e:#}", ws.name);
                None
            }
        }
        .unwrap_or(AgentKind::Claude)
    }

    /// The role a chat started without one plays: a workspace that has
    /// started others is a lead, so its new chats get the orchestrator.
    pub(crate) async fn default_role(&self, ws: &Workspace) -> AgentRole {
        let leads = self.store.workspaces().await.is_ok_and(|all| {
            all.iter().any(|w| {
                w.parent_id.as_deref() == Some(ws.id.as_str())
                    && w.state != WorkspaceState::Archived
            })
        });
        if leads {
            AgentRole::Orchestrator
        } else {
            AgentRole::Worker
        }
    }
}

fn empty(spec: &AgentSpec) -> AgentStatus {
    AgentStatus {
        agent: spec.kind,
        name: spec.name.to_string(),
        mode: spec.mode,
        install_hint: spec.install.to_string(),
        docs_url: spec.docs.to_string(),
        can_sign_in: spec.login != Login::None,
        installed: false,
        version: None,
        logged_in: false,
        auth_method: None,
        account: None,
        organization: None,
        plan: None,
    }
}

/// `claude --version` prints `2.1.0 (Claude Code)`; most others end with it.
fn parse_version(agent: AgentKind, out: &str) -> Option<String> {
    let mut words = out.split_whitespace();
    if agent == AgentKind::Claude {
        words.next()
    } else {
        words.last()
    }
    .map(str::to_string)
}

/// Whether any credential file (relative to `home`) has content, or any of
/// the environment variables is set.
fn signed_in_by_files(home: Option<&Path>, files: &[&str], env: &[&str]) -> bool {
    let has_file = home.is_some_and(|home| {
        files
            .iter()
            .any(|f| std::fs::metadata(home.join(f)).is_ok_and(|m| m.is_file() && m.len() > 0))
    });
    has_file
        || env
            .iter()
            .any(|v| std::env::var_os(v).is_some_and(|v| !v.is_empty()))
}

/// Fields from `claude auth status --json`.
pub fn apply_claude_auth(status: &mut AgentStatus, v: &Value) {
    let s = |k: &str| v[k].as_str().filter(|x| !x.is_empty()).map(str::to_string);
    status.logged_in = v["loggedIn"].as_bool().unwrap_or(false);
    status.auth_method = s("authMethod");
    status.account = s("email");
    status.organization = s("orgName");
    status.plan = s("subscriptionType");
}

/// `codex login status` prints e.g. "Logged in using ChatGPT" and exits 0.
/// The API-key variant includes part of the key, which we never pass on.
pub fn apply_codex_login(status: &mut AgentStatus, ok: bool, out: &str) {
    status.logged_in = ok && out.contains("Logged in");
    if status.logged_in {
        status.auth_method = Some(
            if out.contains("ChatGPT") {
                "chatgpt"
            } else {
                "api_key"
            }
            .into(),
        );
    }
}

/// Whether the agent's program exists: a path that's a file, or a name found
/// on `PATH`.
fn on_path(bin: &std::path::Path) -> bool {
    let program = platform::program(bin);
    if program.components().count() > 1 {
        return program.is_file();
    }
    std::env::var_os("PATH")
        .is_some_and(|paths| std::env::split_paths(&paths).any(|dir| dir.join(&program).is_file()))
}

/// Runs a probe with a timeout; `(success, stdout)`, or `None` if it can't start.
async fn run(bin: &std::path::Path, args: &[&str]) -> Option<(bool, String)> {
    run_for(bin, args, PROBE_TIMEOUT).await
}

async fn run_for(bin: &std::path::Path, args: &[&str], limit: Duration) -> Option<(bool, String)> {
    let out = tokio::process::Command::new(crate::platform::program(bin))
        .args(args)
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true)
        .output();
    let out = tokio::time::timeout(limit, out).await.ok()?.ok()?;
    let mut text = String::from_utf8_lossy(&out.stdout).to_string();
    if text.trim().is_empty() {
        // Some CLIs print status to stderr.
        text = String::from_utf8_lossy(&out.stderr).to_string();
    }
    Some((out.status.success(), text))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn finds_programs_by_path_or_name() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("agent-cli");
        std::fs::write(&file, "").unwrap();
        assert!(on_path(&file));
        assert!(!on_path(&dir.path().join("missing")));
        assert!(on_path(std::path::Path::new("sh")), "found on PATH");
        assert!(!on_path(std::path::Path::new("surely-not-a-real-cli-name")));
    }

    #[test]
    fn reads_claude_and_codex_status() {
        let mut s = empty(catalogue::spec(AgentKind::Claude));
        apply_claude_auth(
            &mut s,
            &json!({"loggedIn": true, "authMethod": "claude.ai", "email": "a@b.c",
                    "orgName": "Team X", "subscriptionType": "team"}),
        );
        assert!(s.logged_in);
        assert_eq!(s.plan.as_deref(), Some("team"));
        assert_eq!(s.organization.as_deref(), Some("Team X"));

        let mut c = empty(catalogue::spec(AgentKind::Codex));
        apply_codex_login(&mut c, true, "Logged in using an API key - sk-proj-***abcd");
        assert_eq!(c.auth_method.as_deref(), Some("api_key"));
        assert_eq!(c.account, None, "never echo the key");
        let mut c = empty(catalogue::spec(AgentKind::Codex));
        apply_codex_login(&mut c, false, "Not logged in");
        assert!(!c.logged_in);
        #[cfg(unix)]
        assert_eq!(
            shell_quote("/Users/me/My Apps/claude"),
            "'/Users/me/My Apps/claude'"
        );
    }

    #[test]
    fn statuses_carry_catalogue_data() {
        let s = empty(catalogue::spec(AgentKind::Copilot));
        assert_eq!(s.name, "GitHub Copilot");
        assert_eq!(s.mode, bantu_protocol::AgentMode::Acp);
        assert!(s.install_hint.contains("@github/copilot"));
        assert_eq!(
            parse_version(AgentKind::Claude, "2.1.0 (Claude Code)").as_deref(),
            Some("2.1.0")
        );
        assert_eq!(
            parse_version(AgentKind::Kiro, "kiro-cli 1.25.0\n").as_deref(),
            Some("1.25.0")
        );
    }

    #[test]
    fn credential_files_and_env() {
        let home = tempfile::tempdir().unwrap();
        let files = &[".augment/session.json"];
        assert!(!signed_in_by_files(Some(home.path()), files, &[]));
        std::fs::create_dir_all(home.path().join(".augment")).unwrap();
        std::fs::write(home.path().join(".augment/session.json"), "").unwrap();
        assert!(
            !signed_in_by_files(Some(home.path()), files, &[]),
            "an empty file isn't a login"
        );
        std::fs::write(home.path().join(".augment/session.json"), "{}").unwrap();
        assert!(signed_in_by_files(Some(home.path()), files, &[]));
        assert!(signed_in_by_files(None, &[], &["PATH"]));
        assert!(!signed_in_by_files(
            None,
            &[],
            &["BANTU_TEST_SURELY_UNSET_VARIABLE"]
        ));
    }
}
