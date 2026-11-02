-- Per-workspace history needs to find a workspace's events without scanning
-- and parsing the whole log.
ALTER TABLE events ADD COLUMN workspace_id TEXT;
UPDATE events SET workspace_id = COALESCE(
    json_extract(payload, '$.workspace_id'),
    json_extract(payload, '$.workspace.id'),
    json_extract(payload, '$.session.workspace_id')
);
CREATE INDEX idx_events_workspace_id ON events(workspace_id, seq);

CREATE TABLE sessions (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    agent TEXT NOT NULL,
    title TEXT NOT NULL,
    model TEXT,
    permission_mode TEXT NOT NULL,
    status TEXT NOT NULL,
    queued INTEGER NOT NULL DEFAULT 0,
    turns INTEGER NOT NULL DEFAULT 0,
    total_cost_usd REAL NOT NULL DEFAULT 0,
    -- The agent's own conversation id (Claude's --session-id), used to resume.
    agent_session_id TEXT NOT NULL,
    created_at TEXT NOT NULL
);
CREATE INDEX idx_sessions_workspace_id ON sessions(workspace_id);
