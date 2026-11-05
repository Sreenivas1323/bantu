//! Per-repo settings from `.bantu/settings.toml`.
//!
//! ```toml
//! [scripts]
//! setup = "bun install"
//! archive = "docker compose down"
//! check = "bun run typecheck && bun test"
//! ```
//!
//! `.conductor/settings.toml` is read as a fallback so Conductor users can
//! switch without rewriting their config. Unknown keys are ignored.

use std::path::Path;

use serde::Deserialize;

#[derive(Debug, Default, Clone, Deserialize)]
pub struct Settings {
    #[serde(default)]
    pub scripts: Scripts,
    #[serde(default)]
    pub prompts: Prompts,
}

/// Repo-specific instructions added to Bantu's built-in prompts:
///
/// ```toml
/// [prompts]
/// review = "We care most about migrations. Ignore generated files."
/// create_pr = "Use our template in .github/pull_request_template.md."
/// fix_checks = "CI runs on Node 22; run `bun test` locally first."
/// address_comments = "Reply on the PR when you disagree."
/// resolve_conflicts = "Prefer rebasing over merge commits."
/// rename_branch = "Branches look like team/short-description."
/// ```
#[derive(Debug, Default, Clone, Deserialize)]
pub struct Prompts {
    pub review: Option<String>,
    pub create_pr: Option<String>,
    pub fix_checks: Option<String>,
    pub address_comments: Option<String>,
    pub resolve_conflicts: Option<String>,
    pub rename_branch: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptKind {
    Review,
    CreatePr,
    FixChecks,
    AddressComments,
    ResolveConflicts,
    RenameBranch,
}

impl Prompts {
    pub fn get(&self, kind: PromptKind) -> Option<&str> {
        match kind {
            PromptKind::Review => self.review.as_deref(),
            PromptKind::CreatePr => self.create_pr.as_deref(),
            PromptKind::FixChecks => self.fix_checks.as_deref(),
            PromptKind::AddressComments => self.address_comments.as_deref(),
            PromptKind::ResolveConflicts => self.resolve_conflicts.as_deref(),
            PromptKind::RenameBranch => self.rename_branch.as_deref(),
        }
        .map(str::trim)
        .filter(|p| !p.is_empty())
    }

    /// `prompt` followed by this repo's instructions for it, if any.
    pub fn apply(&self, kind: PromptKind, prompt: String) -> String {
        match self.get(kind) {
            Some(extra) => format!("{prompt}\n\nInstructions for this repository:\n{extra}"),
            None => prompt,
        }
    }
}

#[derive(Debug, Default, Clone, Deserialize)]
pub struct Scripts {
    pub setup: Option<String>,
    pub archive: Option<String>,
    pub run: Option<RunScript>,
    /// "Definition of done": run after every agent turn that changed files;
    /// failures go back to the agent. E.g. `bun run typecheck && bun test`.
    pub check: Option<String>,
}

/// `run = "bun dev"`, or Conductor-style named actions:
/// `[scripts.run.dev] command = "bun dev"` (the first one is used for now).
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum RunScript {
    Command(String),
    Named(std::collections::BTreeMap<String, NamedRun>),
}

#[derive(Debug, Clone, Deserialize)]
pub struct NamedRun {
    pub command: String,
}

impl Scripts {
    pub fn run_command(&self) -> Option<String> {
        match self.run.as_ref()? {
            RunScript::Command(c) => Some(c.clone()),
            RunScript::Named(m) => m.values().next().map(|r| r.command.clone()),
        }
    }
}

const CANDIDATES: [&str; 2] = [".bantu/settings.toml", ".conductor/settings.toml"];

/// Loads settings from the first directory in `dirs` that has a settings file.
/// The worktree comes first (the branch's committed config), then the repo root
/// (local, possibly untracked config).
pub fn load(dirs: &[&Path]) -> Result<Settings, String> {
    for dir in dirs {
        for rel in CANDIDATES {
            let path = dir.join(rel);
            if let Ok(text) = std::fs::read_to_string(&path) {
                return toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()));
            }
        }
    }
    Ok(Settings::default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adds_repo_prompts() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".bantu")).unwrap();
        std::fs::write(
            dir.path().join(".bantu/settings.toml"),
            "[prompts]\nreview = \"Check the migrations.\"\ncreate_pr = \"  \"\n",
        )
        .unwrap();
        let prompts = load(&[dir.path()]).unwrap().prompts;
        assert_eq!(
            prompts.apply(PromptKind::Review, "Review this.".into()),
            "Review this.\n\nInstructions for this repository:\nCheck the migrations."
        );
        // Blank and missing prompts leave the built-in one alone.
        assert_eq!(prompts.apply(PromptKind::CreatePr, "PR.".into()), "PR.");
        assert_eq!(prompts.apply(PromptKind::FixChecks, "Fix.".into()), "Fix.");
    }

    #[test]
    fn reads_bantu_then_conductor_and_ignores_unknown_keys() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load(&[dir.path()]).unwrap().scripts.setup.is_none());

        std::fs::create_dir_all(dir.path().join(".conductor")).unwrap();
        std::fs::write(
            dir.path().join(".conductor/settings.toml"),
            "[scripts]\nsetup = \"make\"\n[scripts.run.dev]\ncommand = \"x\"\n",
        )
        .unwrap();
        assert_eq!(
            load(&[dir.path()]).unwrap().scripts.setup.as_deref(),
            Some("make")
        );

        std::fs::create_dir_all(dir.path().join(".bantu")).unwrap();
        std::fs::write(
            dir.path().join(".bantu/settings.toml"),
            "[scripts]\nsetup = \"bun i\"\n",
        )
        .unwrap();
        assert_eq!(
            load(&[dir.path()]).unwrap().scripts.setup.as_deref(),
            Some("bun i")
        );
    }
}
