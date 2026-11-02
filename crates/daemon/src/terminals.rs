//! PTY terminals in workspaces: interactive shells and the repo's run script.
//!
//! Output is kept in a bounded scrollback buffer and fanned out to any number
//! of WebSocket viewers, so closing a browser tab never kills a dev server and
//! reopening it replays recent output.

use std::collections::{HashMap, VecDeque};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex as StdMutex};

use anyhow::Context;
use bantu_protocol::{EventData, Terminal, TerminalKind, Workspace, WorkspaceState};
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use tokio::sync::{broadcast, Mutex};

use crate::ops::{new_id, CommandError, Daemon};
use crate::settings;
use crate::store::now;

const SCROLLBACK_BYTES: usize = 512 * 1024;
const DEFAULT_SIZE: PtySize = PtySize {
    rows: 30,
    cols: 100,
    pixel_width: 0,
    pixel_height: 0,
};

type CmdResult<T> = Result<T, CommandError>;

#[derive(Default)]
pub struct Terminals {
    map: Mutex<HashMap<String, Arc<Term>>>,
}

pub struct Term {
    pub info: StdMutex<Terminal>,
    writer: StdMutex<Box<dyn Write + Send>>,
    master: StdMutex<Box<dyn MasterPty + Send>>,
    child: StdMutex<Box<dyn Child + Send + Sync>>,
    /// Recent output plus the live channel. Viewers subscribe while holding
    /// this lock so they see every byte exactly once.
    output: StdMutex<(VecDeque<u8>, broadcast::Sender<Vec<u8>>)>,
}

impl Term {
    /// Recent output and a receiver for everything after it.
    pub fn attach(&self) -> (Vec<u8>, broadcast::Receiver<Vec<u8>>) {
        let out = self.output.lock().unwrap();
        (out.0.iter().copied().collect(), out.1.subscribe())
    }

    pub fn write(&self, bytes: &[u8]) -> std::io::Result<()> {
        let mut w = self.writer.lock().unwrap();
        w.write_all(bytes)?;
        w.flush()
    }

    pub fn resize(&self, cols: u16, rows: u16) {
        let size = PtySize {
            rows: rows.max(2),
            cols: cols.max(2),
            pixel_width: 0,
            pixel_height: 0,
        };
        let _ = self.master.lock().unwrap().resize(size);
    }

    /// Hang up the whole process group so dev servers' children die too.
    fn kill(&self) {
        let mut child = self.child.lock().unwrap();
        #[cfg(unix)]
        if let Some(pid) = child.process_id() {
            // The PTY child is a session leader, so its pid is the group id.
            unsafe {
                libc::killpg(pid as libc::pid_t, libc::SIGHUP);
                libc::killpg(pid as libc::pid_t, libc::SIGTERM);
            }
        }
        let _ = child.kill();
    }
}

impl Daemon {
    pub async fn terminals(&self) -> Vec<Terminal> {
        let map = self.terminals.map.lock().await;
        let mut list: Vec<Terminal> = map
            .values()
            .map(|t| t.info.lock().unwrap().clone())
            .collect();
        list.sort_by(|a, b| a.created_at.cmp(&b.created_at));
        list
    }

    pub async fn terminal(&self, id: &str) -> Option<Arc<Term>> {
        self.terminals.map.lock().await.get(id).cloned()
    }

    pub async fn open_terminal(
        self: &Arc<Self>,
        workspace_id: &str,
        cols: Option<u16>,
        rows: Option<u16>,
    ) -> CmdResult<Terminal> {
        let ws = self.ready_workspace(workspace_id).await?;
        let count = self
            .terminals()
            .await
            .iter()
            .filter(|t| t.workspace_id == ws.id && t.kind == TerminalKind::Shell)
            .count();
        let title = format!("Terminal {}", count + 1);
        self.spawn_terminal(&ws, TerminalKind::Shell, title, None, cols, rows)
            .await
    }

    pub async fn run_workspace(self: &Arc<Self>, workspace_id: &str) -> CmdResult<Terminal> {
        let ws = self.ready_workspace(workspace_id).await?;
        if let Some(t) = self
            .terminals()
            .await
            .into_iter()
            .find(|t| t.workspace_id == ws.id && t.kind == TerminalKind::Run && !t.exited)
        {
            return Ok(t);
        }
        let repo = self
            .store
            .repo(&ws.repo_id)
            .await?
            .ok_or_else(|| CommandError::NotFound("repository is gone".into()))?;
        let settings = settings::load(&[
            PathBuf::from(&ws.path).as_path(),
            PathBuf::from(&repo.root_path).as_path(),
        ])
        .map_err(CommandError::BadRequest)?;
        let command = settings.scripts.run_command().ok_or_else(|| {
            CommandError::BadRequest(
                "no run script: add `[scripts]\nrun = \"…\"` to .bantu/settings.toml".into(),
            )
        })?;
        self.spawn_terminal(
            &ws,
            TerminalKind::Run,
            "Run".into(),
            Some(command),
            None,
            None,
        )
        .await
    }

