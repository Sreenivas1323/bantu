//! The lead's view of its child workspaces: a live board, and merging their
//! branches into the lead's. Merges stay local. Pushing is left to the agent or
//! the user, so all of this can be undone with `git reset`.

use std::path::Path;
use std::sync::Arc;

use bantu_git as git;
use bantu_protocol::{
    ChildStatus, ChildWorkspace, IntegrationResult, IntegrationSkip, Session, SessionStatus,
    Workspace, WorkspaceState,
};

use crate::ops::{CommandError, Daemon};

type CmdResult<T> = Result<T, CommandError>;

impl Daemon {
    async fn child_workspaces(&self, parent_id: &str) -> CmdResult<Vec<Workspace>> {
        Ok(self
            .store
            .workspaces()
            .await?
            .into_iter()
            .filter(|w| {
                w.parent_id.as_deref() == Some(parent_id) && w.state != WorkspaceState::Archived
            })
            .collect())
    }

    /// Everything the board shows for each child.
    pub async fn workspace_children(&self, workspace_id: &str) -> CmdResult<Vec<ChildWorkspace>> {
        self.require_workspace(workspace_id).await?;
        let sessions = self.store.sessions().await?;
        let waiting = self.pending_permissions().await;
        let inbox = self.inbox_items().await;
        let conflicts = self.all_conflicts().await;
        let mut out = Vec::new();
        for ws in self.child_workspaces(workspace_id).await? {
            let mine: Vec<&Session> = sessions
                .iter()
                .filter(|s| s.workspace_id == ws.id)
                .collect();
            let needs_user = waiting.iter().any(|p| p.workspace_id == ws.id)
                || inbox.iter().any(|i| {
                    i.workspace_id == ws.id && i.kind == bantu_protocol::InboxKind::Question
                });
            let status = if ws.state == WorkspaceState::Failed {
                ChildStatus::Failed
            } else if ws.state != WorkspaceState::Ready {
                ChildStatus::Starting
            } else if needs_user {
                ChildStatus::Waiting
            } else if mine.iter().any(|s| s.status == SessionStatus::Running) {
                ChildStatus::Working
            } else {
                ChildStatus::Idle
            };
            let title = mine
                .iter()
                .rev()
                .map(|s| s.title.clone())
                .find(|t| t != "New chat");
            let (mut files, mut additions, mut deletions, mut ahead, mut uncommitted) =
                (0, 0, 0, 0, 0);
            if ws.state == WorkspaceState::Ready {
                if let Ok(diff) = self.workspace_diff(&ws.id).await {
                    files = diff.files.len() as u32;
                    additions = diff.files.iter().map(|f| f.additions).sum();
                    deletions = diff.files.iter().map(|f| f.deletions).sum();
                    ahead = diff.ahead;
                    uncommitted = diff.uncommitted;
                }
            }
            out.push(ChildWorkspace {
                conflicted: conflicts.iter().any(|c| {
                    c.workspace_id == ws.id || c.other_workspace_id.as_ref() == Some(&ws.id)
                }),
                last_reply: self.last_answer(&ws).await,
                workspace: ws,
                status,
                title,
                files,
                additions,
                deletions,
                ahead,
                uncommitted,
            });
        }
        Ok(out)
    }

