//! Spotlight: try a workspace's work in the repo's main checkout, where the
//! user's own dev servers, `.env` files and tools already live.
//!
//! The main checkout moves to a detached snapshot of the workspace, including
//! its uncommitted work, and follows it after each agent turn. Stopping returns
//! it to the branch it was on. The main checkout belongs to the user, so
//! anything uncommitted there stops the process instead of being overwritten.

use std::path::Path;
use std::sync::Arc;

use bantu_git as git;
use bantu_protocol::{Repo, Spotlight, Workspace};

use crate::ops::{CommandError, Daemon};
use crate::store::now;

type CmdResult<T> = Result<T, CommandError>;

fn snapshot_ref(repo_id: &str) -> String {
    format!("refs/bantu/spotlight/{repo_id}")
}

impl Daemon {
    pub async fn start_spotlight(self: &Arc<Self>, workspace_id: &str) -> CmdResult<Spotlight> {
        let ws = self.ready_workspace(workspace_id).await?;
        let repo = self
            .store
            .repo(&ws.repo_id)
            .await?
            .ok_or_else(|| CommandError::NotFound("repository is gone".into()))?;
        let root = Path::new(&repo.root_path);
        let current = self.store.spotlight(&repo.id).await?;
        require_clean(root).await?;

        // Moving between workspaces keeps the original branch to return to.
        let restore_to = match &current {
            Some(s) => s.restore_to.clone(),
            None => git::git_opt(root, &["symbolic-ref", "--short", "HEAD"])
                .await
                .map_err(anyhow::Error::from)?
                .unwrap_or(
                    git::git(root, &["rev-parse", "HEAD"])
                        .await
                        .map_err(anyhow::Error::from)?,
                ),
        };
        let commit = self.show_in_main_checkout(&ws, &repo).await?;
        let spotlight = Spotlight {
            repo_id: repo.id.clone(),
            workspace_id: ws.id.clone(),
            restore_to,
            commit,
            synced_at: now(),
        };
        self.store.set_spotlight(&repo.id, Some(&spotlight)).await?;
        Ok(spotlight)
    }

    pub async fn stop_spotlight(&self, repo_id: &str) -> CmdResult<()> {
        let spotlight = self
            .store
            .spotlight(repo_id)
            .await?
            .ok_or_else(|| CommandError::NotFound("nothing is in the spotlight".into()))?;
        let repo = self
            .store
            .repo(repo_id)
            .await?
            .ok_or_else(|| CommandError::NotFound("repository is gone".into()))?;
        let root = Path::new(&repo.root_path);
        require_clean(root).await?;
        // A branch name switches back to it; a sha returns to a detached HEAD.
        git::git(root, &["checkout", "--quiet", &spotlight.restore_to])
            .await
            .map_err(|e| CommandError::Conflict(format!("couldn't switch back: {e}")))?;
        let _ = git::git(root, &["update-ref", "-d", &snapshot_ref(repo_id)]).await;
        self.store.set_spotlight(repo_id, None).await?;
        Ok(())
    }

    /// Follows the workspace after an agent turn. If that isn't possible, the
    /// reason goes in the workspace log and the main checkout stays as it is.
    pub(crate) async fn sync_spotlight(&self, ws: &Workspace) {
        let Ok(Some(mut spotlight)) = self.store.spotlight(&ws.repo_id).await else {
            return;
        };
        if spotlight.workspace_id != ws.id {
            return;
        }
        let Ok(Some(repo)) = self.store.repo(&ws.repo_id).await else {
            return;
        };
        let result = async {
            require_clean(Path::new(&repo.root_path)).await?;
            self.show_in_main_checkout(ws, &repo).await
        }
        .await;
        match result {
            Ok(commit) => {
                spotlight.commit = commit;
                spotlight.synced_at = now();
                let _ = self.store.set_spotlight(&repo.id, Some(&spotlight)).await;
            }
            Err(e) => {
                self.store
                    .log(
                        &ws.id,
                        bantu_protocol::LogStream::System,
                        format!("Spotlight didn't update: {e}"),
                    )
                    .await;
            }
        }
    }

    /// Checks out a snapshot of the workspace in the main checkout. The ref
    /// keeps the snapshot from being garbage-collected while it's checked out.
    async fn show_in_main_checkout(&self, ws: &Workspace, repo: &Repo) -> CmdResult<String> {
        let root = Path::new(&repo.root_path);
        let snapshot = git::snapshot(Path::new(&ws.path))
            .await
            .map_err(anyhow::Error::from)?;
        let head_tree = git::git(root, &["rev-parse", "HEAD^{tree}"])
            .await
            .map_err(anyhow::Error::from)?;
        let new_tree = git::git(root, &["rev-parse", &format!("{snapshot}^{{tree}}")])
            .await
            .map_err(anyhow::Error::from)?;
        git::update_ref(root, &snapshot_ref(&repo.id), &snapshot)
            .await
            .map_err(anyhow::Error::from)?;
        if head_tree != new_tree {
            git::git(root, &["checkout", "--quiet", "--detach", &snapshot])
                .await
                .map_err(|e| {
                    CommandError::Conflict(format!(
                        "couldn't update the main checkout (an untracked file may be in the \
                         way): {e}"
                    ))
                })?;
        }
        Ok(snapshot)
    }
}

/// The main checkout must have nothing to lose, and no merge or rebase running.
async fn require_clean(root: &Path) -> CmdResult<()> {
    let busy = git::busy_state(root).await.map_err(anyhow::Error::from)?;
    if busy != git::BusyState::Clean {
        return Err(CommandError::Conflict(
            "the main checkout has a merge, rebase or cherry-pick in progress".into(),
        ));
    }
    let status = git::git(root, &["status", "--porcelain", "--untracked-files=no"])
        .await
        .map_err(anyhow::Error::from)?;
    if !status.is_empty() {
        return Err(CommandError::Conflict(format!(
            "the main checkout ({}) has uncommitted changes; commit or stash them first",
            root.display()
        )));
    }
    Ok(())
}
