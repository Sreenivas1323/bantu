//! Running `gh` as the right GitHub login for each repo.
//!
//! People often have several logins (personal, work, a client's), and `gh`
//! uses whichever is active. When a repo's remote is only visible to another
//! signed-in login, Bantu remembers that login for the repo and gives `gh`,
//! and the repo's agents and terminals, its token through `GH_TOKEN`. Git on a
//! machine set up with `gh auth setup-git` uses the same token, so pushes work
//! too. The active login itself is never changed.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, RwLock};
use std::time::{Duration, Instant};

use serde_json::Value;

/// Folder prefixes (a repo's checkout and its workspaces) and their login.
static ACCOUNTS: LazyLock<RwLock<Vec<(PathBuf, String)>>> = LazyLock::new(Default::default);
/// Tokens by login, refreshed now and then in case one is replaced.
static TOKENS: LazyLock<RwLock<HashMap<String, (Instant, String)>>> =
    LazyLock::new(Default::default);
const TOKEN_TTL: Duration = Duration::from_secs(600);

/// Uses `login` for `gh` in these folders, or the active login again for `None`.
pub async fn set_account(paths: &[PathBuf], login: Option<&str>) {
    {
        let mut accounts = ACCOUNTS.write().unwrap();
        accounts.retain(|(p, _)| !paths.contains(p));
        if let Some(login) = login {
            accounts.extend(paths.iter().map(|p| (p.clone(), login.to_string())));
        }
    }
    // Fetched now, so starting an agent never waits on it.
    if let Some(login) = login {
        let _ = token(login).await;
    }
}

/// The login for `dir`, if it isn't the active one.
pub fn account_for(dir: &Path) -> Option<String> {
    ACCOUNTS
        .read()
        .unwrap()
        .iter()
        .filter(|(prefix, _)| dir.starts_with(prefix))
        .max_by_key(|(prefix, _)| prefix.as_os_str().len())
        .map(|(_, login)| login.clone())
}

/// A login's token, from the keychain through `gh`.
pub async fn token(login: &str) -> Option<String> {
    if let Some((at, token)) = TOKENS.read().unwrap().get(login) {
        if at.elapsed() < TOKEN_TTL {
            return Some(token.clone());
        }
    }
    let out = tokio::process::Command::new("gh")
        .args(["auth", "token", "--hostname", "github.com", "--user", login])
        .env_remove("GH_TOKEN")
        .env_remove("GITHUB_TOKEN")
        .output()
        .await
        .ok()?;
    let token = String::from_utf8(out.stdout).ok()?.trim().to_string();
    if !out.status.success() || token.is_empty() {
        return None;
    }
    TOKENS
        .write()
        .unwrap()
        .insert(login.to_string(), (Instant::now(), token.clone()));
    Some(token)
}

/// `GH_TOKEN` for processes in `dir`, from what's already cached. Synchronous,
/// for places that set up a process's environment.
pub fn cached_env(dir: &Path) -> Option<(&'static str, String)> {
    let login = account_for(dir)?;
    let token = TOKENS.read().unwrap().get(&login)?.1.clone();
    Some(("GH_TOKEN", token))
}

/// A `gh` command for `dir`, signed in as that folder's login.
pub async fn command(dir: &Path) -> tokio::process::Command {
    let mut cmd = tokio::process::Command::new("gh");
    cmd.current_dir(dir).env("GH_PROMPT_DISABLED", "1");
    if let Some(login) = account_for(dir) {
        if let Some(token) = token(&login).await {
            cmd.env("GH_TOKEN", token);
        }
    }
    cmd
}

