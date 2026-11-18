//! Every coding agent Bantu knows how to start, as one table.
//!
//! An agent is driven one of three ways ([`AgentMode`]):
//! - **native**: Claude Code and Codex, through their own JSON output
//!   (`sessions.rs`, `codex.rs`);
//! - **ACP**: structured chat over the Agent Client Protocol (`acp.rs`);
//! - **terminal**: agents with no structured interface we could confirm run
//!   in a workspace terminal instead (`terminal.open` with `agent`).
//!
//! An agent is listed as ACP only when its own current documentation shows an
//! ACP mode; the entry's `docs` link points there. Binaries, install commands
//! and sign-in checks were cross-checked against the agent adapters of Agent
//! Orchestrator (<https://github.com/OrchestratorInc/agent-orchestrator>,
//! Apache-2.0, `backend/internal/adapters/agent/`). Only facts were taken from
//! it, no code.
//!
//! Each binary can be overridden with `BANTU_<ID>_BIN`, e.g.
//! `BANTU_KIRO_BIN=/opt/kiro/bin/kiro-cli`.

use std::path::{Path, PathBuf};

use bantu_protocol::{AgentKind, AgentMode};

/// How Bantu learns whether an agent is signed in, without starting a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignIn {
    /// Claude Code's `auth status --json`.
    Claude,
    /// Codex's `login status`.
    Codex,
    /// Nothing reliable to check, so an installed agent counts as ready
    /// until a turn says otherwise.
    Assume,
    /// Signed in if any of these files (relative to the home directory)
    /// exists and isn't empty, or any of these environment variables is set.
    Files {
        files: &'static [&'static str],
        env: &'static [&'static str],
    },
    /// Signed in if this command succeeds and doesn't print `signed_out`.
    Command {
        args: &'static [&'static str],
        signed_out: &'static str,
    },
}

/// How the agent's own sign-in flow starts, run in a terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Login {
    /// A sign-in subcommand, e.g. `kiro-cli login`.
    Args(&'static str),
    /// The agent signs in from its own terminal UI (often a `/login` command).
    Tui,
    /// No sign-in of its own: it reads provider API keys from the environment.
    None,
}

#[derive(Debug, Clone, Copy)]
pub struct AgentSpec {
    pub kind: AgentKind,
    pub name: &'static str,
    /// The CLI's default program name, found on `PATH`.
    pub bin: &'static str,
    pub mode: AgentMode,
    /// Arguments that start ACP mode over stdio.
    pub acp_args: &'static [&'static str],
    /// A separate program for ACP mode, next to `bin` (Mistral ships
    /// `vibe-acp` beside `vibe`).
    pub acp_program: Option<&'static str>,
    /// Arguments that open its interactive terminal UI.
    pub tui_args: &'static [&'static str],
    pub install: &'static str,
    pub docs: &'static str,
    pub sign_in: SignIn,
    pub login: Login,
}

impl AgentSpec {
    /// `BANTU_KIRO_BIN` and friends.
    pub fn env_var(&self) -> String {
        format!("BANTU_{}_BIN", self.kind.as_str().to_ascii_uppercase())
    }

    pub fn chat(&self) -> bool {
        self.mode != AgentMode::Terminal
    }
}

/// One table row; positional so the table stays one screen per agent.
#[allow(clippy::too_many_arguments)]
const fn acp(
    kind: AgentKind,
    name: &'static str,
    bin: &'static str,
    acp_args: &'static [&'static str],
    install: &'static str,
    docs: &'static str,
    sign_in: SignIn,
    login: Login,
) -> AgentSpec {
    AgentSpec {
        kind,
        name,
        bin,
        mode: AgentMode::Acp,
        acp_args,
        acp_program: None,
        tui_args: &[],
        install,
        docs,
        sign_in,
        login,
    }
}

const fn terminal(
    kind: AgentKind,
    name: &'static str,
    bin: &'static str,
    install: &'static str,
    docs: &'static str,
    login: Login,
) -> AgentSpec {
    AgentSpec {
        kind,
        name,
        bin,
        mode: AgentMode::Terminal,
        acp_args: &[],
        acp_program: None,
        tui_args: &[],
        install,
        docs,
        sign_in: SignIn::Assume,
        login,
    }
}

