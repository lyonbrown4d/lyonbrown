CREATE TABLE IF NOT EXISTS operations (
  action_id TEXT PRIMARY KEY NOT NULL,
  run_id TEXT NOT NULL,
  owner_epoch BIGINT NOT NULL,
  status TEXT NOT NULL CHECK (status IN ('intent', 'completed', 'failed', 'uncertain')),
  record TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS operations_status ON operations(status);