    /// Merges finished children into the lead's branch, one at a time. The first
    /// conflict aborts that merge, so the lead's branch is never left half
    /// merged. With a chat to send it to, its agent is asked to take over.
    pub async fn integrate_children(
        self: &Arc<Self>,
        workspace_id: &str,
        session_id: Option<&str>,
        // An agent calling this as a tool is mid-turn, which is fine.
        caller: Option<&str>,
    ) -> CmdResult<IntegrationResult> {
        let lead = self.ready_workspace(workspace_id).await?;
        let path = Path::new(&lead.path);
        let sessions = self.store.sessions().await?;
        if sessions.iter().any(|s| {
            s.workspace_id == lead.id
                && s.status == SessionStatus::Running
                && Some(s.id.as_str()) != caller
        }) {
            return Err(CommandError::Conflict(
                "the lead's agent is working; wait for it to finish first".into(),
            ));
        }
        if git::busy_state(path).await.map_err(anyhow::Error::from)? != git::BusyState::Clean {
            return Err(CommandError::Conflict(
                "the lead has a merge or rebase in progress".into(),
            ));
        }
        let dirty = git::git(path, &["status", "--porcelain", "--untracked-files=no"])
            .await
            .map_err(anyhow::Error::from)?;
        if !dirty.is_empty() {
            return Err(CommandError::Conflict(
                "the lead has uncommitted changes; commit them first".into(),
            ));
        }

        let mut result = IntegrationResult {
            merged: vec![],
            skipped: vec![],
            conflict: None,
            sent_to_agent: false,
        };
        let skip = |result: &mut IntegrationResult, ws: &Workspace, reason: String| {
            result.skipped.push(IntegrationSkip {
                workspace: ws.name.clone(),
                reason,
            });
        };
        let mut remaining = Vec::new();
        for child in self.child_workspaces(&lead.id).await? {
            if child.state != WorkspaceState::Ready {
                skip(
                    &mut result,
                    &child,
                    format!("it's {}", child.state.as_str()),
                );
                continue;
            }
            if sessions
                .iter()
                .any(|s| s.workspace_id == child.id && s.status == SessionStatus::Running)
            {
                skip(&mut result, &child, "its agent is still working".into());
                continue;
            }
            let uncommitted = git::uncommitted_count(Path::new(&child.path))
                .await
                .map_err(anyhow::Error::from)?;
            if uncommitted > 0 {
                skip(
                    &mut result,
                    &child,
                    format!("it has {uncommitted} uncommitted file(s)"),
                );
                continue;
            }
            let already = git::git_opt(
                path,
                &["merge-base", "--is-ancestor", &child.branch, "HEAD"],
            )
            .await
            .map_err(anyhow::Error::from)?;
            if already.is_some() {
                skip(&mut result, &child, "there's nothing new to merge".into());
                continue;
            }
            remaining.push(child);
        }

        let mut queue = remaining.iter();
        for child in queue.by_ref() {
            let merge = git::git(path, &["merge", "--no-edit", &child.branch]).await;
            match merge {
                Ok(_) => {
                    self.store
                        .log(
                            &lead.id,
                            bantu_protocol::LogStream::System,
                            format!("Merged {} ({})", child.name, child.branch),
                        )
                        .await;
                    result.merged.push(child.name.clone());
                }
                Err(e) => {
                    let unmerged = git::git(path, &["diff", "--name-only", "--diff-filter=U"])
                        .await
                        .unwrap_or_default();
                    let _ = git::git(path, &["merge", "--abort"]).await;
                    if unmerged.is_empty() {
                        // Not a conflict: e.g. no git identity, or a failing hook.
                        return Err(CommandError::Conflict(format!(
                            "couldn't merge {}: {e}",
                            child.name
                        )));
                    }
                    result.conflict = Some(child.name.clone());
                    if let Some(session_id) = session_id {
                        let rest: Vec<String> = std::iter::once(child)
                            .chain(queue.by_ref())
                            .map(|w| format!("`{}`", w.branch))
                            .collect();
                        let prompt = format!(
                            "Integrate these branches into this one, in order, with `git merge`: \
                             {}. The first one conflicts in: {}. Resolve conflicts by keeping the \
                             intent of both sides, run the relevant tests, and summarize what you \
                             decided. Don't push.",
                            rest.join(", "),
                            unmerged.lines().collect::<Vec<_>>().join(", "),
                        );
                        self.send_message(session_id, prompt).await?;
                        result.sent_to_agent = true;
                    }
                    break;
                }
            }
        }
        // Anything left after a conflict wasn't attempted.
        for child in queue {
            skip(
                &mut result,
                child,
                "not attempted after the conflict".into(),
            );
        }
        self.rescan_conflicts(&lead.repo_id);
        Ok(result)
    }
}

/// How an agent reads the outcome of `integrate_workspaces`.
pub fn describe(result: &IntegrationResult) -> String {
    let mut lines = Vec::new();
    if !result.merged.is_empty() {
        lines.push(format!("Merged: {}.", result.merged.join(", ")));
    }
    if let Some(name) = &result.conflict {
        lines.push(format!(
            "{name} conflicts with this branch, so that merge was aborted. Merge it yourself and \
             resolve the conflicts."
        ));
    }
    for skip in &result.skipped {
        lines.push(format!("Skipped {}: {}.", skip.workspace, skip.reason));
    }
    if lines.is_empty() {
        "This workspace has no child workspaces to merge.".into()
    } else {
        lines.join("\n")
    }
}
