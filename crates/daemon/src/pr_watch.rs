//! Pull requests for every workspace, kept fresh in the background.
//!
//! The fleet board needs each workspace's PR (CI, reviews, conflicts) without
//! opening each one, so this asks GitHub once per repo every 90 seconds with a
//! single `gh pr list`.
//!
//! With `[pr] follow_up = true`, it also sends the agent that owns a PR what
//! needs doing: failed CI once per commit, conflicts once per commit, and new
//! review comments once per batch. It only acts while that agent is idle, and
//! after three automatic rounds without a word from you, it stops and tells you.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use bantu_protocol::{
    EventData, PermissionMode, PullRequest, SessionStatus, Workspace, WorkspacePr, WorkspaceState,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::Mutex;

use crate::ops::Daemon;
use crate::review::parse_pr;

const POLL_EVERY: Duration = Duration::from_secs(90);
/// Automatic follow-ups per PR before a person has to step in.
pub const MAX_ROUNDS: u32 = 3;

#[derive(Default)]
pub struct PrWatch {
    /// The latest PR for each workspace, by workspace id.
    prs: Mutex<HashMap<String, WorkspacePr>>,
}

/// One PR from `gh pr list`.
#[derive(Debug, Clone, PartialEq)]
pub struct Listed {
    pub branch: String,
    pub pr: PullRequest,
    pub mergeable: Option<bool>,
    pub head_sha: String,
    pub updated_at: Option<String>,
    /// Review comments and reviews with something to say, from anyone but
    /// the PR's author.
    pub feedback: u32,
}

/// What follow-up has already done for a workspace. Kept across restarts.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct Handled {
    /// The commit whose failing CI was last sent to the agent.
    pub checks_sha: Option<String>,
    /// The commit whose conflict was last sent to the agent.
    pub conflict_sha: Option<String>,
    /// How much feedback the agent has already been asked to address.
    pub feedback: u32,
    /// Automatic follow-ups since you last wrote in the workspace.
    pub rounds: u32,
    /// Whether you've been told that it reached its limit.
    pub notified: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    ResolveConflict,
    FixChecks,
    AddressFeedback,
}

