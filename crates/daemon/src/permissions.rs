//! Permission prompts, answered from any Bantu client.
//!
//! Each agent turn gets a `PermissionRequest` HTTP hook (via `--settings`)
//! pointing back at the daemon. Claude Code calls it whenever a tool needs
//! approval and waits for the response, so the request blocks here until a
//! user answers (or the turn is stopped, or it times out).

use std::collections::HashMap;
use std::sync::OnceLock;
use std::time::Duration;

use bantu_protocol::{EventData, PermissionRequest, Session};
use serde_json::{json, Value};
use tokio::sync::{oneshot, Mutex};

use crate::ops::{new_id, CommandError, Daemon};
use crate::store::now;

/// Claude's hook timeout is 600 s; answer a little before it gives up.
const ANSWER_TIMEOUT: Duration = Duration::from_secs(590);

pub struct Permissions {
    pending: Mutex<HashMap<String, Pending>>,
    /// Secret the hook presents; it only grants access to `/v1/hooks/*`.
    pub hook_token: String,
    /// `http://host:port` the daemon listens on, once bound.
    base_url: OnceLock<String>,
}

struct Pending {
    request: PermissionRequest,
    answer: oneshot::Sender<Decision>,
}

#[derive(Debug)]
pub struct Decision {
    pub allow: bool,
    pub message: Option<String>,
    /// Deny and stop the turn.
    pub interrupt: bool,
}

impl Default for Permissions {
    fn default() -> Self {
        Self {
            pending: Mutex::new(HashMap::new()),
            hook_token: format!("{}", uuid::Uuid::new_v4().simple()),
            base_url: OnceLock::new(),
        }
    }
}

type CmdResult<T> = Result<T, CommandError>;

impl Daemon {
    /// Call once the HTTP listener is bound so agent turns can reach the hook.
    pub fn set_listen_addr(&self, mut addr: std::net::SocketAddr) {
        // Agents run on this machine, so call back over loopback even when
        // the daemon listens on every interface.
        if addr.ip().is_unspecified() {
            addr.set_ip(if addr.is_ipv4() {
                std::net::Ipv4Addr::LOCALHOST.into()
            } else {
                std::net::Ipv6Addr::LOCALHOST.into()
            });
        }
        let _ = self.permissions.base_url.set(format!("http://{addr}"));
    }

    /// `--settings` JSON that routes this session's permission prompts to us,
    /// or `None` before the listener is bound.
    pub fn permission_hook_settings(&self, session_id: &str) -> Option<String> {
        let base = self.permissions.base_url.get()?;
        Some(
            json!({
                "hooks": {
                    "PermissionRequest": [{
                        "matcher": "*",
                        "hooks": [{
                            "type": "http",
                            "url": format!("{base}/v1/hooks/permission?session={session_id}"),
                            // The token travels in the environment, not argv,
                            // so other users can't read it from `ps`.
                            "headers": {"Authorization": "Bearer $BANTU_HOOK_TOKEN"},
                            "allowedEnvVars": ["BANTU_HOOK_TOKEN"],
                            "timeout": 600
                        }]
                    }]
                }
            })
            .to_string(),
        )
    }

    pub fn hook_token(&self) -> &str {
        &self.permissions.hook_token
    }

    pub async fn pending_permissions(&self) -> Vec<PermissionRequest> {
        let mut list: Vec<_> = self
            .permissions
            .pending
            .lock()
            .await
            .values()
            .map(|p| p.request.clone())
            .collect();
        list.sort_by(|a, b| a.created_at.cmp(&b.created_at));
        list
    }

