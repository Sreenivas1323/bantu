-- The gh login a repo's GitHub calls use, when it isn't the active one.
ALTER TABLE repos ADD COLUMN github_account TEXT;