/// The catalogue, in [`AgentKind::ALL`] order.
pub const AGENTS: [AgentSpec; 18] = [
    AgentSpec {
        kind: AgentKind::Claude,
        name: "Claude",
        bin: "claude",
        mode: AgentMode::Native,
        acp_args: &[],
        acp_program: None,
        tui_args: &[],
        install: "curl -fsSL https://claude.ai/install.sh | bash",
        docs: "https://code.claude.com/docs/en/cli-reference",
        sign_in: SignIn::Claude,
        login: Login::Args("auth login --claudeai"),
    },
    AgentSpec {
        kind: AgentKind::Codex,
        name: "Codex",
        bin: "codex",
        mode: AgentMode::Native,
        acp_args: &[],
        acp_program: None,
        tui_args: &[],
        install: "npm install -g @openai/codex",
        docs: "https://github.com/openai/codex",
        sign_in: SignIn::Codex,
        login: Login::Args("login"),
    },
    acp(
        AgentKind::Opencode,
        "OpenCode",
        "opencode",
        &["acp"],
        "curl -fsSL https://opencode.ai/install | bash",
        "https://opencode.ai/docs/acp/",
        SignIn::Assume,
        Login::Args("auth login"),
    ),
    acp(
        AgentKind::Gemini,
        "Gemini",
        "gemini",
        &["--acp"],
        "npm install -g @google/gemini-cli",
        "https://github.com/google-gemini/gemini-cli",
        SignIn::Assume,
        // Gemini CLI signs in from its own first-run screen.
        Login::Tui,
    ),
    acp(
        AgentKind::Cursor,
        "Cursor",
        "agent",
        &["acp"],
        "curl https://cursor.com/install -fsS | bash",
        "https://cursor.com/docs/cli/acp",
        SignIn::Assume,
        Login::Args("login"),
    ),
    AgentSpec {
        tui_args: &["session"],
        ..acp(
            AgentKind::Goose,
            "Goose",
            "goose",
            &["acp"],
            "curl -fsSL https://github.com/aaif-goose/goose/releases/download/stable/download_cli.sh | bash",
            "https://github.com/block/goose/blob/main/documentation/docs/guides/goose-cli-commands.md",
            SignIn::Files {
                files: &[".config/goose/config.yaml"],
                env: &["GOOSE_PROVIDER"],
            },
            Login::Args("configure"),
        )
    },
    acp(
        AgentKind::Qwen,
        "Qwen Code",
        "qwen",
        &["--acp"],
        "npm install -g @qwen-code/qwen-code@latest",
        "https://qwenlm.github.io/qwen-code-docs/en/users/integration-zed",
        SignIn::Assume,
        // `/auth` inside the TUI.
        Login::Tui,
    ),
    acp(
        AgentKind::Copilot,
        "GitHub Copilot",
        "copilot",
        &["--acp"],
        "npm install -g @github/copilot",
        "https://github.blog/changelog/2026-01-28-acp-support-in-copilot-cli-is-now-in-public-preview/",
        SignIn::Assume,
        // `/login` inside the TUI.
        Login::Tui,
    ),
    acp(
        AgentKind::Auggie,
        "Auggie",
        "auggie",
        &["--acp"],
        "npm install -g @augmentcode/auggie",
        "https://docs.augmentcode.com/cli/acp",
        SignIn::Files {
            files: &[".augment/session.json"],
            env: &["AUGMENT_SESSION_AUTH"],
        },
        Login::Args("auth login"),
    ),
    acp(
        AgentKind::Kimi,
        "Kimi",
        "kimi",
        &["acp"],
        "curl -fsSL https://code.kimi.com/kimi-code/install.sh | bash",
        "https://www.kimi.com/code/docs/en/kimi-code-cli/reference/kimi-acp.html",
        SignIn::Assume,
        // `/login` inside the TUI.
        Login::Tui,
    ),
    AgentSpec {
        tui_args: &["chat"],
        ..acp(
            AgentKind::Kiro,
            "Kiro",
            "kiro-cli",
            &["acp"],
            "curl -fsSL https://cli.kiro.dev/install | bash",
            "https://kiro.dev/docs/cli/acp/",
            SignIn::Command {
                args: &["whoami", "--format", "json"],
                signed_out: "\"account\":null",
            },
            Login::Args("login"),
        )
    },
    acp(
        AgentKind::Cline,
        "Cline",
        "cline",
        &["--acp"],
        "npm install -g cline",
        "https://docs.cline.bot/usage/acp",
        SignIn::Files {
            files: &[".cline/data/settings/providers.json"],
            env: &["CLINE_API_KEY"],
        },
        Login::Args("auth"),
    ),
    acp(
        AgentKind::Kilo,
        "Kilo Code",
        "kilo",
        &["acp"],
        "npm install -g @kilocode/cli",
        "https://kilo.ai/docs/code-with-ai/platforms/cli-reference",
        SignIn::Assume,
        Login::Tui,
    ),
    AgentSpec {
        acp_program: Some("vibe-acp"),
        ..acp(
            AgentKind::Vibe,
            "Mistral Vibe",
            "vibe",
            &[],
            "curl -LsSf https://mistral.ai/vibe/install.sh | bash",
            "https://docs.mistral.ai/vibe/code/use-vibe-in-other-ides",
            SignIn::Assume,
            Login::Tui,
        )
    },
    terminal(
        AgentKind::Aider,
        "Aider",
        "aider",
        "curl -LsSf https://aider.chat/install.sh | sh",
        "https://aider.chat/docs/install.html",
        Login::None,
    ),
    terminal(
        AgentKind::Amp,
        "Amp",
        "amp",
        "curl -fsSL https://ampcode.com/install.sh | bash",
        "https://ampcode.com/manual",
        Login::Tui,
    ),
    terminal(
        AgentKind::Crush,
        "Crush",
        "crush",
        "npm install -g @charmland/crush",
        "https://github.com/charmbracelet/crush",
        Login::Tui,
    ),
    terminal(
        AgentKind::Droid,
        "Droid",
        "droid",
        "curl -fsSL https://app.factory.ai/cli | sh",
        "https://docs.factory.com/reference/cli-reference",
        Login::Tui,
    ),
];

