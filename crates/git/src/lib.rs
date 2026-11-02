//! Thin async wrapper around the `git` CLI.
//!
//! Bantu shells out to the user's `git` instead of linking libgit2 so that
//! hooks, credential helpers, config and LFS behave exactly as in their terminal.

use std::path::{Path, PathBuf};
use std::process::Stdio;

use tokio::process::Command;

#[derive(Debug, thiserror::Error)]
pub enum GitError {
    #[error("failed to run git: {0}")]
    Spawn(#[from] std::io::Error),
    #[error("git {args} failed: {stderr}")]
    Failed { args: String, stderr: String },
}

pub type Result<T> = std::result::Result<T, GitError>;

/// Runs `git -C <dir> <args>` and returns trimmed stdout.
pub async fn git(dir: &Path, args: &[&str]) -> Result<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .output()
        .await?;
    if !out.status.success() {
        return Err(GitError::Failed {
            args: args.join(" "),
            stderr: String::from_utf8_lossy(&out.stderr).trim().to_string(),
        });
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim_end().to_string())
}

/// Like [`git`] but returns `None` instead of an error when git exits non-zero.
pub async fn git_opt(dir: &Path, args: &[&str]) -> Result<Option<String>> {
    match git(dir, args).await {
        Ok(s) => Ok(Some(s)),
        Err(GitError::Failed { .. }) => Ok(None),
        Err(e) => Err(e),
    }
}

/// The top-level directory of the main working tree containing `path`.
pub async fn repo_root(path: &Path) -> Result<PathBuf> {
    // --git-common-dir resolves to the main repo even when `path` is inside a worktree.
    let common = git(
        path,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )
    .await?;
    let common = PathBuf::from(common);
    if common.file_name().is_some_and(|n| n == ".git") {
        if let Some(parent) = common.parent() {
            return Ok(parent.to_path_buf());
        }
    }
    Ok(PathBuf::from(
        git(path, &["rev-parse", "--show-toplevel"]).await?,
    ))
}

pub async fn remote_url(root: &Path, remote: &str) -> Result<Option<String>> {
    git_opt(root, &["remote", "get-url", remote]).await
}

/// Best guess at the repository's default branch name (without remote prefix).
pub async fn default_branch(root: &Path) -> Result<String> {
    if let Some(head) = git_opt(
        root,
        &["symbolic-ref", "--short", "refs/remotes/origin/HEAD"],
    )
    .await?
    {
        if let Some(name) = head.strip_prefix("origin/") {
            return Ok(name.to_string());
        }
    }
    for candidate in ["main", "master"] {
        if ref_exists(root, &format!("refs/heads/{candidate}")).await? {
            return Ok(candidate.to_string());
        }
    }
    Ok(git_opt(root, &["symbolic-ref", "--short", "HEAD"])
        .await?
        .unwrap_or_else(|| "main".to_string()))
}

pub async fn ref_exists(root: &Path, full_ref: &str) -> Result<bool> {
    Ok(
        git_opt(root, &["rev-parse", "--verify", "--quiet", full_ref])
            .await?
            .is_some(),
    )
}

pub async fn fetch(root: &Path, remote: &str) -> Result<()> {
    git(root, &["fetch", "--prune", remote]).await.map(|_| ())
}

/// `git worktree add -b <branch> <path> <base>`.
pub async fn worktree_add(root: &Path, path: &Path, branch: &str, base: &str) -> Result<()> {
    let path = path.to_string_lossy();
    git(
        root,
        &["worktree", "add", "--no-track", "-b", branch, &path, base],
    )
    .await
    .map(|_| ())
}

/// Removes a worktree (forcefully) and prunes stale worktree metadata.
pub async fn worktree_remove(root: &Path, path: &Path) -> Result<()> {
    let p = path.to_string_lossy();
    if path.exists() {
        git(root, &["worktree", "remove", "--force", &p]).await?;
    }
    git(root, &["worktree", "prune"]).await.map(|_| ())
}

/// Snapshot of uncommitted tracked changes as a commit object, without touching
/// the working tree or index. `None` when the tree is clean.
pub async fn stash_create(worktree: &Path) -> Result<Option<String>> {
    let sha = git(worktree, &["stash", "create"]).await?;
    Ok((!sha.is_empty()).then_some(sha))
}

pub async fn update_ref(root: &Path, full_ref: &str, sha: &str) -> Result<()> {
    git(root, &["update-ref", full_ref, sha]).await.map(|_| ())
}

/// Gitignored paths in the working tree; untracked ignored directories are
/// reported once (with a trailing `/`) rather than file by file.
pub async fn ignored_paths(root: &Path) -> Result<Vec<String>> {
    let out = git(
        root,
        &[
            "ls-files",
            "--others",
            "--ignored",
            "--exclude-standard",
            "--directory",
            "-z",
        ],
    )
    .await?;
    Ok(out
        .split('\0')
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect())
}

pub async fn merge_base(dir: &Path, a: &str, b: &str) -> Result<String> {
    git(dir, &["merge-base", a, b]).await
}