/// `owner/repo` for a github.com remote, in https or ssh form.
pub fn owner_repo(remote: &str) -> Option<String> {
    let rest = remote
        .strip_prefix("https://github.com/")
        .or_else(|| remote.strip_prefix("http://github.com/"))
        .or_else(|| remote.strip_prefix("git@github.com:"))
        .or_else(|| remote.strip_prefix("ssh://git@github.com/"))?;
    // https remotes may carry credentials: https://user@github.com/... isn't matched above.
    let rest = rest.trim_end_matches('/').trim_end_matches(".git");
    let mut parts = rest.split('/');
    let (owner, repo) = (parts.next()?, parts.next()?);
    (!owner.is_empty() && !repo.is_empty() && parts.next().is_none())
        .then(|| format!("{owner}/{repo}"))
}

/// Signed-in github.com logins, the active one first.
pub fn parse_logins(status: &Value) -> Vec<String> {
    let mut accounts: Vec<(bool, String)> = status["hosts"]["github.com"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|a| a["state"] == "success")
        .filter_map(|a| Some((a["active"] == true, a["login"].as_str()?.to_string())))
        .collect();
    accounts.sort_by_key(|(active, _)| !active);
    accounts.into_iter().map(|(_, login)| login).collect()
}

/// Which login can see `remote`: `None` when the active one can (or nobody
/// can, or it isn't on github.com), otherwise the first other login that can.
pub async fn detect(remote: &str) -> Option<String> {
    let slug = owner_repo(remote)?;
    let status = tokio::process::Command::new("gh")
        .args([
            "auth",
            "status",
            "--hostname",
            "github.com",
            "--json",
            "hosts",
        ])
        .env_remove("GH_TOKEN")
        .env_remove("GITHUB_TOKEN")
        .output()
        .await
        .ok()?;
    let logins = parse_logins(&serde_json::from_slice(&status.stdout).ok()?);
    for (i, login) in logins.iter().enumerate() {
        let token = token(login).await?;
        let seen = tokio::process::Command::new("gh")
            .args(["api", &format!("repos/{slug}"), "--silent"])
            .env("GH_TOKEN", token)
            .env("GH_PROMPT_DISABLED", "1")
            // Not seeing it is the expected answer for most logins.
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .await
            .is_ok_and(|s| s.success());
        if seen {
            return (i > 0).then(|| login.clone());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reads_github_remotes() {
        assert_eq!(
            owner_repo("https://github.com/Modozo-Tech/web-app.git").as_deref(),
            Some("Modozo-Tech/web-app")
        );
        assert_eq!(
            owner_repo("git@github.com:Subler/subler-API.git").as_deref(),
            Some("Subler/subler-API")
        );
        assert_eq!(
            owner_repo("ssh://git@github.com/a/b").as_deref(),
            Some("a/b")
        );
        assert_eq!(owner_repo("https://gitlab.com/a/b.git"), None);
        assert_eq!(owner_repo("https://github.com/a/b/c"), None);
    }

    #[test]
    fn lists_logins_active_first() {
        let status = json!({"hosts": {"github.com": [
            {"login": "work", "active": false, "state": "success"},
            {"login": "me", "active": true, "state": "success"},
            {"login": "stale", "active": false, "state": "error"}
        ]}});
        assert_eq!(parse_logins(&status), ["me", "work"]);
    }

    #[tokio::test]
    async fn picks_the_most_specific_folder() {
        let root = PathBuf::from("/tmp/bantu-gh-test/repo");
        let ws = PathBuf::from("/tmp/bantu-gh-test/workspaces/repo");
        set_account(&[root.clone(), ws.clone()], None).await;
        ACCOUNTS.write().unwrap().extend([
            (root.clone(), "work".to_string()),
            (ws.clone(), "work".to_string()),
        ]);
        assert_eq!(account_for(&root.join("src")).as_deref(), Some("work"));
        assert_eq!(account_for(&ws.join("tokyo")).as_deref(), Some("work"));
        assert_eq!(account_for(Path::new("/tmp/bantu-gh-test/other")), None);
        set_account(&[root.clone(), ws.clone()], None).await;
        assert_eq!(account_for(&root), None);
    }
}
