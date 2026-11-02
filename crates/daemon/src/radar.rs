//! Conflict radar: tells you two workspaces (or a workspace and its base)
//! will conflict before either opens a PR.
//!
//! Each ready workspace is snapshotted (uncommitted work included, nothing
//! touched) and every pair is test-merged with `git merge-tree`. All local,
//! so it's free; it reruns when agents finish turns and workspaces change.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use bantu_git as git;
use bantu_protocol::{EventData, WorkspaceConflict, WorkspaceState};
use tokio::sync::Mutex;

use crate::ops::Daemon;

#[derive(Default)]
pub struct Radar {
    /// Latest result per repo.
    results: Mutex<HashMap<String, Vec<WorkspaceConflict>>>,
    /// One scan per repo at a time; extra requests coalesce into one rerun.
    scanning: Mutex<HashMap<String, bool>>,
}

impl Daemon {
    pub async fn all_conflicts(&self) -> Vec<WorkspaceConflict> {
        let results = self.radar.results.lock().await;
        let mut all: Vec<_> = results.values().flatten().cloned().collect();
        all.sort_by(|a, b| (&a.repo_id, &a.workspace_id).cmp(&(&b.repo_id, &b.workspace_id)));
        all
    }

    /// Schedules a scan of `repo_id`. Cheap to call often.
    pub fn rescan_conflicts(self: &Arc<Self>, repo_id: &str) {
        let this = self.clone();
        let repo_id = repo_id.to_string();
        tokio::spawn(async move {
            {
                let mut scanning = this.radar.scanning.lock().await;
                if let Some(pending) = scanning.get_mut(&repo_id) {
                    // A scan is running; ask it to go again when done.
                    *pending = true;
                    return;
                }
                scanning.insert(repo_id.clone(), false);
            }
            loop {
                if let Err(e) = this.scan_conflicts(&repo_id).await {
                    tracing::warn!("conflict scan for {repo_id} failed: {e:#}");
                }
                let mut scanning = this.radar.scanning.lock().await;
                if scanning.get(&repo_id) == Some(&true) {
                    scanning.insert(repo_id.clone(), false);
                } else {
                    scanning.remove(&repo_id);
                    return;
                }
            }
        });
    }

    pub async fn scan_conflicts(&self, repo_id: &str) -> anyhow::Result<Vec<WorkspaceConflict>> {
        let Some(repo) = self.store.repo(repo_id).await? else {
            return Ok(vec![]);
        };
        let root = Path::new(&repo.root_path);
        let workspaces: Vec<_> = self
            .store
            .workspaces()
            .await?
            .into_iter()
            .filter(|w| w.repo_id == repo_id && w.state == WorkspaceState::Ready)
            .filter(|w| Path::new(&w.path).is_dir())
            .collect();

        let mut snaps = Vec::new();
        for ws in &workspaces {
            // A workspace mid-merge can't be snapshotted meaningfully; skip it.
            match git::snapshot(Path::new(&ws.path)).await {
                Ok(sha) => snaps.push((ws, sha)),
                Err(e) => tracing::debug!("skipping {} in conflict scan: {e}", ws.name),
            }
        }

        let mut conflicts = Vec::new();
        for (i, (a, sa)) in snaps.iter().enumerate() {
            if git::ref_exists(root, &a.base_ref).await? {
                let files = git::merge_conflicts(root, sa, &a.base_ref).await?;
                if !files.is_empty() {
                    conflicts.push(WorkspaceConflict {
                        repo_id: repo_id.to_string(),
                        workspace_id: a.id.clone(),
                        other_workspace_id: None,
                        base_ref: Some(a.base_ref.clone()),
                        files,
                    });
                }
            }
            for (b, sb) in &snaps[i + 1..] {
                let files = git::merge_conflicts(root, sa, sb).await?;
                if !files.is_empty() {
                    conflicts.push(WorkspaceConflict {
                        repo_id: repo_id.to_string(),
                        workspace_id: a.id.clone(),
                        other_workspace_id: Some(b.id.clone()),
                        base_ref: None,
                        files,
                    });
                }
            }
        }

        let changed = {
            let mut results = self.radar.results.lock().await;
            let previous = results.get(repo_id).cloned().unwrap_or_default();
            results.insert(repo_id.to_string(), conflicts.clone());
            previous != conflicts
        };
        if changed {
            self.store
                .emit(EventData::ConflictsUpdated {
                    repo_id: repo_id.to_string(),
                    conflicts: conflicts.clone(),
                })
                .await?;
        }
        Ok(conflicts)
    }
}
