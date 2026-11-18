-- Whether a chat's agent gets a headless browser (Playwright MCP).
ALTER TABLE sessions ADD COLUMN browser INTEGER NOT NULL DEFAULT 0;
