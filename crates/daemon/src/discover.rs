//! First-run help: find git repositories the user already has, so adding one
//! is a click instead of typing a path.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::ops::Daemon;

/// Common places people keep code, relative to $HOME.
const ROOTS: &[&str] = &[
    "code",
    "Code",
    "src",
    "dev",
    "Developer",
    "Projects",
    "projects",
    "repos",
    "git",
    "GitHub",
    "Documents/GitHub",
    "Documents/code",
    "workspace",
    "work",
    "conductor/repos",
];
const MAX_RESULTS: usize = 30;

#[derive(serde::Serialize)]
pub struct RepoSuggestion {
    pub path: String,
    pub name: String,
}

impl Daemon {
    pub async fn suggest_repos(&self) -> anyhow::Result<Vec<RepoSuggestion>> {
        let added: Vec<String> = self
            .store
            .repos()
            .await?
            .into_iter()
            .map(|r| r.root_path)
            .collect();
        let workspaces = self.config.workspaces_dir.clone();
        let home = crate::platform::home_dir();
        tokio::task::spawn_blocking(move || {
            let Some(home) = home else { return Ok(vec![]) };
            let mut found: Vec<(SystemTime, PathBuf)> = Vec::new();
            for root in ROOTS {
                scan(&home.join(root), 2, &mut found);
            }
            found.sort_by_key(|a| std::cmp::Reverse(a.0));
            let mut out: Vec<RepoSuggestion> = Vec::new();
            for (_, path) in found {
                let canon = path.canonicalize().unwrap_or(path);
                let s = canon.to_string_lossy().to_string();
                // Skip repos already added and Bantu's own worktrees.
                if added.contains(&s)
                    || canon.starts_with(&workspaces)
                    || out.iter().any(|o| o.path == s)
                {
                    continue;
                }
                let name = canon
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                out.push(RepoSuggestion { path: s, name });
                if out.len() >= MAX_RESULTS {
                    break;
                }
            }
            Ok(out)
        })
        .await?
    }
}

/// Collects directories with a `.git` *directory* (main checkouts, not
/// worktrees), newest activity first, up to `depth` levels below `dir`.
fn scan(dir: &Path, depth: u32, found: &mut Vec<(SystemTime, PathBuf)>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') || name == "node_modules" || name == "target" {
            continue;
        }
        if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        let git = path.join(".git");
        if git.is_dir() {
            // HEAD changes on checkout/commit: a decent "recently used" signal.
            let touched = std::fs::metadata(git.join("HEAD"))
                .and_then(|m| m.modified())
                .or_else(|_| entry.metadata().and_then(|m| m.modified()))
                .unwrap_or(SystemTime::UNIX_EPOCH);
            found.push((touched, path));
        } else if depth > 1 {
            scan(&path, depth - 1, found);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_main_checkouts_only() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("a/.git")).unwrap();
        std::fs::create_dir_all(dir.path().join("group/b/.git")).unwrap();
        std::fs::create_dir_all(dir.path().join("wt")).unwrap();
        std::fs::write(dir.path().join("wt/.git"), "gitdir: elsewhere").unwrap();
        std::fs::create_dir_all(dir.path().join("node_modules/x/.git")).unwrap();
        let mut found = Vec::new();
        scan(dir.path(), 2, &mut found);
        let mut names: Vec<String> = found
            .iter()
            .map(|(_, p)| p.file_name().unwrap().to_string_lossy().to_string())
            .collect();
        names.sort();
        assert_eq!(names, ["a", "b"]);
    }
}
