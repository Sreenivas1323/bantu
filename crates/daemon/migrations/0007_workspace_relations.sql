-- A workspace started by another workspace's agent, or by a loop; whether another
-- tool owns its worktree (imported); and where automatic review has got to.
ALTER TABLE workspaces ADD COLUMN parent_id TEXT;
ALTER TABLE workspaces ADD COLUMN loop_id TEXT;
ALTER TABLE workspaces ADD COLUMN external INTEGER NOT NULL DEFAULT 0;
ALTER TABLE workspaces ADD COLUMN review_state TEXT;
