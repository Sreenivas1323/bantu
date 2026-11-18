//! Organization membership through Bantu Connect.
//!
//! A member pastes a join link (`https://<connector>/c/<code>`); the daemon
//! trades the code for a daemon token, then syncs the org's settings every
//! few minutes:
//!
//! - **Connections** (Jira, ClickUp, GitLab) become trackers marked with the
//!   org's id. Each one attaches to the local repos its scope obviously points
//!   at (see [`repos_for`]), or to every repo when nothing matches. Members
//!   can't remove them; they go when the admin drops them or the member leaves.
//! - **Claude access**: in any mode but `own`, the org's `claude.env` is set on
//!   every Claude Code process Bantu starts (chats, reviews and terminals).
//! - **Team totals**: once a day, yesterday's counts (never names, branches or
//!   text) are posted to the connector.
//!
//! The daemon token lives in `$BANTU_HOME/org.token` and the Claude settings
//! in `$BANTU_HOME/org-claude-env.json`, both readable only by the user and
//! never in the event log, the snapshot or any response. A 401 from the
//! connector means this machine was removed: everything the org added goes.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use anyhow::Context;
use bantu_protocol::{ClaudeAccessMode, Org, OrgClaude, OrgMember, Repo, Tracker, TrackerKind};
use serde::Deserialize;
use serde_json::json;
use tokio::sync::Mutex;

use crate::ops::{CommandError, Daemon};
use crate::store::now;
use crate::tasks::{api_error, http_status, valid_site};

const SYNC_EVERY: Duration = Duration::from_secs(300);
const TOTALS_SENT_KEY: &str = "org-totals-day";

/// Only variables that configure Claude Code, its providers and telemetry
/// are passed on, so an org's settings can't change `PATH`, preload
/// libraries or redirect Claude's own config.
const ENV_PREFIXES: &[&str] = &[
    "ANTHROPIC_",
    "CLAUDE_CODE_",
    "AWS_",
    "CLOUD_ML_",
    "GOOGLE_",
    "VERTEX_REGION_",
    "DISABLE_",
    "OTEL_",
];

type CmdResult<T> = Result<T, CommandError>;

#[derive(Default)]
pub struct OrgState {
    /// Serializes joining, syncing and leaving.
    lock: Mutex<()>,
    /// What to set on Claude Code processes; empty in `own` mode.
    claude_env: RwLock<Vec<(String, String)>>,
}

// ---- Join links ------------------------------------------------------------------

#[derive(Debug, PartialEq, Eq)]
pub struct JoinLink {
    /// The connector's address, e.g. `https://connect.example.com`.
    pub base: String,
    pub code: String,
}

/// Reads `https://<connector>/c/<code>`. The connector may sit under a path
/// (`https://example.com/connect/c/<code>`). Plain HTTP is only accepted to
/// `127.0.0.1`, for testing.
pub fn parse_join_link(link: &str) -> Result<JoinLink, &'static str> {
    const NOT_A_LINK: &str =
        "that isn't a Bantu Connect join link; it looks like https://connect.example.com/c/ABC123";
    let link = link.trim().trim_end_matches('/');
    let (scheme, rest) = link.split_once("://").ok_or(NOT_A_LINK)?;
    if rest.contains(['?', '#', '@', '\\']) || rest.chars().any(char::is_whitespace) {
        return Err(NOT_A_LINK);
    }
    let (base, code) = rest.rsplit_once("/c/").ok_or(NOT_A_LINK)?;
    let valid_code = !code.is_empty()
        && code.len() <= 128
        && code
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'));
    let authority = base.split('/').next().unwrap_or_default();
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) => (host, Some(port)),
        None => (authority, None),
    };
    let valid_host = !host.is_empty()
        && host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.'))
        && port.is_none_or(|p| !p.is_empty() && p.parse::<u16>().is_ok());
    if !valid_code
        || !valid_host
        || base
            .split('/')
            .skip(1)
            .any(|seg| seg.is_empty() || seg == "." || seg == "..")
    {
        return Err(NOT_A_LINK);
    }
    match scheme {
        "https" => {}
        "http" if host == "127.0.0.1" => {}
        _ => return Err("join links must use https://"),
    }
    Ok(JoinLink {
        base: format!("{scheme}://{base}"),
        code: code.to_string(),
    })
}

