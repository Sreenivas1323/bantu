//! Reviewing a workspace: its diff against the base, line comments that get
//! sent to the agent, and the pull request on GitHub.

use std::path::Path;
use std::sync::Arc;

use bantu_git as git;
use bantu_protocol::{
    AgentKind, DiffComment, FileDiff, FileStatus, PermissionMode, PullRequest, Session,
    WorkspaceDiff,
};
use serde_json::Value;

use crate::ops::{new_id, CommandError, Daemon};
use crate::store::now;

/// Per-file patch size beyond which the UI gets a truncated patch.
const MAX_PATCH_BYTES: usize = 200_000;

type CmdResult<T> = Result<T, CommandError>;

impl Daemon {
    pub async fn workspace_diff(&self, workspace_id: &str) -> CmdResult<WorkspaceDiff> {
        let ws = self.require_workspace(workspace_id).await?;
        let path = Path::new(&ws.path);
        if !path.is_dir() {
            return Err(CommandError::Conflict("workspace has no worktree".into()));
        }
        let merge_base = git::merge_base(path, "HEAD", &ws.base_ref)
            .await
            .map_err(anyhow::Error::from)?;
        let (ahead, behind) = git::ahead_behind(path, &ws.base_ref)
            .await
            .map_err(anyhow::Error::from)?;
        let uncommitted = git::uncommitted_count(path)
            .await
            .map_err(anyhow::Error::from)?;
        let raw = git::diff_worktree(path, &merge_base)
            .await
            .map_err(anyhow::Error::from)?;
        Ok(WorkspaceDiff {
            workspace_id: ws.id,
            base_ref: ws.base_ref,
            merge_base,
            ahead,
            behind,
            uncommitted,
            files: parse_diff(&raw),
        })
    }

    pub async fn add_comment(
        &self,
        workspace_id: &str,
        path: String,
        line: u32,
        body: String,
    ) -> CmdResult<DiffComment> {
        self.require_workspace(workspace_id).await?;
        let body = body.trim().to_string();
        if body.is_empty() || path.trim().is_empty() {
            return Err(CommandError::BadRequest(
                "comments need a path and some text".into(),
            ));
        }
        let c = DiffComment {
            id: new_id(),
            workspace_id: workspace_id.to_string(),
            path,
            line,
            body,
            created_at: now(),
            sent_at: None,
        };
        self.store.upsert_comment(&c).await?;
        Ok(c)
    }

    pub async fn delete_comment(&self, comment_id: &str) -> CmdResult<()> {
        let c = self
            .store
            .comment(comment_id)
            .await?
            .ok_or_else(|| CommandError::NotFound(format!("no comment {comment_id}")))?;
        self.store.delete_comment(&c).await?;
        Ok(())
    }

    /// Bundles every draft comment into one message to the agent.
    pub async fn send_comments(
        self: &Arc<Self>,
        workspace_id: &str,
        session_id: &str,
    ) -> CmdResult<Session> {
        let mut drafts: Vec<DiffComment> = self
            .store
            .comments()
            .await?
            .into_iter()
            .filter(|c| c.workspace_id == workspace_id && c.sent_at.is_none())
            .collect();
        if drafts.is_empty() {
            return Err(CommandError::BadRequest(
                "there are no draft comments".into(),
            ));
        }
        drafts.sort_by(|a, b| (&a.path, a.line).cmp(&(&b.path, b.line)));
        let session = self
            .send_message(session_id, review_message(&drafts))
            .await?;
        let sent_at = now();
        for mut c in drafts {
            c.sent_at = Some(sent_at.clone());
            self.store.upsert_comment(&c).await?;
        }
        Ok(session)
    }

    pub async fn review_workspace(
        self: &Arc<Self>,
        workspace_id: &str,
        agent: Option<AgentKind>,
    ) -> CmdResult<Session> {
        let ws = self.require_workspace(workspace_id).await?;
        let merge_base = git::merge_base(Path::new(&ws.path), "HEAD", &ws.base_ref)
            .await
            .map_err(|_| {
                CommandError::Conflict("couldn't find where this branch started".into())
            })?;
        let session = self
            .create_session(workspace_id, agent, None, Some(PermissionMode::Plan))
            .await?;
        let session = self
            .update_session(&session.id, Some("Review".into()), None, None)
            .await?;
        self.send_message(
            &session.id,
            review_prompt(&ws.branch, &ws.base_ref, &merge_base),
        )
        .await
    }

