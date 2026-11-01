-- Append-only event log. `payload` is a serialized `bantu_protocol::EventData`.
CREATE TABLE events (
    seq INTEGER PRIMARY KEY AUTOINCREMENT,
    ts TEXT NOT NULL,
    kind TEXT NOT NULL,
    payload TEXT NOT NULL
);

-- Projections of the event log, kept in the same transaction as the event.
CREATE TABLE repos (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    root_path TEXT NOT NULL UNIQUE,
    default_branch TEXT NOT NULL,
    remote_url TEXT,
    created_at TEXT NOT NULL
);

CREATE TABLE workspaces (
    id TEXT PRIMARY KEY,
    repo_id TEXT NOT NULL REFERENCES repos(id),
    name TEXT NOT NULL,
    branch TEXT NOT NULL,
    base_ref TEXT NOT NULL,
    path TEXT NOT NULL,
    port_base INTEGER NOT NULL,
    state TEXT NOT NULL,
    error TEXT,
    archive_commit TEXT,
    created_at TEXT NOT NULL
);
CREATE INDEX idx_workspaces_repo_id ON workspaces(repo_id);

-- Responses to commands sent with an Idempotency-Key, so retries are safe.
CREATE TABLE idempotency (
    key TEXT PRIMARY KEY,
    status INTEGER NOT NULL,
    response TEXT NOT NULL,
    created_at TEXT NOT NULL
);