/// Each branch's most relevant PR: an open one if there is one, otherwise the
/// most recently updated.
pub fn parse_pr_list(list: &Value) -> HashMap<String, Listed> {
    let mut out: HashMap<String, Listed> = HashMap::new();
    for v in list.as_array().into_iter().flatten() {
        let Some(pr) = parse_pr(v) else { continue };
        let Some(branch) = v["headRefName"].as_str() else {
            continue;
        };
        let author = v["author"]["login"].as_str().unwrap_or_default();
        let from_others = |item: &&Value| item["author"]["login"].as_str() != Some(author);
        let comments = v["comments"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(from_others)
            .count();
        let reviews = v["reviews"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(from_others)
            .filter(|r| {
                r["state"] == "CHANGES_REQUESTED"
                    || (r["state"] == "COMMENTED"
                        && !r["body"].as_str().unwrap_or_default().trim().is_empty())
            })
            .count();
        let listed = Listed {
            branch: branch.to_string(),
            mergeable: match v["mergeable"].as_str() {
                Some("MERGEABLE") => Some(true),
                Some("CONFLICTING") => Some(false),
                _ => None,
            },
            head_sha: v["headRefOid"].as_str().unwrap_or_default().to_string(),
            updated_at: v["updatedAt"].as_str().map(String::from),
            feedback: (comments + reviews) as u32,
            pr,
        };
        let better = match out.get(branch) {
            None => true,
            Some(current) => {
                let open = |l: &Listed| l.pr.state == "OPEN";
                (open(&listed) && !open(current))
                    || (open(&listed) == open(current) && listed.updated_at > current.updated_at)
            }
        };
        if better {
            out.insert(branch.to_string(), listed);
        }
    }
    out
}

/// What, if anything, to send the agent next. Conflicts come first, because
/// CI on a conflicting branch is often meaningless; then failing CI; then
/// reviewers' comments.
pub fn next_action(h: &Handled, l: &Listed) -> Option<Action> {
    if l.pr.state != "OPEN" || l.pr.is_draft || l.head_sha.is_empty() {
        return None;
    }
    if l.mergeable == Some(false) && h.conflict_sha.as_deref() != Some(&l.head_sha) {
        return Some(Action::ResolveConflict);
    }
    if l.pr.checks_failing > 0
        && l.pr.checks_pending == 0
        && h.checks_sha.as_deref() != Some(&l.head_sha)
    {
        return Some(Action::FixChecks);
    }
    if l.feedback > h.feedback {
        return Some(Action::AddressFeedback);
    }
    None
}

fn conflict_message(pr: &PullRequest, branch: &str, base: &str) -> String {
    format!(
        "[Bantu] Pull request #{number} ({url}) can't be merged: `{branch}` conflicts with \
         `{base}`.\n\nCommit any work in progress, then merge `{base}` into this branch, resolve \
         the conflicts keeping the intent of both sides, run the relevant tests and push. \
         Summarize what you decided.",
        number = pr.number,
        url = pr.url,
    )
}

impl Daemon {
    pub async fn pull_requests(&self) -> Vec<WorkspacePr> {
        let mut prs: Vec<WorkspacePr> = self.pr_watch.prs.lock().await.values().cloned().collect();
        prs.sort_by(|a, b| a.workspace_id.cmp(&b.workspace_id));
        prs
    }

    pub fn start_pr_watch(self: &Arc<Self>) {
        let this = self.clone();
        tokio::spawn(async move {
            loop {
                this.poll_pull_requests().await;
                tokio::time::sleep(POLL_EVERY).await;
            }
        });
    }

    /// A person wrote in this workspace, so follow-up gets fresh rounds.
    pub(crate) async fn reset_pr_rounds(&self, workspace_id: &str) {
        let mut handled = self.pr_handled(workspace_id).await;
        if handled.rounds == 0 && !handled.notified {
            return;
        }
        handled.rounds = 0;
        handled.notified = false;
        self.save_pr_handled(workspace_id, &handled).await;
        let updated = {
            let mut prs = self.pr_watch.prs.lock().await;
            prs.get_mut(workspace_id).map(|pr| {
                pr.follow_ups = 0;
                pr.needs_you = false;
                pr.clone()
            })
        };
        if let Some(pr) = updated {
            let _ = self.store.emit(EventData::PrUpdated { pr }).await;
        }
    }

    async fn pr_handled(&self, workspace_id: &str) -> Handled {
        self.store
            .private_value(&format!("pr-follow:{workspace_id}"))
            .await
            .ok()
            .flatten()
            .and_then(|v| serde_json::from_str(&v).ok())
            .unwrap_or_default()
    }

    async fn save_pr_handled(&self, workspace_id: &str, h: &Handled) {
        if let Ok(v) = serde_json::to_string(h) {
            let _ = self
                .store
                .set_private_value(&format!("pr-follow:{workspace_id}"), &v)
                .await;
        }
    }

    async fn poll_pull_requests(self: &Arc<Self>) {
        let (Ok(repos), Ok(workspaces)) = (self.store.repos().await, self.store.workspaces().await)
        else {
            return;
        };
        for repo in repos.iter().filter(|r| r.remote_url.is_some()) {
            let mine: Vec<&Workspace> = workspaces
                .iter()
                .filter(|w| w.repo_id == repo.id && w.state == WorkspaceState::Ready)
                .collect();
            if mine.is_empty() {
                continue;
            }
            let Some(list) = crate::ops::gh_json(
                Path::new(&repo.root_path),
                &[
                    "pr",
                    "list",
                    "--state",
                    "all",
                    "--limit",
                    "100",
                    "--json",
                    "number,url,title,state,isDraft,reviewDecision,statusCheckRollup,mergeable,\
                     headRefName,headRefOid,updatedAt,author,comments,reviews",
                ],
            )
            .await
            else {
                // `gh` missing or signed out: the board just shows no PRs.
                continue;
            };
            let listed = parse_pr_list(&list);
            for ws in mine {
                self.update_workspace_pr(ws, listed.get(&ws.branch)).await;
            }
        }
        // Workspaces that went away take their PRs with them.
        let live: Vec<&str> = workspaces
            .iter()
            .filter(|w| w.state == WorkspaceState::Ready)
            .map(|w| w.id.as_str())
            .collect();
        let gone: Vec<String> = self
            .pr_watch
            .prs
            .lock()
            .await
            .keys()
            .filter(|id| !live.contains(&id.as_str()))
            .cloned()
            .collect();
        for id in gone {
            self.pr_watch.prs.lock().await.remove(&id);
            let _ = self
                .store
                .emit(EventData::PrRemoved { workspace_id: id })
                .await;
        }
    }

    async fn update_workspace_pr(self: &Arc<Self>, ws: &Workspace, listed: Option<&Listed>) {
        let Some(listed) = listed else {
            if self.pr_watch.prs.lock().await.remove(&ws.id).is_some() {
                let _ = self
                    .store
                    .emit(EventData::PrRemoved {
                        workspace_id: ws.id.clone(),
                    })
                    .await;
            }
            return;
        };
        let mut handled = self.pr_handled(&ws.id).await;
        if let Err(e) = self.follow_up(ws, listed, &mut handled).await {
            tracing::warn!("PR follow-up for {}: {e}", ws.name);
        }
        let pr = WorkspacePr {
            workspace_id: ws.id.clone(),
            pull_request: listed.pr.clone(),
            mergeable: listed.mergeable,
            head_sha: listed.head_sha.clone(),
            updated_at: listed.updated_at.clone(),
            follow_ups: handled.rounds,
            needs_you: handled.notified,
        };
        let changed = {
            let mut prs = self.pr_watch.prs.lock().await;
            let changed = prs.get(&ws.id) != Some(&pr);
            prs.insert(ws.id.clone(), pr.clone());
            changed
        };
        if changed {
            let _ = self.store.emit(EventData::PrUpdated { pr }).await;
        }
    }

    async fn follow_up(
        self: &Arc<Self>,
        ws: &Workspace,
        listed: &Listed,
        handled: &mut Handled,
    ) -> anyhow::Result<()> {
        let Ok(settings) = self.trusted_settings(ws).await else {
            return Ok(());
        };
        if !settings.pr.follow_up {
            return Ok(());
        }
        let Some(action) = next_action(handled, listed) else {
            return Ok(());
        };
        // Only while the agent is free, and nobody's waiting on you already.
        let sessions = self.store.sessions().await?;
        let mine: Vec<_> = sessions
            .iter()
            .filter(|s| s.workspace_id == ws.id)
            .collect();
        if mine.iter().any(|s| s.status == SessionStatus::Running) {
            return Ok(());
        }
        let waiting =
            self.pending_permissions()
                .await
                .iter()
                .any(|p| p.workspace_id == ws.id)
                || self.inbox_items().await.iter().any(|i| {
                    i.workspace_id == ws.id && i.kind == bantu_protocol::InboxKind::Question
                });
        if waiting {
            return Ok(());
        }
        // The chat that did the work: the newest one that can edit files.
        let Some(session) = mine
            .iter()
            .filter(|s| s.permission_mode != PermissionMode::Plan && s.title != "Automatic review")
            .max_by(|a, b| a.created_at.cmp(&b.created_at))
        else {
            return Ok(());
        };

        if handled.rounds >= MAX_ROUNDS {
            if !handled.notified {
                handled.notified = true;
                self.save_pr_handled(&ws.id, handled).await;
                self.notify_user(
                    session,
                    format!("Pull request #{} needs you", listed.pr.number),
                    Some(format!(
                        "Bantu sent {MAX_ROUNDS} automatic follow-ups for {} and it still isn't \
                         ready. Write in the workspace to give it more.",
                        ws.name
                    )),
                )
                .await;
            }
            return Ok(());
        }

        let sent = match action {
            Action::ResolveConflict => {
                handled.conflict_sha = Some(listed.head_sha.clone());
                let message = conflict_message(&listed.pr, &ws.branch, &ws.base_ref);
                let message = self
                    .with_repo_prompt(
                        &ws.id,
                        crate::settings::PromptKind::ResolveConflicts,
                        message,
                    )
                    .await;
                self.send_message(&session.id, message).await.map(|_| ())
            }
            Action::FixChecks => {
                handled.checks_sha = Some(listed.head_sha.clone());
                self.fix_checks(&ws.id, &session.id).await.map(|_| ())
            }
            Action::AddressFeedback => {
                handled.feedback = listed.feedback;
                self.address_pr_comments(&ws.id, &session.id)
                    .await
                    .map(|_| ())
            }
        };
        // Recorded either way: a follow-up that can't be sent shouldn't be
        // retried every 90 seconds.
        if sent.is_ok() {
            handled.rounds += 1;
        }
        self.save_pr_handled(&ws.id, handled).await;
        sent.map_err(|e| anyhow::anyhow!("{e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn pr(state: &str, failing: u32, pending: u32) -> PullRequest {
        PullRequest {
            number: 7,
            url: "u".into(),
            title: "t".into(),
            state: state.into(),
            is_draft: false,
            review_decision: None,
            checks_passing: 1,
            checks_failing: failing,
            checks_pending: pending,
        }
    }

    fn listed(pr: PullRequest, mergeable: Option<bool>, sha: &str, feedback: u32) -> Listed {
        Listed {
            branch: "b".into(),
            pr,
            mergeable,
            head_sha: sha.into(),
            updated_at: None,
            feedback,
        }
    }

    #[test]
    fn picks_the_open_pr_and_counts_others_feedback() {
        let list = json!([
            {"number": 1, "url": "u1", "title": "old", "state": "MERGED", "headRefName": "feat",
             "updatedAt": "2026-11-18T10:00:00Z", "author": {"login": "me"}, "statusCheckRollup": []},
            {"number": 2, "url": "u2", "title": "new", "state": "OPEN", "headRefName": "feat",
             "updatedAt": "2026-11-17T10:00:00Z", "author": {"login": "me"}, "mergeable": "CONFLICTING",
             "headRefOid": "abc",
             "statusCheckRollup": [{"conclusion": "FAILURE"}, {"conclusion": "SUCCESS"}],
             "comments": [{"author": {"login": "me"}}, {"author": {"login": "ana"}}],
             "reviews": [
                {"author": {"login": "ana"}, "state": "CHANGES_REQUESTED", "body": ""},
                {"author": {"login": "ben"}, "state": "COMMENTED", "body": "  "},
                {"author": {"login": "ben"}, "state": "APPROVED", "body": "lgtm"}
             ]},
            {"number": 3, "url": "u3", "title": "other", "state": "OPEN", "headRefName": "other",
             "author": {"login": "me"}, "mergeable": "UNKNOWN", "statusCheckRollup": []}
        ]);
        let prs = parse_pr_list(&list);
        let feat = &prs["feat"];
        assert_eq!(feat.pr.number, 2, "an open PR wins over a newer merged one");
        assert_eq!(feat.mergeable, Some(false));
        assert_eq!(feat.head_sha, "abc");
        assert_eq!(feat.pr.checks_failing, 1);
        assert_eq!(
            feat.feedback, 2,
            "ana's comment and her review; not mine, not an empty one"
        );
        assert_eq!(prs["other"].mergeable, None);
    }

    #[test]
    fn decides_what_to_send() {
        let none = Handled::default();
        // Conflicts first, once per commit.
        let conflicted = listed(pr("OPEN", 1, 0), Some(false), "a", 0);
        assert_eq!(
            next_action(&none, &conflicted),
            Some(Action::ResolveConflict)
        );
        let after = Handled {
            conflict_sha: Some("a".into()),
            ..Default::default()
        };
        assert_eq!(next_action(&after, &conflicted), Some(Action::FixChecks));
        // CI only once it's finished, and once per commit.
        assert_eq!(
            next_action(&none, &listed(pr("OPEN", 1, 2), Some(true), "a", 0)),
            None
        );
        let failing = listed(pr("OPEN", 1, 0), Some(true), "a", 0);
        assert_eq!(next_action(&none, &failing), Some(Action::FixChecks));
        let fixed_a = Handled {
            checks_sha: Some("a".into()),
            ..Default::default()
        };
        assert_eq!(next_action(&fixed_a, &failing), None);
        assert_eq!(
            next_action(&fixed_a, &listed(pr("OPEN", 1, 0), Some(true), "b", 0)),
            Some(Action::FixChecks),
            "a new commit that still fails gets another go"
        );
        // New feedback only.
        let reviewed = listed(pr("OPEN", 0, 0), Some(true), "a", 3);
        assert_eq!(next_action(&none, &reviewed), Some(Action::AddressFeedback));
        let addressed = Handled {
            feedback: 3,
            ..Default::default()
        };
        assert_eq!(next_action(&addressed, &reviewed), None);
        // Nothing for drafts, merged or closed PRs.
        let mut draft = pr("OPEN", 1, 0);
        draft.is_draft = true;
        assert_eq!(
            next_action(&none, &listed(draft, Some(false), "a", 5)),
            None
        );
        assert_eq!(
            next_action(&none, &listed(pr("MERGED", 1, 0), Some(false), "a", 5)),
            None
        );
    }

    #[test]
    fn conflict_message_names_the_branches() {
        let msg = conflict_message(&pr("OPEN", 0, 0), "bantu/x", "origin/main");
        assert!(
            msg.contains("`bantu/x` conflicts with `origin/main`"),
            "{msg}"
        );
    }
}