    pub async fn close_terminal(&self, id: &str) -> CmdResult<()> {
        let term = self
            .terminals
            .map
            .lock()
            .await
            .remove(id)
            .ok_or_else(|| CommandError::NotFound(format!("no terminal {id}")))?;
        term.kill();
        let workspace_id = term.info.lock().unwrap().workspace_id.clone();
        self.store
            .emit(EventData::TerminalClosed {
                terminal_id: id.to_string(),
                workspace_id,
            })
            .await?;
        Ok(())
    }

    pub async fn close_workspace_terminals(&self, workspace_id: &str) -> anyhow::Result<()> {
        for t in self.terminals().await {
            if t.workspace_id == workspace_id {
                let _ = self.close_terminal(&t.id).await;
            }
        }
        Ok(())
    }

    /// Terminals from a previous daemon process are gone; say so in the log.
    pub async fn recover_terminals(&self) -> anyhow::Result<()> {
        for (terminal_id, workspace_id) in self.store.open_terminals_in_log().await? {
            self.store
                .emit(EventData::TerminalClosed {
                    terminal_id,
                    workspace_id,
                })
                .await?;
        }
        Ok(())
    }

    async fn ready_workspace(&self, id: &str) -> CmdResult<Workspace> {
        let ws = self.require_workspace(id).await?;
        if ws.state != WorkspaceState::Ready {
            return Err(CommandError::Conflict(format!(
                "workspace is {}, not ready",
                ws.state.as_str()
            )));
        }
        Ok(ws)
    }

    async fn spawn_terminal(
        self: &Arc<Self>,
        ws: &Workspace,
        kind: TerminalKind,
        title: String,
        command: Option<String>,
        cols: Option<u16>,
        rows: Option<u16>,
    ) -> CmdResult<Terminal> {
        let size = PtySize {
            cols: cols.unwrap_or(DEFAULT_SIZE.cols).max(2),
            rows: rows.unwrap_or(DEFAULT_SIZE.rows).max(2),
            ..DEFAULT_SIZE
        };
        let pair = native_pty_system().openpty(size).context("opening a pty")?;
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into());
        let mut cmd = CommandBuilder::new(&shell);
        match &command {
            Some(c) => cmd.args(["-lc", c]),
            None => cmd.arg("-l"),
        }
        cmd.cwd(&ws.path);
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        cmd.env("BANTU_PORT", ws.port_base.to_string());
        cmd.env("BANTU_WORKSPACE_ID", &ws.id);
        cmd.env("BANTU_WORKSPACE_NAME", &ws.name);
        cmd.env("BANTU_WORKSPACE_PATH", &ws.path);
        let child = pair
            .slave
            .spawn_command(cmd)
            .context("starting the shell")?;
        drop(pair.slave);
        let reader = pair.master.try_clone_reader().context("pty reader")?;
        let writer = pair.master.take_writer().context("pty writer")?;

        let info = Terminal {
            id: new_id(),
            workspace_id: ws.id.clone(),
            kind,
            title,
            command,
            exited: false,
            exit_code: None,
            created_at: now(),
        };
        let (tx, _) = broadcast::channel(256);
        let term = Arc::new(Term {
            info: StdMutex::new(info.clone()),
            writer: StdMutex::new(writer),
            master: StdMutex::new(pair.master),
            child: StdMutex::new(child),
            output: StdMutex::new((VecDeque::new(), tx)),
        });
        self.terminals
            .map
            .lock()
            .await
            .insert(info.id.clone(), term.clone());
        self.store
            .emit(EventData::TerminalUpdated {
                terminal: info.clone(),
            })
            .await?;

        let this = self.clone();
        let rt = tokio::runtime::Handle::current();
        std::thread::Builder::new()
            .name(format!("pty-{}", info.id))
            .spawn(move || pump_output(reader, term, this, rt))
            .context("starting pty reader")?;
        Ok(info)
    }
}

/// Blocking loop: PTY output → scrollback + viewers, then record the exit.
fn pump_output(
    mut reader: Box<dyn Read + Send>,
    term: Arc<Term>,
    daemon: Arc<Daemon>,
    rt: tokio::runtime::Handle,
) {
    let mut buf = [0u8; 16 * 1024];
    loop {
        match reader.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                let mut out = term.output.lock().unwrap();
                out.0.extend(&buf[..n]);
                let excess = out.0.len().saturating_sub(SCROLLBACK_BYTES);
                out.0.drain(..excess);
                let _ = out.1.send(buf[..n].to_vec());
            }
        }
    }
    let code = term
        .child
        .lock()
        .unwrap()
        .wait()
        .ok()
        .map(|s| s.exit_code() as i32);
    let info = {
        let mut info = term.info.lock().unwrap();
        info.exited = true;
        info.exit_code = code;
        info.clone()
    };
    rt.spawn(async move {
        // Closed terminals were already removed and announced.
        if daemon.terminal(&info.id).await.is_some() {
            let _ = daemon
                .store
                .emit(EventData::TerminalUpdated { terminal: info })
                .await;
        }
    });
}
