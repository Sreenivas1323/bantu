//! The inbox: notices and questions an agent sends to the user over MCP.
//!
//! Like permission requests, items live in memory and end with the process.
//! A question blocks its MCP call until the user answers, dismisses it or
//! stops the agent, or the timeout passes.

use std::collections::HashMap;
use std::time::Duration;

use bantu_protocol::{EventData, InboxItem, InboxKind, Session};
use tokio::sync::{oneshot, Mutex};

use crate::ops::{new_id, CommandError, Daemon};
use crate::store::now;

/// Old notices fall off rather than growing without bound.
const MAX_NOTICES: usize = 100;

type CmdResult<T> = Result<T, CommandError>;

#[derive(Default)]
pub struct Inbox {
    items: Mutex<HashMap<String, Entry>>,
}

struct Entry {
    item: InboxItem,
    /// Set for questions.
    answer: Option<oneshot::Sender<Answer>>,
}

/// How a question ended.
#[derive(Debug, PartialEq, Eq)]
pub enum Answer {
    Text(String),
    Dismissed,
    Stopped,
}

impl Daemon {
    pub async fn inbox_items(&self) -> Vec<InboxItem> {
        let mut list: Vec<_> = self
            .inbox
            .items
            .lock()
            .await
            .values()
            .map(|e| e.item.clone())
            .collect();
        list.sort_by(|a, b| a.created_at.cmp(&b.created_at));
        list
    }

    fn new_item(s: &Session, kind: InboxKind, title: String, body: Option<String>) -> InboxItem {
        InboxItem {
            id: new_id(),
            workspace_id: s.workspace_id.clone(),
            session_id: s.id.clone(),
            kind,
            title,
            body: body.filter(|b| !b.trim().is_empty()),
            options: vec![],
            created_at: now(),
        }
    }

    pub async fn notify_user(&self, s: &Session, title: String, body: Option<String>) {
        let item = Self::new_item(s, InboxKind::Notice, title, body);
        let oldest = {
            let mut items = self.inbox.items.lock().await;
            items.insert(
                item.id.clone(),
                Entry {
                    item: item.clone(),
                    answer: None,
                },
            );
            let notices: Vec<_> = items
                .values()
                .filter(|e| e.item.kind == InboxKind::Notice)
                .map(|e| (e.item.created_at.clone(), e.item.id.clone()))
                .collect();
            (notices.len() > MAX_NOTICES)
                .then(|| notices.into_iter().min())
                .flatten()
                .and_then(|(_, id)| items.remove(&id))
        };
        let _ = self.store.emit(EventData::InboxAdded { item }).await;
        if let Some(old) = oldest {
            self.announce_removed(&old.item).await;
        }
    }

    /// Publishes a question and waits for the answer. `None` on timeout.
    pub async fn ask_user(
        &self,
        s: &Session,
        question: String,
        options: Vec<String>,
        timeout: Duration,
    ) -> Option<Answer> {
        let mut item = Self::new_item(s, InboxKind::Question, question, None);
        item.options = options;
        let (tx, rx) = oneshot::channel();
        self.inbox.items.lock().await.insert(
            item.id.clone(),
            Entry {
                item: item.clone(),
                answer: Some(tx),
            },
        );
        let _ = self
            .store
            .emit(EventData::InboxAdded { item: item.clone() })
            .await;
        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(answer)) => Some(answer),
            Ok(Err(_)) => Some(Answer::Stopped),
            Err(_) => {
                self.inbox.items.lock().await.remove(&item.id);
                self.announce_removed(&item).await;
                None
            }
        }
    }

    pub async fn respond_inbox(&self, item_id: &str, answer: String) -> CmdResult<()> {
        let answer = answer.trim().to_string();
        if answer.is_empty() {
            return Err(CommandError::BadRequest("the answer is empty".into()));
        }
        let entry = self.take_inbox_item(item_id).await?;
        let Some(tx) = entry.answer else {
            self.announce_removed(&entry.item).await;
            return Err(CommandError::BadRequest(
                "that's a notice, not a question".into(),
            ));
        };
        self.announce_removed(&entry.item).await;
        let _ = tx.send(Answer::Text(answer));
        Ok(())
    }

    pub async fn dismiss_inbox(&self, item_id: &str) -> CmdResult<()> {
        let entry = self.take_inbox_item(item_id).await?;
        self.announce_removed(&entry.item).await;
        if let Some(tx) = entry.answer {
            let _ = tx.send(Answer::Dismissed);
        }
        Ok(())
    }

    /// Releases the questions of a stopped agent. Its notices stay.
    pub async fn drop_session_questions(&self, session_id: &str) {
        let drained: Vec<Entry> = {
            let mut items = self.inbox.items.lock().await;
            let ids: Vec<String> = items
                .iter()
                .filter(|(_, e)| e.item.session_id == session_id && e.answer.is_some())
                .map(|(id, _)| id.clone())
                .collect();
            ids.iter().filter_map(|id| items.remove(id)).collect()
        };
        for entry in drained {
            self.announce_removed(&entry.item).await;
            if let Some(tx) = entry.answer {
                let _ = tx.send(Answer::Stopped);
            }
        }
    }

    async fn take_inbox_item(&self, item_id: &str) -> CmdResult<Entry> {
        self.inbox
            .items
            .lock()
            .await
            .remove(item_id)
            .ok_or_else(|| CommandError::NotFound("that item was already handled".into()))
    }

    async fn announce_removed(&self, item: &InboxItem) {
        let _ = self
            .store
            .emit(EventData::InboxRemoved {
                item_id: item.id.clone(),
                workspace_id: item.workspace_id.clone(),
            })
            .await;
    }
}
