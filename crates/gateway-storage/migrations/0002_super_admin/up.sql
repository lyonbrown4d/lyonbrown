-- Legacy token credentials and sessions cannot authenticate after the account migration.
DROP TABLE IF EXISTS gateway_admin_credentials;
DROP TABLE IF EXISTS gateway_admin_sessions;
CREATE TABLE gateway_admin_accounts (
  id INTEGER PRIMARY KEY NOT NULL CHECK (id = 1),
  username TEXT NOT NULL UNIQUE,
  password_hash TEXT NOT NULL,
  role SMALLINT NOT NULL CHECK (role = 1),
  credential_revision TEXT NOT NULL UNIQUE
);
CREATE TABLE gateway_admin_sessions (
  token_hash TEXT PRIMARY KEY NOT NULL,
  csrf_hash TEXT NOT NULL,
  expires_at BIGINT NOT NULL,
  credential_revision TEXT NOT NULL
);
CREATE INDEX gateway_admin_sessions_expiry ON gateway_admin_sessions (expires_at);
