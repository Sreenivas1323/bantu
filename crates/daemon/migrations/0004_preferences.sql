-- Daemon-wide preferences as JSON, one row per scope (only 'app' for now).
CREATE TABLE preferences (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