    /// Handles one hook call: publish the request, wait for an answer, and
    /// return Claude's hook response body.
    pub async fn request_permission(&self, session: &Session, hook_input: &Value) -> Value {
        let request = PermissionRequest {
            id: new_id(),
            session_id: session.id.clone(),
            workspace_id: session.workspace_id.clone(),
            tool_name: hook_input["tool_name"]
                .as_str()
                .unwrap_or("tool")
                .to_string(),
            tool_input: hook_input["tool_input"].clone(),
            created_at: now(),
        };
        let (tx, rx) = oneshot::channel();
        self.permissions.pending.lock().await.insert(
            request.id.clone(),
            Pending {
                request: request.clone(),
                answer: tx,
            },
        );
        let _ = self
            .store
            .emit(EventData::PermissionRequested {
                request: request.clone(),
            })
            .await;

        let decision = match tokio::time::timeout(ANSWER_TIMEOUT, rx).await {
            Ok(Ok(d)) => d,
            // Dropped sender: the request was discarded without an answer.
            Ok(Err(_)) => Decision {
                allow: false,
                message: None,
                interrupt: true,
            },
            Err(_) => {
                self.permissions.pending.lock().await.remove(&request.id);
                self.announce_resolved(&request, false).await;
                Decision {
                    allow: false,
                    message: Some("Nobody answered the permission request in time.".into()),
                    interrupt: false,
                }
            }
        };
        hook_response(&decision)
    }

    pub async fn respond_permission(
        &self,
        request_id: &str,
        allow: bool,
        message: Option<String>,
    ) -> CmdResult<()> {
        let pending = self
            .permissions
            .pending
            .lock()
            .await
            .remove(request_id)
            .ok_or_else(|| {
                CommandError::NotFound("that request was already answered or expired".into())
            })?;
        self.announce_resolved(&pending.request, allow).await;
        let _ = pending.answer.send(Decision {
            allow,
            message: message.filter(|m| !m.trim().is_empty()),
            interrupt: false,
        });
        Ok(())
    }

    /// Denies everything a session is waiting on (used when it's stopped).
    pub async fn deny_session_permissions(&self, session_id: &str) {
        let drained: Vec<Pending> = {
            let mut map = self.permissions.pending.lock().await;
            let ids: Vec<String> = map
                .iter()
                .filter(|(_, p)| p.request.session_id == session_id)
                .map(|(id, _)| id.clone())
                .collect();
            ids.into_iter().filter_map(|id| map.remove(&id)).collect()
        };
        for p in drained {
            self.announce_resolved(&p.request, false).await;
            let _ = p.answer.send(Decision {
                allow: false,
                message: Some("The user stopped the agent.".into()),
                interrupt: true,
            });
        }
    }

    async fn announce_resolved(&self, r: &PermissionRequest, allowed: bool) {
        let _ = self
            .store
            .emit(EventData::PermissionResolved {
                request_id: r.id.clone(),
                session_id: r.session_id.clone(),
                workspace_id: r.workspace_id.clone(),
                allowed,
            })
            .await;
    }
}

fn hook_response(d: &Decision) -> Value {
    let decision = if d.allow {
        json!({"behavior": "allow"})
    } else {
        let mut v = json!({
            "behavior": "deny",
            "message": d.message.clone().unwrap_or_else(|| "The user denied this action.".into()),
        });
        if d.interrupt {
            v["interrupt"] = json!(true);
        }
        v
    };
    json!({"hookSpecificOutput": {"hookEventName": "PermissionRequest", "decision": decision}})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hook_responses_match_claude_schema() {
        let allow = hook_response(&Decision {
            allow: true,
            message: None,
            interrupt: false,
        });
        assert_eq!(
            allow["hookSpecificOutput"]["decision"],
            json!({"behavior": "allow"})
        );
        let deny = hook_response(&Decision {
            allow: false,
            message: Some("use bun".into()),
            interrupt: true,
        });
        assert_eq!(
            deny["hookSpecificOutput"]["decision"],
            json!({"behavior": "deny", "message": "use bun", "interrupt": true})
        );
        assert_eq!(
            deny["hookSpecificOutput"]["hookEventName"],
            "PermissionRequest"
        );
    }
}