/// `(ahead, behind)` of `HEAD` relative to `base`.
pub async fn ahead_behind(dir: &Path, base: &str) -> Result<(u32, u32)> {
    let out = git(
        dir,
        &[
            "rev-list",
            "--left-right",
            "--count",
            &format!("HEAD...{base}"),
        ],
    )
    .await?;
    let mut parts = out
        .split_whitespace()
        .map(|n| n.parse::<u32>().unwrap_or(0));
    Ok((parts.next().unwrap_or(0), parts.next().unwrap_or(0)))
}

/// Unified diff of the working tree (including untracked, non-ignored files)
/// against `base`. Uses a throwaway index so the user's staging area is never
/// touched.
pub async fn diff_worktree(dir: &Path, base: &str) -> Result<String> {
    let git_dir =
        PathBuf::from(git(dir, &["rev-parse", "--path-format=absolute", "--git-dir"]).await?);
    let tmp_index = git_dir.join(format!("bantu-diff-index-{}", std::process::id()));
    let run = |args: Vec<&str>| {
        let tmp_index = tmp_index.clone();
        let args: Vec<String> = args.into_iter().map(str::to_string).collect();
        async move {
            let out = Command::new("git")
                .arg("-C")
                .arg(dir)
                .args(&args)
                .env("GIT_INDEX_FILE", &tmp_index)
                .env("GIT_TERMINAL_PROMPT", "0")
                .stdin(Stdio::null())
                .output()
                .await?;
            if !out.status.success() {
                return Err(GitError::Failed {
                    args: args.join(" "),
                    stderr: String::from_utf8_lossy(&out.stderr).trim().to_string(),
                });
            }
            Ok(String::from_utf8_lossy(&out.stdout).to_string())
        }
    };
    let result = async {
        run(vec!["read-tree", "HEAD"]).await?;
        run(vec!["add", "-A", "."]).await?;
        run(vec![
            "diff",
            "--cached",
            "--no-color",
            "--no-ext-diff",
            "-M",
            "--full-index",
            base,
        ])
        .await
    }
    .await;
    let _ = tokio::fs::remove_file(&tmp_index).await;
    result
}

/// Paths with uncommitted changes (tracked or untracked), per `git status`.
pub async fn uncommitted_count(dir: &Path) -> Result<u32> {
    let out = git(dir, &["status", "--porcelain", "--untracked-files=all"]).await?;
    Ok(out.lines().filter(|l| !l.trim().is_empty()).count() as u32)
}

/// An in-progress operation that would make committing unsafe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BusyState {
    Clean,
    Rebase,
    Merge,
    CherryPick,
    Revert,
}

/// Mirrors the detection in git's contrib/completion/git-prompt.sh.
pub async fn busy_state(worktree: &Path) -> Result<BusyState> {
    let git_dir = PathBuf::from(
        git(
            worktree,
            &["rev-parse", "--path-format=absolute", "--git-dir"],
        )
        .await?,
    );
    let has = |name: &str| git_dir.join(name).exists();
    Ok(if has("rebase-merge") || has("rebase-apply") {
        BusyState::Rebase
    } else if has("MERGE_HEAD") {
        BusyState::Merge
    } else if has("CHERRY_PICK_HEAD") {
        BusyState::CherryPick
    } else if has("REVERT_HEAD") {
        BusyState::Revert
    } else {
        BusyState::Clean
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn init_repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path();
        git(p, &["init", "-q", "-b", "main"]).await.unwrap();
        git(p, &["config", "user.email", "t@example.com"])
            .await
            .unwrap();
        git(p, &["config", "user.name", "t"]).await.unwrap();
        std::fs::write(p.join("a.txt"), "a\n").unwrap();
        std::fs::write(p.join(".gitignore"), ".env\n").unwrap();
        std::fs::write(p.join(".env"), "SECRET=1\n").unwrap();
        git(p, &["add", "."]).await.unwrap();
        git(p, &["commit", "-qm", "init"]).await.unwrap();
        dir
    }

    #[tokio::test]
    async fn worktree_lifecycle() {
        let repo = init_repo().await;
        let root = repo_root(repo.path()).await.unwrap();
        assert_eq!(default_branch(&root).await.unwrap(), "main");
        assert_eq!(
            ignored_paths(&root).await.unwrap(),
            vec![".env".to_string()]
        );

        let wt_parent = tempfile::tempdir().unwrap();
        let wt = wt_parent.path().join("lisbon");
        worktree_add(&root, &wt, "bantu/lisbon", "main")
            .await
            .unwrap();
        assert!(wt.join("a.txt").exists());
        assert_eq!(repo_root(&wt).await.unwrap(), root);
        assert_eq!(busy_state(&wt).await.unwrap(), BusyState::Clean);

        assert_eq!(stash_create(&wt).await.unwrap(), None);
        std::fs::write(wt.join("a.txt"), "changed\n").unwrap();
        let sha = stash_create(&wt).await.unwrap().expect("dirty tree");
        update_ref(&root, "refs/bantu/archive/x", &sha)
            .await
            .unwrap();

        worktree_remove(&root, &wt).await.unwrap();
        assert!(!wt.exists());
        assert!(ref_exists(&root, "refs/heads/bantu/lisbon").await.unwrap());
        assert!(ref_exists(&root, "refs/bantu/archive/x").await.unwrap());
    }
}
