-- Downgrade revokes account sessions and creates no legacy login credential.
DROP TABLE IF EXISTS gateway_admin_sessions;
DROP TABLE IF EXISTS gateway_admin_accounts;
CREATE TABLE gateway_admin_credentials (id INTEGER PRIMARY KEY NOT NULL, token_hash TEXT NOT NULL);
CREATE TABLE gateway_admin_sessions (token_hash TEXT PRIMARY KEY NOT NULL, csrf_hash TEXT NOT NULL, expires_at BIGINT NOT NULL);
CREATE INDEX gateway_admin_sessions_expiry ON gateway_admin_sessions (expires_at);
