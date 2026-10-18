CREATE TABLE IF NOT EXISTS browser_actions (
  action_id TEXT PRIMARY KEY NOT NULL,
  session_id TEXT NOT NULL,
  owner_epoch BIGINT NOT NULL,
  kind TEXT NOT NULL,
  arguments TEXT NOT NULL,
  status TEXT NOT NULL,
  result TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS browser_actions_status ON browser_actions(status);
