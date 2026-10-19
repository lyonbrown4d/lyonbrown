CREATE TABLE IF NOT EXISTS gateway_migrations (version INTEGER PRIMARY KEY NOT NULL);
CREATE TABLE IF NOT EXISTS gateway_audits (
  sequence INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
  request_id TEXT NOT NULL UNIQUE,
  model TEXT NOT NULL,
  status SMALLINT NOT NULL,
  elapsed_ms BIGINT NOT NULL,
  policy_revision TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS gateway_model_snapshots (
  sequence INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
  revision TEXT NOT NULL UNIQUE,
  document TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS gateway_policy_revisions (
  revision TEXT PRIMARY KEY NOT NULL,
  document TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS gateway_policy_head (
  id INTEGER PRIMARY KEY NOT NULL,
  revision TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS gateway_admin_credentials (
  id INTEGER PRIMARY KEY NOT NULL,
  token_hash TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS gateway_admin_sessions (
  token_hash TEXT PRIMARY KEY NOT NULL,
  csrf_hash TEXT NOT NULL,
  expires_at BIGINT NOT NULL
);
CREATE INDEX IF NOT EXISTS gateway_admin_sessions_expiry ON gateway_admin_sessions (expires_at);
CREATE TABLE IF NOT EXISTS gateway_access_tokens (
  id TEXT PRIMARY KEY NOT NULL,
  label TEXT NOT NULL,
  token_hash TEXT NOT NULL UNIQUE,
  created_at BIGINT NOT NULL,
  expires_at BIGINT NOT NULL,
  revoked BOOLEAN NOT NULL
);
CREATE INDEX IF NOT EXISTS gateway_access_tokens_expiry ON gateway_access_tokens (expires_at);
CREATE TABLE IF NOT EXISTS gateway_model_head (id INTEGER PRIMARY KEY NOT NULL, revision TEXT NOT NULL);