    pub async fn create_pr(
        self: &Arc<Self>,
        workspace_id: &str,
        session_id: &str,
    ) -> CmdResult<Session> {
        let ws = self.require_workspace(workspace_id).await?;
        let repo = self
            .store
            .repo(&ws.repo_id)
            .await?
            .ok_or_else(|| CommandError::NotFound(format!("no repository {}", ws.repo_id)))?;
        if repo.remote_url.is_none() {
            return Err(CommandError::BadRequest(
                "this repository has no `origin` remote to open a pull request against".into(),
            ));
        }
        let target = ws.base_ref.strip_prefix("origin/").unwrap_or(&ws.base_ref);
        let prompt = format!(
            "Open a pull request for the work in this workspace.\n\n\
             1. Review `git status` and `git diff`. Commit any uncommitted work with clear, \
             conventional commit messages (don't commit secrets or build output).\n\
             2. Push the branch `{branch}` to origin.\n\
             3. Create the PR with `gh pr create --base {target} --head {branch}`. Write a concise \
             title and a body that explains what changed and why, plus how it was tested.\n\
             4. Reply with the PR URL.\n\n\
             If a PR for this branch already exists, push the new commits and reply with its URL.",
            branch = ws.branch,
        );
        self.send_message(session_id, prompt).await
    }

    pub async fn fix_checks(
        self: &Arc<Self>,
        workspace_id: &str,
        session_id: &str,
    ) -> CmdResult<Session> {
        let ws = self.require_workspace(workspace_id).await?;
        let repo = self
            .store
            .repo(&ws.repo_id)
            .await?
            .ok_or_else(|| CommandError::NotFound(format!("no repository {}", ws.repo_id)))?;
        let out = gh(
            &repo.root_path,
            &[
                "pr",
                "checks",
                &ws.branch,
                "--json",
                "name,bucket,link,workflow,description",
            ],
        )
        .await;
        // `gh pr checks` exits 8 while checks are pending but still prints JSON.
        let checks: Vec<Value> = out
            .as_deref()
            .and_then(|o| serde_json::from_str(o).ok())
            .ok_or_else(|| {
                CommandError::BadRequest("this branch has no pull request with checks".into())
            })?;
        let failing: Vec<&Value> = checks.iter().filter(|c| c["bucket"] == "fail").collect();
        if failing.is_empty() {
            return Err(CommandError::BadRequest("no checks are failing".into()));
        }
        let mut sections = Vec::new();
        for check in failing.iter().take(MAX_CHECKS_WITH_LOGS) {
            let log = match actions_job(check["link"].as_str().unwrap_or("")) {
                Some((run, job)) => gh(
                    &repo.root_path,
                    &["run", "view", &run, "--job", &job, "--log-failed"],
                )
                .await
                .map(|l| tail_lines(&clean_actions_log(&l), LOG_TAIL_LINES)),
                None => None,
            };
            sections.push(FailingCheck {
                name: check["name"].as_str().unwrap_or("check").to_string(),
                workflow: check["workflow"].as_str().unwrap_or("").to_string(),
                link: check["link"].as_str().unwrap_or("").to_string(),
                description: check["description"].as_str().unwrap_or("").to_string(),
                log,
            });
        }
        let more = failing.len().saturating_sub(MAX_CHECKS_WITH_LOGS);
        self.send_message(session_id, fix_checks_message(&ws.branch, &sections, more))
            .await
    }

    /// The open (or most recent) PR for the workspace's branch, if any.
    pub async fn pull_request(&self, workspace_id: &str) -> CmdResult<Option<PullRequest>> {
        let ws = self.require_workspace(workspace_id).await?;
        let repo = self
            .store
            .repo(&ws.repo_id)
            .await?
            .ok_or_else(|| CommandError::NotFound(format!("no repository {}", ws.repo_id)))?;
        if repo.remote_url.is_none() {
            return Ok(None);
        }
        let out = tokio::process::Command::new("gh")
            .args([
                "pr",
                "view",
                &ws.branch,
                "--json",
                "number,url,title,state,isDraft,reviewDecision,statusCheckRollup",
            ])
            .current_dir(&repo.root_path)
            .env("GH_PROMPT_DISABLED", "1")
            .output()
            .await;
        let out = match out {
            Ok(o) => o,
            // No gh installed: PR status just isn't available.
            Err(_) => return Ok(None),
        };
        if !out.status.success() {
            return Ok(None);
        }
        let v: Value = serde_json::from_slice(&out.stdout).map_err(anyhow::Error::from)?;
        Ok(parse_pr(&v))
    }
}

