//! Which agent CLIs are installed on this machine and which accounts they're
//! signed in to. Bantu runs the user's own `claude` / `codex`, so turns use
//! that login: a Claude Pro, Max or Team subscription, or an API key.

use std::sync::Arc;
use std::time::Duration;

use bantu_protocol::{AgentKind, AgentStatus, LoginMethod, Terminal, TerminalKind};
use serde_json::Value;

use crate::ops::{CommandError, Daemon};

const PROBE_TIMEOUT: Duration = Duration::from_secs(15);

impl Daemon {
    pub async fn agent_statuses(&self) -> Vec<AgentStatus> {
        let (claude, codex, opencode, gemini, cursor) = tokio::join!(
            self.claude_status(),
            self.codex_status(),
            self.acp_status(AgentKind::Opencode),
            self.acp_status(AgentKind::Gemini),
            self.acp_status(AgentKind::Cursor),
        );
        vec![claude, codex, opencode, gemini, cursor]
    }

    /// ACP has no standard way to ask who's signed in without starting a
    /// session, so an installed agent counts as ready until a turn says otherwise.
    async fn acp_status(&self, agent: AgentKind) -> AgentStatus {
        let mut status = empty(agent);
        let Some((bin, _)) = crate::acp::command(&self.config, agent) else {
            return status;
        };
        let Some(version) = run(&bin, &["--version"]).await.filter(|(ok, _)| *ok) else {
            return status;
        };
        status.installed = true;
        status.logged_in = true;
        status.version = version.1.split_whitespace().last().map(str::to_string);
        status
    }

    async fn claude_status(&self) -> AgentStatus {
        let bin = &self.config.claude_bin;
        let mut status = empty(AgentKind::Claude);
        let Some(version) = run(bin, &["--version"]).await.filter(|(ok, _)| *ok) else {
            return status;
        };
        status.installed = true;
        status.version = version.1.split_whitespace().next().map(str::to_string);
        if let Some((_, out)) = run(bin, &["auth", "status", "--json"]).await {
            if let Ok(v) = serde_json::from_str::<Value>(&out) {
                apply_claude_auth(&mut status, &v);
            }
        }
        status
    }

    async fn codex_status(&self) -> AgentStatus {
        let bin = &self.config.codex_bin;
        let mut status = empty(AgentKind::Codex);
        let Some(version) = run(bin, &["--version"]).await.filter(|(ok, _)| *ok) else {
            return status;
        };
        status.installed = true;
        status.version = version.1.split_whitespace().last().map(str::to_string);
        if let Some((ok, out)) = run(bin, &["login", "status"]).await {
            apply_codex_login(&mut status, ok, &out);
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
        let (bin, args, title): (&std::path::Path, &str, &str) = match (agent, method) {
            (AgentKind::Claude, LoginMethod::Subscription) => (
                &self.config.claude_bin,
                "auth login --claudeai",
                "Sign in to Claude",
            ),
            (AgentKind::Claude, LoginMethod::Sso) => (
                &self.config.claude_bin,
                "auth login --sso",
                "Sign in to Claude (SSO)",
            ),
            (AgentKind::Claude, LoginMethod::Console) => (
                &self.config.claude_bin,
                "auth login --console",
                "Sign in to Anthropic Console",
            ),
            (AgentKind::Codex, LoginMethod::Console) => (
                &self.config.codex_bin,
                "login --with-api-key",
                "Codex API key",
            ),
            (AgentKind::Codex, _) => (&self.config.codex_bin, "login", "Sign in to Codex"),
            (AgentKind::Opencode, _) => (
                &self.config.opencode_bin,
                "auth login",
                "Sign in to OpenCode",
            ),
            // Gemini CLI signs in from its own first-run screen.
            (AgentKind::Gemini, _) => (&self.config.gemini_bin, "", "Sign in to Gemini"),
            (AgentKind::Cursor, _) => (&self.config.cursor_bin, "login", "Sign in to Cursor"),
        };
        let command = format!("{} {args}", shell_quote(&bin.to_string_lossy()))
            .trim_end()
            .to_string();
        self.spawn_terminal(
            None,
            TerminalKind::Login,
            title.into(),
            Some(command),
            None,
            None,
        )
        .await
    }
}

fn empty(agent: AgentKind) -> AgentStatus {
    AgentStatus {
        agent,
        installed: false,
        version: None,
        logged_in: false,
        auth_method: None,
        account: None,
        organization: None,
        plan: None,
    }
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

/// Runs a probe with a timeout; `(success, stdout)`, or `None` if it can't start.
async fn run(bin: &std::path::Path, args: &[&str]) -> Option<(bool, String)> {
    let out = tokio::process::Command::new(bin)
        .args(args)
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true)
        .output();
    let out = tokio::time::timeout(PROBE_TIMEOUT, out).await.ok()?.ok()?;
    let mut text = String::from_utf8_lossy(&out.stdout).to_string();
    if text.trim().is_empty() {
        // Some CLIs print status to stderr.
        text = String::from_utf8_lossy(&out.stderr).to_string();
    }
    Some((out.status.success(), text))
}

fn shell_quote(s: &str) -> String {
    if s.chars()
        .all(|c| c.is_ascii_alphanumeric() || "/._-".contains(c))
    {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', r"'\''"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reads_claude_and_codex_status() {
        let mut s = empty(AgentKind::Claude);
        apply_claude_auth(
            &mut s,
            &json!({"loggedIn": true, "authMethod": "claude.ai", "email": "a@b.c",
                    "orgName": "Team X", "subscriptionType": "team"}),
        );
        assert!(s.logged_in);
        assert_eq!(s.plan.as_deref(), Some("team"));
        assert_eq!(s.organization.as_deref(), Some("Team X"));

        let mut c = empty(AgentKind::Codex);
        apply_codex_login(&mut c, true, "Logged in using an API key - sk-proj-***abcd");
        assert_eq!(c.auth_method.as_deref(), Some("api_key"));
        assert_eq!(c.account, None, "never echo the key");
        let mut c = empty(AgentKind::Codex);
        apply_codex_login(&mut c, false, "Not logged in");
        assert!(!c.logged_in);
        assert_eq!(
            shell_quote("/Users/me/My Apps/claude"),
            "'/Users/me/My Apps/claude'"
        );
    }
}
