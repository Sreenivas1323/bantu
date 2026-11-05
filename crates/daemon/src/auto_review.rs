//! Automatic cross-agent review, opt-in with `[review] auto = true`.
//!
//! When an agent finishes work that changed files and passed the repo's check,
//! a different agent reviews it in a read-only chat. Approval marks the workspace
//! ready. Findings go back to the author, which gets a limited number of rounds
//! so two agents can't spend the user's quota arguing forever.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use bantu_git as git;
use bantu_protocol::{
    AgentKind, ChatItem, LogStream, PermissionMode, ReviewState, Session, Workspace,
};
use futures::future::BoxFuture;
use tokio::sync::Mutex;

use crate::ops::Daemon;
use crate::settings::PromptKind;

/// How many times findings go back to the author before a person decides.
const MAX_ROUNDS: u32 = 2;
const APPROVED: &str = "VERDICT: APPROVED";
const CHANGES: &str = "VERDICT: CHANGES_REQUESTED";

#[derive(Default)]
pub struct AutoReview {
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    /// Reviews in flight, by the reviewer's session id.
    running: HashMap<String, Run>,
    /// Findings sent back so far, per workspace.
    rounds: HashMap<String, u32>,
    /// The last tree reviewed in each workspace, and how that went.
    reviewed: HashMap<String, (String, ReviewState)>,
}

struct Run {
    author: String,
    tree: String,
}

impl Daemon {
    pub(crate) async fn is_reviewer(&self, session_id: &str) -> bool {
        self.auto_review
            .state
            .lock()
            .await
            .running
            .contains_key(session_id)
    }

    /// A person gave the author new direction, so it gets fresh rounds.
    pub(crate) async fn reset_review_rounds(&self, workspace_id: &str) {
        self.auto_review
            .state
            .lock()
            .await
            .rounds
            .remove(workspace_id);
    }

    pub(crate) async fn set_review_state(&self, workspace_id: &str, state: Option<ReviewState>) {
        // Read fresh: a clone from earlier could undo other changes.
        let Ok(mut ws) = self.require_workspace(workspace_id).await else {
            return;
        };
        if ws.review_state != state {
            ws.review_state = state;
            if let Err(e) = self.store.upsert_workspace(&ws).await {
                tracing::warn!("couldn't record the review state of {workspace_id}: {e:#}");
            }
        }
    }

    /// Reviews left running by a previous process can't report back.
    pub(crate) async fn recover_reviews(&self) -> anyhow::Result<()> {
        for ws in self.store.workspaces().await? {
            if ws.review_state == Some(ReviewState::Reviewing) {
                self.set_review_state(&ws.id, None).await;
            }
        }
        Ok(())
    }