const MAX_CHECKS_WITH_LOGS: usize = 5;
const LOG_TAIL_LINES: usize = 120;

struct FailingCheck {
    name: String,
    workflow: String,
    link: String,
    description: String,
    log: Option<String>,
}

/// Runs `gh` in `dir`; `None` if it's missing or produced nothing usable.
async fn gh(dir: &str, args: &[&str]) -> Option<String> {
    let out = tokio::process::Command::new("gh")
        .args(args)
        .current_dir(dir)
        .env("GH_PROMPT_DISABLED", "1")
        .output()
        .await
        .ok()?;
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    (!stdout.trim().is_empty()).then_some(stdout)
}

/// `(run_id, job_id)` from a GitHub Actions job URL.
fn actions_job(link: &str) -> Option<(String, String)> {
    let rest = link.split("/actions/runs/").nth(1)?;
    let mut parts = rest.split('/');
    let run = parts.next()?.to_string();
    (parts.next()? == "job").then_some(())?;
    let job = parts.next()?.split(['?', '#']).next()?.to_string();
    Some((run, job))
}

/// `gh run view --log-failed` prefixes every line with job, step and a
/// timestamp, and keeps ANSI colors; the agent only needs the message.
fn clean_actions_log(raw: &str) -> String {
    raw.lines()
        .map(|line| {
            let msg = line.splitn(3, '\t').nth(2).unwrap_or(line);
            // "2026-11-02T06:00:34.6455480Z message"
            let msg = match msg.split_once(' ') {
                Some((ts, rest))
                    if ts.len() > 20 && ts.ends_with('Z') && ts.as_bytes()[4] == b'-' =>
                {
                    rest
                }
                _ => msg,
            };
            strip_ansi(msg)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' && chars.peek() == Some(&'[') {
            chars.next();
            // Skip parameters until the final byte (a letter).
            for c in chars.by_ref() {
                if c.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn tail_lines(text: &str, n: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(n)..].join("\n")
}

fn fix_checks_message(branch: &str, checks: &[FailingCheck], more: usize) -> String {
    let mut msg = format!(
        "CI checks are failing on the pull request for `{branch}`. Find the cause, fix it, \
         run the relevant checks locally if you can, then commit and push.\n"
    );
    for c in checks {
        let title = if c.workflow.is_empty() {
            c.name.clone()
        } else {
            format!("{} / {}", c.workflow, c.name)
        };
        msg.push_str(&format!("\n### {title}\n{}\n", c.link));
        if !c.description.is_empty() {
            msg.push_str(&format!("{}\n", c.description));
        }
        match &c.log {
            Some(log) => msg.push_str(&format!("\nFailed steps (last lines):\n```\n{log}\n```\n")),
            None => msg.push_str("(No log available; open the link above.)\n"),
        }
    }
    if more > 0 {
        msg.push_str(&format!("\n…and {more} more failing check(s) not shown.\n"));
    }
    msg
}

fn review_prompt(branch: &str, base: &str, merge_base: &str) -> String {
    format!(
        "Review the changes on `{branch}` against `{base}`: everything since `{merge_base}`, \
         including uncommitted work (`git diff {merge_base}` plus untracked files from \
         `git status`). Read the surrounding code where needed. Don't change any files.\n\n\
         Report only real problems: bugs, edge cases that break, security issues, data loss, \
         missing error handling, and tests that don't test what they claim. Skip style nits.\n\n\
         Format: a numbered list, most severe first, each as `path:line` — what's wrong — \
         how to fix it. If you find nothing significant, say so in one line."
    )
}

fn review_message(comments: &[DiffComment]) -> String {
    let mut msg = String::from(
        "Please address these review comments on the current changes. \
         Line numbers refer to the files as they are now.\n",
    );
    for c in comments {
        msg.push_str(&format!(
            "\n- `{}:{}` — {}",
            c.path,
            c.line,
            c.body.replace('\n', "\n  ")
        ));
    }
    msg
}

fn parse_pr(v: &Value) -> Option<PullRequest> {
    let (mut passing, mut failing, mut pending) = (0, 0, 0);
    for check in v["statusCheckRollup"].as_array().into_iter().flatten() {
        // CheckRun has status/conclusion; StatusContext has state.
        let verdict = check["conclusion"]
            .as_str()
            .filter(|s| !s.is_empty())
            .or_else(|| check["state"].as_str())
            .unwrap_or("PENDING");
        match verdict {
            "SUCCESS" | "NEUTRAL" | "SKIPPED" => passing += 1,
            "FAILURE" | "ERROR" | "CANCELLED" | "TIMED_OUT" | "ACTION_REQUIRED"
            | "STARTUP_FAILURE" => failing += 1,
            _ => pending += 1,
        }
    }
    Some(PullRequest {
        number: v["number"].as_u64()? as u32,
        url: v["url"].as_str()?.to_string(),
        title: v["title"].as_str().unwrap_or_default().to_string(),
        state: v["state"].as_str().unwrap_or("OPEN").to_string(),
        is_draft: v["isDraft"].as_bool().unwrap_or(false),
        review_decision: v["reviewDecision"]
            .as_str()
            .filter(|s| !s.is_empty())
            .map(str::to_string),
        checks_passing: passing,
        checks_failing: failing,
        checks_pending: pending,
    })
}

/// Splits `git diff` output into per-file patches.
pub fn parse_diff(raw: &str) -> Vec<FileDiff> {
    let mut files = Vec::new();
    for chunk in raw.split("\ndiff --git ").filter(|c| !c.trim().is_empty()) {
        let chunk = chunk.strip_prefix("diff --git ").unwrap_or(chunk);
        let (header, patch) = match chunk.find("\n@@") {
            Some(i) => (&chunk[..i], &chunk[i + 1..]),
            None => (chunk, ""),
        };
        let mut status = FileStatus::Modified;
        let (mut path, mut old_path) = (None, None);
        let mut binary = false;
        for line in header.lines() {
            if line.starts_with("new file mode") {
                status = FileStatus::Added;
            } else if line.starts_with("deleted file mode") {
                status = FileStatus::Deleted;
            } else if let Some(p) = line.strip_prefix("rename from ") {
                status = FileStatus::Renamed;
                old_path = Some(p.to_string());
            } else if let Some(p) = line.strip_prefix("rename to ") {
                path = Some(p.to_string());
            } else if let Some(p) = line.strip_prefix("+++ b/") {
                path = Some(p.to_string());
            } else if let Some(p) = line.strip_prefix("--- a/") {
                path.get_or_insert_with(|| p.to_string());
            } else if line.starts_with("Binary files ") {
                binary = true;
            }
        }
        // Binary/mode-only changes have no ---/+++ lines; use the header.
        let path = path.unwrap_or_else(|| header_path(header.lines().next().unwrap_or("")));
        let (mut additions, mut deletions) = (0, 0);
        for line in patch.lines() {
            if line.starts_with('+') {
                additions += 1;
            } else if line.starts_with('-') {
                deletions += 1;
            }
        }
        let truncated = patch.len() > MAX_PATCH_BYTES;
        let patch = if truncated {
            let mut end = MAX_PATCH_BYTES;
            while !patch.is_char_boundary(end) {
                end -= 1;
            }
            let cut = patch[..end].rfind('\n').unwrap_or(end);
            patch[..cut].to_string()
        } else {
            patch.trim_end_matches('\n').to_string()
        };
        files.push(FileDiff {
            path,
            old_path,
            status,
            additions,
            deletions,
            binary,
            patch,
            truncated,
        });
    }
    files
}

/// `a/foo b/foo` → `foo` (good enough for the paths git doesn't quote).
fn header_path(first_line: &str) -> String {
    first_line
        .rsplit_once(" b/")
        .map(|(_, p)| p.to_string())
        .unwrap_or_else(|| first_line.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const DIFF: &str = "diff --git a/src/a.rs b/src/a.rs
index 1111111..2222222 100644
--- a/src/a.rs
+++ b/src/a.rs
@@ -1,3 +1,3 @@
 fn a() {}
-fn b() {}
+fn b() { 1 }
+fn c() {}
diff --git a/new.txt b/new.txt
new file mode 100644
index 0000000..3333333
--- /dev/null
+++ b/new.txt
@@ -0,0 +1 @@
+hello
diff --git a/old.md b/docs/old.md
similarity index 100%
rename from old.md
rename to docs/old.md
diff --git a/logo.png b/logo.png
new file mode 100644
index 0000000..4444444
Binary files /dev/null and b/logo.png differ
";

    #[test]
    fn parses_git_diff() {
        let files = parse_diff(DIFF);
        assert_eq!(files.len(), 4);
        assert_eq!(files[0].path, "src/a.rs");
        assert_eq!((files[0].additions, files[0].deletions), (2, 1));
        assert!(files[0].patch.starts_with("@@ -1,3 +1,3 @@"));
        assert_eq!(files[1].status, FileStatus::Added);
        assert_eq!(files[1].path, "new.txt");
        assert_eq!(files[2].status, FileStatus::Renamed);
        assert_eq!(files[2].old_path.as_deref(), Some("old.md"));
        assert_eq!(files[2].path, "docs/old.md");
        assert!(files[3].binary);
        assert_eq!(files[3].path, "logo.png");
    }

    #[test]
    fn summarizes_pr_checks() {
        let v = json!({"number": 7, "url": "https://x/7", "title": "T", "state": "OPEN",
            "isDraft": false, "reviewDecision": "",
            "statusCheckRollup": [
                {"status": "COMPLETED", "conclusion": "SUCCESS"},
                {"status": "COMPLETED", "conclusion": "FAILURE"},
                {"status": "IN_PROGRESS", "conclusion": ""},
                {"state": "SUCCESS"}]});
        let pr = parse_pr(&v).unwrap();
        assert_eq!(
            (pr.checks_passing, pr.checks_failing, pr.checks_pending),
            (2, 1, 1)
        );
        assert_eq!(pr.review_decision, None);
    }

    #[test]
    fn finds_actions_jobs_and_builds_fix_prompt() {
        assert_eq!(
            actions_job("https://github.com/o/r/actions/runs/123/job/456?pr=7"),
            Some(("123".into(), "456".into()))
        );
        assert_eq!(actions_job("https://vercel.com/o/r/deployments/x"), None);
        let msg = fix_checks_message(
            "bantu/fix-login",
            &[FailingCheck {
                name: "Rust (macos-latest)".into(),
                workflow: "CI".into(),
                link: "https://x/1".into(),
                description: String::new(),
                log: Some("error: test failed".into()),
            }],
            2,
        );
        assert!(msg.contains("`bantu/fix-login`"));
        assert!(msg.contains("### CI / Rust (macos-latest)"));
        assert!(msg.contains("```\nerror: test failed\n```"));
        assert!(msg.contains("and 2 more failing check(s)"));
        assert_eq!(tail_lines("a\nb\nc", 2), "b\nc");
        let raw = "Rust (macos)\tRun cargo test\t2026-11-02T06:00:34.6346140Z \u{1b}[1m\u{1b}[91merror\u{1b}[0m: test failed\n\
                   Rust (macos)\tRun cargo test\t2026-11-02T06:00:34.6455480Z ##[error]Process completed";
        assert_eq!(
            clean_actions_log(raw),
            "error: test failed\n##[error]Process completed"
        );
    }

    #[test]
    fn review_message_lists_comments() {
        let c = |path: &str, line, body: &str| DiffComment {
            id: "x".into(),
            workspace_id: "w".into(),
            path: path.into(),
            line,
            body: body.into(),
            created_at: "t".into(),
            sent_at: None,
        };
        let msg = review_message(&[c("a.rs", 3, "rename this"), c("b.rs", 9, "two\nlines")]);
        assert!(msg.contains("- `a.rs:3` — rename this"));
        assert!(msg.contains("- `b.rs:9` — two\n  lines"));
    }
}
