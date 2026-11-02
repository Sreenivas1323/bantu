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