    /// Called when a chat has finished all its queued work. Boxed because this
    /// can start another chat's turns, which end up back here.
    pub(crate) fn after_turns(self: Arc<Self>, s: Session) -> BoxFuture<'static, ()> {
        Box::pin(async move {
            let run = self.auto_review.state.lock().await.running.remove(&s.id);
            match run {
                Some(run) => self.finish_review(&s, run).await,
                None => {
                    if let Err(e) = self.maybe_start_review(&s).await {
                        tracing::warn!("automatic review didn't start for {}: {e:#}", s.id);
                    }
                }
            }
        })
    }

    async fn maybe_start_review(self: &Arc<Self>, s: &Session) -> anyhow::Result<()> {
        if s.permission_mode == PermissionMode::Plan {
            return Ok(());
        }
        let ws = self.require_workspace(&s.workspace_id).await?;
        let Some(repo) = self.store.repo(&ws.repo_id).await? else {
            return Ok(());
        };
        let path = Path::new(&ws.path);
        let Ok(settings) = crate::settings::load(&[path, Path::new(&repo.root_path)]) else {
            return Ok(());
        };
        if !settings.review.auto {
            return Ok(());
        }
        // Work that fails the repo's own check isn't ready for review, and a
        // failed turn may be half done.
        let items = self.store.session_items(&s.id).await?;
        let unfinished = items.iter().rev().find_map(|(_, item)| match item {
            ChatItem::Check { passed, .. } => Some(!passed),
            ChatItem::Error { .. } => Some(true),
            ChatItem::TurnEnd { is_error, .. } => Some(*is_error),
            _ => None,
        });
        if unfinished == Some(true) {
            return Ok(());
        }
        let merge_base = git::merge_base(path, "HEAD", &ws.base_ref).await?;
        let tree = worktree_tree(path).await?;
        let base_tree = git::git(path, &["rev-parse", &format!("{merge_base}^{{tree}}")]).await?;
        if tree == base_tree {
            return Ok(());
        }
        // Nothing changed since the last review, e.g. the user asked a question.
        let previous = self
            .auto_review
            .state
            .lock()
            .await
            .reviewed
            .get(&ws.id)
            .filter(|(reviewed, _)| *reviewed == tree)
            .map(|(_, verdict)| *verdict);
        if let Some(verdict) = previous {
            self.set_review_state(&ws.id, Some(verdict)).await;
            return Ok(());
        }

        let agent = settings
            .review
            .agent
            .as_deref()
            .and_then(AgentKind::parse)
            .unwrap_or(if s.agent == AgentKind::Claude {
                AgentKind::Codex
            } else {
                AgentKind::Claude
            });
        let reviewer = self
            .create_session(&ws.id, Some(agent), None, Some(PermissionMode::Plan))
            .await?;
        let reviewer = self
            .update_session(&reviewer.id, Some("Automatic review".into()), None, None)
            .await?;
        self.auto_review.state.lock().await.running.insert(
            reviewer.id.clone(),
            Run {
                author: s.id.clone(),
                tree,
            },
        );
        self.set_review_state(&ws.id, Some(ReviewState::Reviewing))
            .await;
        let prompt = format!(
            "{}\n\nEnd your reply with a final line that is exactly `{APPROVED}` if you found \
             nothing significant, or `{CHANGES}` if the author should fix what you listed.",
            crate::review::review_prompt(&ws.branch, &ws.base_ref, &merge_base)
        );
        let prompt = self
            .with_repo_prompt(&ws.id, PromptKind::Review, prompt)
            .await;
        if let Err(e) = self.send_message(&reviewer.id, prompt).await {
            self.auto_review
                .state
                .lock()
                .await
                .running
                .remove(&reviewer.id);
            self.set_review_state(&ws.id, None).await;
            return Err(e.into());
        }
        Ok(())
    }

    async fn finish_review(self: &Arc<Self>, reviewer: &Session, run: Run) {
        let Ok(ws) = self.require_workspace(&reviewer.workspace_id).await else {
            return;
        };
        // The author moved on while this ran; its next stop starts a new review.
        match worktree_tree(Path::new(&ws.path)).await {
            Ok(tree) if tree == run.tree => {}
            _ => return,
        }
        let items = self
            .store
            .session_items(&reviewer.id)
            .await
            .unwrap_or_default();
        let reply = items.iter().rev().find_map(|(turn, item)| match item {
            ChatItem::Assistant { text } if *turn == reviewer.turns => Some(text.clone()),
            _ => None,
        });
        match reply.as_deref().and_then(parse_verdict) {
            Some(true) => self.approve(&ws, run.tree).await,
            Some(false) => {
                let findings = reply.unwrap_or_default();
                self.request_changes(&ws, reviewer, run, findings).await;
            }
            None => {
                self.set_review_state(&ws.id, None).await;
                self.store
                    .log(
                        &ws.id,
                        LogStream::System,
                        format!(
                            "Automatic review by {} finished without a verdict; see its chat.",
                            reviewer.agent.as_str()
                        ),
                    )
                    .await;
            }
        }
    }

    async fn approve(&self, ws: &Workspace, tree: String) {
        {
            let mut state = self.auto_review.state.lock().await;
            state.rounds.remove(&ws.id);
            state
                .reviewed
                .insert(ws.id.clone(), (tree, ReviewState::Approved));
        }
        self.set_review_state(&ws.id, Some(ReviewState::Approved))
            .await;
    }

    async fn request_changes(
        self: &Arc<Self>,
        ws: &Workspace,
        reviewer: &Session,
        run: Run,
        findings: String,
    ) {
        let round = {
            let mut state = self.auto_review.state.lock().await;
            state
                .reviewed
                .insert(ws.id.clone(), (run.tree, ReviewState::ChangesRequested));
            let round = state.rounds.entry(ws.id.clone()).or_insert(0);
            *round += 1;
            *round
        };
        self.set_review_state(&ws.id, Some(ReviewState::ChangesRequested))
            .await;
        let author = self.store.session(&run.author).await.ok().flatten();
        let Some((author, _)) = author else {
            return;
        };
        if round > MAX_ROUNDS {
            self.notify_user(
                &author,
                "Automatic review still has concerns".into(),
                Some(format!(
                    "{} still requests changes after {MAX_ROUNDS} rounds. See its chat and decide.",
                    reviewer.agent.as_str()
                )),
            )
            .await;
            return;
        }
        let message = format!(
            "[Bantu] An automatic review by {} found problems in your changes. Fix what's right, \
             and say why if you disagree with something.\n\n{findings}",
            reviewer.agent.as_str()
        );
        if let Err(e) = self.send_message(&author.id, message).await {
            tracing::warn!("couldn't send review findings to {}: {e:#}", author.id);
        }
    }
}

/// The worktree's contents, including uncommitted work, as a tree id.
async fn worktree_tree(path: &Path) -> anyhow::Result<String> {
    let snapshot = git::snapshot(path).await?;
    Ok(git::git(path, &["rev-parse", &format!("{snapshot}^{{tree}}")]).await?)
}

/// `Some(true)` for approved, `Some(false)` for changes requested. Only the last
/// line that counts, so a review quoting the instructions isn't misread.
pub fn parse_verdict(reply: &str) -> Option<bool> {
    let last = reply
        .lines()
        .rev()
        .map(|l| l.trim().trim_matches(['*', '`', '_', ' ']))
        .find(|l| !l.is_empty())?;
    if last == APPROVED {
        Some(true)
    } else if last == CHANGES {
        Some(false)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_only_the_final_verdict() {
        assert_eq!(parse_verdict("Fine.\n\nVERDICT: APPROVED"), Some(true));
        assert_eq!(parse_verdict("**VERDICT: APPROVED**\n"), Some(true));
        assert_eq!(
            parse_verdict("1. bug\n`VERDICT: CHANGES_REQUESTED`"),
            Some(false)
        );
        assert_eq!(
            parse_verdict("I would say VERDICT: APPROVED is wrong"),
            None
        );
        assert_eq!(
            parse_verdict("VERDICT: APPROVED\nActually, one more thing."),
            None
        );
        assert_eq!(parse_verdict(""), None);
    }
}