pub fn spec(kind: AgentKind) -> &'static AgentSpec {
    AGENTS
        .iter()
        .find(|s| s.kind == kind)
        .expect("every AgentKind has a catalogue entry")
}

/// The ids of agents that can hold a structured chat, for error messages and
/// tool schemas.
pub fn chat_ids() -> Vec<&'static str> {
    AGENTS
        .iter()
        .filter(|s| s.chat())
        .map(|s| s.kind.as_str())
        .collect()
}

/// A program with the given file name in the same directory as `bin`, or
/// just the name (found on `PATH`) when `bin` is a bare name.
pub fn sibling(bin: &Path, name: &str) -> PathBuf {
    match bin.parent().filter(|p| !p.as_os_str().is_empty()) {
        Some(dir) => dir.join(name),
        None => PathBuf::from(name),
    }
}

/// Parses `id=path`, as given to `--agent-bin`.
pub fn parse_override(s: &str) -> Result<(AgentKind, PathBuf), String> {
    let (id, path) = s
        .split_once('=')
        .ok_or_else(|| format!("expected <agent>=<path>, got {s:?}"))?;
    let kind = AgentKind::parse(id.trim()).ok_or_else(|| format!("unknown agent {id:?}"))?;
    if path.trim().is_empty() {
        return Err(format!("no path for {id}"));
    }
    Ok((kind, PathBuf::from(path.trim())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_entry_per_agent_in_order() {
        assert_eq!(AGENTS.len(), AgentKind::ALL.len());
        for (spec, kind) in AGENTS.iter().zip(AgentKind::ALL) {
            assert_eq!(spec.kind, kind);
            assert!(!spec.name.is_empty() && !spec.bin.is_empty());
            assert!(!spec.install.is_empty());
            assert!(spec.docs.starts_with("https://"), "{}", spec.docs);
            // An ACP agent needs a way into ACP mode; the others mustn't have one.
            let has_acp = !spec.acp_args.is_empty() || spec.acp_program.is_some();
            assert_eq!(has_acp, spec.mode == AgentMode::Acp, "{}", spec.name);
        }
    }

    #[test]
    fn env_overrides_follow_the_id() {
        assert_eq!(spec(AgentKind::Opencode).env_var(), "BANTU_OPENCODE_BIN");
        assert_eq!(spec(AgentKind::Kiro).env_var(), "BANTU_KIRO_BIN");
        assert_eq!(spec(AgentKind::Kiro).bin, "kiro-cli");
    }

    #[test]
    fn modes() {
        let ids = |mode| {
            AGENTS
                .iter()
                .filter(|s| s.mode == mode)
                .map(|s| s.kind.as_str())
                .collect::<Vec<_>>()
        };
        assert_eq!(ids(AgentMode::Native), ["claude", "codex"]);
        assert_eq!(
            ids(AgentMode::Acp),
            [
                "opencode", "gemini", "cursor", "goose", "qwen", "copilot", "auggie", "kimi",
                "kiro", "cline", "kilo", "vibe"
            ]
        );
        assert_eq!(ids(AgentMode::Terminal), ["aider", "amp", "crush", "droid"]);
        assert!(!chat_ids().contains(&"aider"));
        assert!(chat_ids().contains(&"goose"));
    }

    #[test]
    fn sibling_programs_and_overrides() {
        assert_eq!(
            sibling(Path::new("vibe"), "vibe-acp"),
            PathBuf::from("vibe-acp")
        );
        assert_eq!(
            sibling(Path::new("/opt/vibe/bin/vibe"), "vibe-acp"),
            PathBuf::from("/opt/vibe/bin/vibe-acp")
        );
        assert_eq!(
            parse_override("goose=/usr/local/bin/goose").unwrap(),
            (AgentKind::Goose, PathBuf::from("/usr/local/bin/goose"))
        );
        assert!(parse_override("nope=/x").is_err());
        assert!(parse_override("goose").is_err());
        assert!(parse_override("goose=").is_err());
    }
}
