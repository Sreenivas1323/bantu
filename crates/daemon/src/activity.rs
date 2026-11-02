//! Is anyone using this daemon? Cloud machines use `GET /v1/activity` to
//! stop themselves when idle, so a forgotten box doesn't run up a bill.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Instant;

use bantu_protocol::{Activity, SessionStatus};

use crate::ops::Daemon;

pub struct Tracker {
    last_active: Mutex<Instant>,
    clients: AtomicUsize,
}

impl Default for Tracker {
    fn default() -> Self {
        Self {
            last_active: Mutex::new(Instant::now()),
            clients: AtomicUsize::new(0),
        }
    }
}

/// Counts a connected client (event stream or terminal) while it lives.
pub struct ClientGuard<'a>(&'a Tracker);

impl Drop for ClientGuard<'_> {
    fn drop(&mut self) {
        self.0.clients.fetch_sub(1, Ordering::Relaxed);
        self.0.touch();
    }
}

impl Tracker {
    pub fn touch(&self) {
        *self.last_active.lock().unwrap() = Instant::now();
    }

    pub fn client(&self) -> ClientGuard<'_> {
        self.clients.fetch_add(1, Ordering::Relaxed);
        self.touch();
        ClientGuard(self)
    }
}

impl Daemon {
    pub async fn activity(&self) -> anyhow::Result<Activity> {
        let running_sessions = self
            .store
            .sessions()
            .await?
            .iter()
            .filter(|s| s.status == SessionStatus::Running)
            .count() as u32;
        let open_terminals = self.terminals().await.iter().filter(|t| !t.exited).count() as u32;
        let waiting = !self.pending_permissions().await.is_empty();
        let connected_clients = self.activity.clients.load(Ordering::Relaxed) as u32;
        let busy = running_sessions > 0 || open_terminals > 0 || waiting || connected_clients > 0;
        if busy {
            self.activity.touch();
        }
        let idle_seconds = self
            .activity
            .last_active
            .lock()
            .unwrap()
            .elapsed()
            .as_secs();
        Ok(Activity {
            busy,
            idle_seconds,
            connected_clients,
            running_sessions,
            open_terminals,
        })
    }
}