// ---- The connector's API -----------------------------------------------------------

#[derive(Deserialize)]
struct OrgWire {
    id: String,
    name: String,
    #[serde(default)]
    slug: String,
}

#[derive(Deserialize)]
struct JoinWire {
    daemon_token: String,
    org: OrgWire,
    member: OrgMember,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Connection {
    pub id: String,
    pub kind: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub site: Option<String>,
    #[serde(default)]
    pub scope: String,
    #[serde(default)]
    pub account: Option<String>,
    #[serde(default)]
    pub token: Option<String>,
}

#[derive(Deserialize, Default)]
struct ClaudeWire {
    #[serde(default)]
    mode: ClaudeAccessMode,
    #[serde(default)]
    plan: Option<String>,
    #[serde(default)]
    env: BTreeMap<String, String>,
}

#[derive(Deserialize)]
struct SyncWire {
    org: OrgWire,
    member: OrgMember,
    #[serde(default)]
    connections: Vec<Connection>,
    #[serde(default)]
    claude: Option<ClaudeWire>,
}

fn bearer(token: &str) -> Vec<(&'static str, String)> {
    vec![
        ("Authorization", format!("Bearer {}", token.trim())),
        ("Accept", "application/json".into()),
    ]
}

/// This machine's name, as members see it in Connect.
fn machine_name() -> String {
    #[cfg(unix)]
    {
        let mut buf = [0u8; 256];
        // SAFETY: the buffer is valid for its length, and gethostname writes
        // at most that many bytes.
        let ok = unsafe { libc::gethostname(buf.as_mut_ptr().cast(), buf.len()) } == 0;
        if ok {
            let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
            let name = String::from_utf8_lossy(&buf[..end]).trim().to_string();
            if !name.is_empty() {
                return name;
            }
        }
    }
    ["COMPUTERNAME", "HOSTNAME"]
        .iter()
        .find_map(|v| std::env::var(v).ok().filter(|v| !v.trim().is_empty()))
        .unwrap_or_else(|| "bantu".into())
}

/// Claude settings worth passing on, dropping anything else.
fn claude_env(mode: ClaudeAccessMode, env: &BTreeMap<String, String>) -> Vec<(String, String)> {
    if mode == ClaudeAccessMode::Own {
        return Vec::new();
    }
    env.iter()
        .filter(|(k, v)| {
            let ok = !k.is_empty()
                && k.chars()
                    .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
                && ENV_PREFIXES.iter().any(|p| k.starts_with(p))
                && !v.contains('\0');
            if !ok {
                tracing::warn!("ignoring {k} from the organization's Claude settings");
            }
            ok
        })
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}

// ---- Which repos a connection belongs to -------------------------------------------

/// The repos an org connection attaches to. A repo matches when its folder
/// name is the connection's name, a Jira project key, or a GitLab project's
/// last path segment (case aside), or when its `origin` is that GitLab
/// project. With no match it goes to every repo, so nothing an admin connects
/// is hidden from the member.
pub fn repos_for<'a>(
    kind: TrackerKind,
    name: &str,
    scope: &str,
    repos: &'a [Repo],
) -> Vec<&'a Repo> {
    let mut names = vec![name.trim().to_lowercase()];
    let mut project = None;
    match kind {
        TrackerKind::Gitlab => {
            if let Some(s) = crate::tasks::gitlab_scope(scope) {
                if let Some(last) = s.project.rsplit('/').next() {
                    names.push(last.to_lowercase());
                }
                project = Some(s.project.to_lowercase());
            }
        }
        TrackerKind::Jira => {
            let key = scope.trim();
            if !key.is_empty() && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                names.push(key.to_lowercase());
            }
        }
        TrackerKind::Clickup | TrackerKind::Github => {}
    }
    names.retain(|n| !n.is_empty());
    let remote_is = |remote: &str, project: &str| {
        let remote = remote.trim().to_lowercase();
        let remote = remote.trim_end_matches('/').trim_end_matches(".git");
        remote.ends_with(&format!("/{project}")) || remote.ends_with(&format!(":{project}"))
    };
    let matched: Vec<&Repo> = repos
        .iter()
        .filter(|r| {
            names.contains(&r.name.to_lowercase())
                || project.as_deref().is_some_and(|p| {
                    !p.chars().all(|c| c.is_ascii_digit())
                        && r.remote_url.as_deref().is_some_and(|u| remote_is(u, p))
                })
        })
        .collect();
    if matched.is_empty() {
        repos.iter().collect()
    } else {
        matched
    }
}

