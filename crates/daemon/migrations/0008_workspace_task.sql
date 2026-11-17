-- The tracker task (Jira, ClickUp, GitHub) a workspace was started from, as JSON.
ALTER TABLE workspaces ADD COLUMN task TEXT;
