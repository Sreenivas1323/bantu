-- Effort and fast mode are per-chat settings. A fork remembers the chat it came
-- from, and the parent's conversation id until its first turn succeeds.
ALTER TABLE sessions ADD COLUMN effort TEXT;
ALTER TABLE sessions ADD COLUMN fast INTEGER NOT NULL DEFAULT 0;
ALTER TABLE sessions ADD COLUMN forked_from TEXT;
ALTER TABLE sessions ADD COLUMN fork_source TEXT;