/// `org-<connection>-<repo>`, if the connection's id is safe in a file name.
fn tracker_id(connection_id: &str, repo_id: &str) -> Option<String> {
    let ok = !connection_id.is_empty()
        && connection_id.len() <= 64
        && connection_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'));
    ok.then(|| format!("org-{connection_id}-{repo_id}"))
}

// ---- The daemon's side -------------------------------------------------------------

impl Daemon {
    fn org_token_path(&self) -> PathBuf {
        self.config.home.join("org.token")
    }

    fn org_env_path(&self) -> PathBuf {
        self.config.home.join("org-claude-env.json")
    }

    fn org_token(&self) -> Option<String> {
        std::fs::read_to_string(self.org_token_path())
            .ok()
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty())
    }

    /// What the org sets on Claude Code processes. Empty without an org, or
    /// in `own` mode.
    pub(crate) fn claude_env(&self) -> Vec<(String, String)> {
        self.org
            .claude_env
            .read()
            .map(|e| e.clone())
            .unwrap_or_default()
    }

    fn set_claude_env(&self, env: Vec<(String, String)>) -> anyhow::Result<()> {
        let path = self.org_env_path();
        if env.is_empty() {
            let _ = std::fs::remove_file(&path);
        } else {
            let map: BTreeMap<&str, &str> =
                env.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
            crate::write_private(&path, &serde_json::to_string(&map)?)?;
        }
        if let Ok(mut current) = self.org.claude_env.write() {
            *current = env;
        }
        Ok(())
    }

    pub async fn join_org(self: &Arc<Self>, link: &str) -> CmdResult<Org> {
        let link = parse_join_link(link).map_err(|e| CommandError::BadRequest(e.into()))?;
        let _guard = self.org.lock.lock().await;
        if let Some(org) = self.store.org().await? {
            return Err(CommandError::Conflict(format!(
                "this machine is already in {}; leave it first",
                org.name
            )));
        }
        let body = json!({"code": link.code, "name": machine_name()});
        let (status, value, raw) = http_status(
            "POST",
            &format!("{}/api/daemon/join", link.base),
            &[("Accept", "application/json".into())],
            Some(&body),
        )
        .await
        .map_err(|e| CommandError::BadRequest(format!("couldn't reach {}: {e:#}", link.base)))?;
        match status {
            200..=299 => {}
            400 | 404 => {
                return Err(CommandError::BadRequest(
                    "that join link isn't valid; ask an admin for a new one".into(),
                ))
            }
            410 => {
                return Err(CommandError::BadRequest(
                    "that join link has expired or was already used; ask an admin for a new one"
                        .into(),
                ))
            }
            other => {
                return Err(CommandError::BadRequest(format!(
                    "Bantu Connect answered HTTP {other}: {}",
                    api_error(&value, &raw)
                )))
            }
        }
        let joined: JoinWire = serde_json::from_value(value).map_err(|e| {
            CommandError::BadRequest(format!(
                "Bantu Connect sent a reply Bantu doesn't understand: {e}"
            ))
        })?;
        if joined.daemon_token.trim().is_empty() {
            return Err(CommandError::BadRequest(
                "Bantu Connect didn't send a token".into(),
            ));
        }
        crate::write_private(&self.org_token_path(), joined.daemon_token.trim())?;
        let org = Org {
            id: joined.org.id,
            name: joined.org.name,
            slug: joined.org.slug,
            member: joined.member,
            connector: link.base,
            joined_at: now(),
            last_synced_at: None,
            last_error: None,
            claude: OrgClaude::default(),
        };
        self.store.set_org(Some(&org), None).await?;
        match self.sync_org_locked().await? {
            Some(org) => Ok(org),
            None => Err(CommandError::Conflict(
                "Bantu Connect accepted the link but then refused this machine".into(),
            )),
        }
    }

    /// Fetches the org's settings now. `None` when there's no org, or the
    /// connector said this machine isn't a member any more.
    pub async fn sync_org(self: &Arc<Self>) -> CmdResult<Option<Org>> {
        let _guard = self.org.lock.lock().await;
        self.sync_org_locked().await
    }

    async fn sync_org_locked(self: &Arc<Self>) -> CmdResult<Option<Org>> {
        let Some(mut org) = self.store.org().await? else {
            return Ok(None);
        };
        let Some(token) = self.org_token() else {
            self.end_membership(Some(format!(
                "This machine's Bantu Connect token is missing, so it left {}. Join again with a new link.",
                org.name
            )))
            .await?;
            return Ok(None);
        };
        let result = http_status(
            "GET",
            &format!("{}/api/daemon/sync", org.connector),
            &bearer(&token),
            None,
        )
        .await;
        let error = match result {
            Err(e) => Some(format!("couldn't reach {}: {e:#}", org.connector)),
            Ok((401, _, _)) => {
                self.end_membership(Some(format!(
                    "{} removed this machine, or removed you from the organization, so its trackers \
                     and Claude settings were removed here. Ask an admin for a new join link to come back.",
                    org.name
                )))
                .await?;
                return Ok(None);
            }
            Ok((status, value, raw)) if !(200..300).contains(&status) => Some(format!(
                "Bantu Connect answered HTTP {status}: {}",
                api_error(&value, &raw)
            )),
            Ok((_, value, _)) => match serde_json::from_value::<SyncWire>(value) {
                Err(e) => Some(format!(
                    "Bantu Connect sent settings this Bantu doesn't understand: {e}"
                )),
                Ok(sync) => {
                    let claude = sync.claude.unwrap_or_default();
                    let env = claude_env(claude.mode, &claude.env);
                    org.claude = OrgClaude {
                        mode: claude.mode,
                        plan: claude.plan.filter(|p| !p.trim().is_empty()),
                        env_keys: env.iter().map(|(k, _)| k.clone()).collect(),
                    };
                    self.set_claude_env(env)?;
                    org.id = sync.org.id;
                    org.name = sync.org.name;
                    if !sync.org.slug.is_empty() {
                        org.slug = sync.org.slug;
                    }
                    org.member = sync.member;
                    self.apply_connections(&org.id, &sync.connections).await?;
                    org.last_synced_at = Some(now());
                    None
                }
            },
        };
        org.last_error = error;
        self.store.set_org(Some(&org), None).await?;
        if org.last_error.is_none() {
            self.send_totals(&org, &token).await;
        }
        Ok(Some(org))
    }

    /// Makes the org's trackers match its connections.
    async fn apply_connections(
        self: &Arc<Self>,
        org_id: &str,
        connections: &[Connection],
    ) -> CmdResult<()> {
        let repos = self.store.repos().await?;
        let existing: HashMap<String, Tracker> = self
            .store
            .trackers()
            .await?
            .into_iter()
            .filter(|t| t.org_id.is_some())
            .map(|t| (t.id.clone(), t))
            .collect();
        let mut keep = HashSet::new();
        let mut changed = Vec::new();
        for c in connections {
            let kind = match c.kind.as_str() {
                "jira" => TrackerKind::Jira,
                "clickup" => TrackerKind::Clickup,
                "gitlab" => TrackerKind::Gitlab,
                other => {
                    tracing::warn!("skipping the org's {other} connection {}", c.id);
                    continue;
                }
            };
            let clean = |s: &Option<String>| {
                s.as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(String::from)
            };
            let (site, account, token) = (clean(&c.site), clean(&c.account), clean(&c.token));
            if site.as_deref().is_some_and(|s| !valid_site(s)) {
                tracing::warn!(
                    "skipping the org's connection {}: its site isn't HTTPS",
                    c.id
                );
                continue;
            }
            let scope = c.scope.trim().to_string();
            let name = if c.name.trim().is_empty() {
                format!(
                    "{}: {}",
                    kind.label(),
                    scope.chars().take(40).collect::<String>()
                )
            } else {
                c.name.trim().to_string()
            };
            for repo in repos_for(kind, &name, &scope, &repos) {
                let Some(id) = tracker_id(&c.id, &repo.id) else {
                    tracing::warn!("skipping the org's connection {:?}: unusable id", c.id);
                    break;
                };
                keep.insert(id.clone());
                let prev = existing.get(&id);
                let tracker = Tracker {
                    id: id.clone(),
                    kind,
                    repo_id: repo.id.clone(),
                    name: name.clone(),
                    site: site.clone(),
                    scope: scope.clone(),
                    account: account.clone(),
                    created_at: prev.map(|t| t.created_at.clone()).unwrap_or_else(now),
                    last_synced_at: prev.and_then(|t| t.last_synced_at.clone()),
                    last_error: prev.and_then(|t| t.last_error.clone()),
                    org_id: Some(org_id.to_string()),
                };
                let stored = std::fs::read_to_string(self.token_path(&id)).ok();
                let token_changed = stored.as_deref().map(str::trim) != token.as_deref();
                match &token {
                    Some(token) if token_changed => self.save_token(&id, token)?,
                    None if token_changed => {
                        let _ = std::fs::remove_file(self.token_path(&id));
                    }
                    _ => {}
                }
                if prev != Some(&tracker) {
                    self.store.upsert_tracker(&tracker).await?;
                }
                let same_definition = prev.is_some_and(|p| {
                    (&p.kind, &p.site, &p.scope, &p.account)
                        == (
                            &tracker.kind,
                            &tracker.site,
                            &tracker.scope,
                            &tracker.account,
                        )
                });
                if token_changed || !same_definition {
                    changed.push(tracker);
                }
            }
        }
        for id in existing.keys().filter(|id| !keep.contains(*id)) {
            self.drop_tracker(id).await?;
        }
        // New or changed trackers fetch their tasks now rather than at the
        // next round of the task sync.
        if !changed.is_empty() {
            let this = self.clone();
            tokio::spawn(async move {
                for t in changed {
                    this.forget_tracker_account(&t.id).await;
                    if let Err(e) = this.sync_tracker(&t).await {
                        tracing::warn!("couldn't sync {}: {e}", t.name);
                    }
                }
            });
        }
        Ok(())
    }

    /// Leaves the org: tells the connector, then removes what it added.
    pub async fn leave_org(&self) -> CmdResult<()> {
        let _guard = self.org.lock.lock().await;
        let Some(org) = self.store.org().await? else {
            return Err(CommandError::BadRequest(
                "this machine isn't in an organization".into(),
            ));
        };
        if let Some(token) = self.org_token() {
            let url = format!("{}/api/daemon/leave", org.connector);
            match http_status("POST", &url, &bearer(&token), None).await {
                // 401: the connector already let this machine go.
                Ok((status, _, _)) if (200..300).contains(&status) || status == 401 => {}
                Ok((status, value, raw)) => tracing::warn!(
                    "Bantu Connect answered HTTP {status} to leaving: {}",
                    api_error(&value, &raw)
                ),
                Err(e) => tracing::warn!("couldn't tell Bantu Connect about leaving: {e:#}"),
            }
        }
        self.end_membership(None).await
    }

    /// Removes everything the org added, and records why if it wasn't the
    /// member's choice.
    async fn end_membership(&self, ended: Option<String>) -> CmdResult<()> {
        for t in self.store.trackers().await? {
            if t.org_id.is_some() {
                self.drop_tracker(&t.id).await?;
            }
        }
        let _ = std::fs::remove_file(self.org_token_path());
        self.set_claude_env(Vec::new())?;
        self.store.delete_private_value(TOTALS_SENT_KEY).await?;
        self.store.set_org(None, ended.as_deref()).await?;
        Ok(())
    }

    /// Posts yesterday's counts (UTC), once. Called after each good sync, so
    /// a day missed while the machine was off goes on the next start.
    async fn send_totals(&self, org: &Org, token: &str) {
        let yesterday = (chrono::Utc::now().date_naive() - chrono::Days::new(1))
            .format("%Y-%m-%d")
            .to_string();
        let sent = self
            .store
            .private_value(TOTALS_SENT_KEY)
            .await
            .ok()
            .flatten();
        if sent.is_some_and(|d| d >= yesterday) {
            return;
        }
        let result = async {
            let counts = self.store.day_counts(&yesterday).await?;
            let body = json!({"day": yesterday, "counts": counts});
            let url = format!("{}/api/daemon/totals", org.connector);
            let (status, value, raw) = http_status("POST", &url, &bearer(token), Some(&body))
                .await
                .context("couldn't send team totals")?;
            if !(200..300).contains(&status) {
                anyhow::bail!("HTTP {status}: {}", api_error(&value, &raw));
            }
            self.store
                .set_private_value(TOTALS_SENT_KEY, &yesterday)
                .await
        }
        .await;
        if let Err(e) = result {
            tracing::warn!("team totals for {yesterday}: {e:#}");
        }
    }

    /// Loads the org's Claude settings, then syncs every few minutes.
    pub fn start_org_sync(self: &Arc<Self>) {
        let saved: Vec<(String, String)> = std::fs::read_to_string(self.org_env_path())
            .ok()
            .and_then(|text| serde_json::from_str::<BTreeMap<String, String>>(&text).ok())
            .map(|m| m.into_iter().collect())
            .unwrap_or_default();
        if let Ok(mut env) = self.org.claude_env.write() {
            *env = saved;
        }
        let this = self.clone();
        tokio::spawn(async move {
            loop {
                if let Err(e) = this.sync_org().await {
                    tracing::warn!("couldn't sync the organization: {e}");
                }
                tokio::time::sleep(SYNC_EVERY).await;
            }
        });
    }

    /// Whether a sync is worth doing now, e.g. after a repo is added.
    pub(crate) async fn in_org(&self) -> bool {
        matches!(self.store.org().await, Ok(Some(_)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn link(base: &str, code: &str) -> Result<JoinLink, &'static str> {
        Ok(JoinLink {
            base: base.into(),
            code: code.into(),
        })
    }

    #[test]
    fn parses_join_links() {
        assert_eq!(
            parse_join_link("https://connect.acme.dev/c/AbC-123_x"),
            link("https://connect.acme.dev", "AbC-123_x")
        );
        assert_eq!(
            parse_join_link("  https://acme.dev:8443/connect/c/XYZ/ "),
            link("https://acme.dev:8443/connect", "XYZ")
        );
        assert_eq!(
            parse_join_link("http://127.0.0.1:4000/c/test"),
            link("http://127.0.0.1:4000", "test")
        );
        assert_eq!(
            parse_join_link("http://connect.acme.dev/c/ABC"),
            Err("join links must use https://")
        );
        assert_eq!(
            parse_join_link("http://localhost:4000/c/ABC"),
            Err("join links must use https://")
        );
        for bad in [
            "",
            "connect.acme.dev/c/ABC",
            "https://connect.acme.dev/ABC",
            "https://connect.acme.dev/c/",
            "https://connect.acme.dev/c/AB C",
            "https://connect.acme.dev/c/ABC?x=1",
            "https://connect.acme.dev/c/ABC#frag",
            "https://me@evil.example/c/ABC",
            "https://connect.acme.dev/c/../ABC",
            "https://connect.acme.dev:notaport/c/ABC",
            "https:///c/ABC",
            "https://a//c/ABC",
            "ftp://connect.acme.dev/c/ABC",
        ] {
            assert!(parse_join_link(bad).is_err(), "{bad:?}");
        }
    }

    fn repo(id: &str, name: &str, remote: Option<&str>) -> Repo {
        Repo {
            id: id.into(),
            name: name.into(),
            root_path: format!("/code/{name}"),
            default_branch: "main".into(),
            remote_url: remote.map(String::from),
            created_at: String::new(),
            github_account: None,
        }
    }

    #[test]
    fn attaches_connections_to_the_obvious_repo_or_every_repo() {
        let repos = vec![
            repo("r1", "web", Some("git@gitlab.com:acme/web.git")),
            repo("r2", "API", Some("https://github.com/acme/api")),
            repo(
                "r3",
                "site",
                Some("https://gitlab.example.com/acme/platform/marketing.git"),
            ),
        ];
        let ids = |kind, name: &str, scope: &str| -> Vec<String> {
            repos_for(kind, name, scope, &repos)
                .into_iter()
                .map(|r| r.id.clone())
                .collect()
        };
        assert_eq!(ids(TrackerKind::Jira, "Web tickets", "WEB"), ["r1"]);
        assert_eq!(ids(TrackerKind::Jira, "api", "project = X"), ["r2"]);
        assert_eq!(
            ids(TrackerKind::Gitlab, "Issues", "acme/web ~agent"),
            ["r1"]
        );
        assert_eq!(
            ids(TrackerKind::Gitlab, "Marketing", "acme/platform/marketing"),
            ["r3"],
            "by origin"
        );
        assert_eq!(
            ids(TrackerKind::Clickup, "Sprint", "900"),
            ["r1", "r2", "r3"]
        );
        assert_eq!(
            ids(TrackerKind::Gitlab, "Other", "1234"),
            ["r1", "r2", "r3"]
        );
        assert!(repos_for(TrackerKind::Jira, "x", "WEB", &[]).is_empty());
    }

    #[test]
    fn keeps_only_claude_settings() {
        let env: BTreeMap<String, String> = [
            ("ANTHROPIC_API_KEY", "sk-ant-1"),
            ("CLAUDE_CODE_USE_BEDROCK", "1"),
            ("AWS_REGION", "us-east-1"),
            ("PATH", "/evil"),
            ("LD_PRELOAD", "/evil.so"),
            ("anthropic_lower", "x"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
        let kept: Vec<String> = claude_env(ClaudeAccessMode::ApiKey, &env)
            .into_iter()
            .map(|(k, _)| k)
            .collect();
        assert_eq!(
            kept,
            ["ANTHROPIC_API_KEY", "AWS_REGION", "CLAUDE_CODE_USE_BEDROCK"]
        );
        assert!(claude_env(ClaudeAccessMode::Own, &env).is_empty());
        assert_eq!(tracker_id("c1", "r1").as_deref(), Some("org-c1-r1"));
        assert_eq!(tracker_id("../x", "r1"), None);
    }
}
