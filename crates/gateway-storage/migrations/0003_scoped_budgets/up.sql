ALTER TABLE gateway_access_tokens ADD COLUMN project_id TEXT NOT NULL DEFAULT 'default';
ALTER TABLE gateway_access_tokens ADD COLUMN allowed_models TEXT NOT NULL DEFAULT '[]';
ALTER TABLE gateway_access_tokens ADD COLUMN max_output_tokens BIGINT CHECK (max_output_tokens >= 0);
ALTER TABLE gateway_access_tokens ADD COLUMN token_budget BIGINT CHECK (token_budget >= 0);
ALTER TABLE gateway_access_tokens ADD COLUMN tokens_charged BIGINT NOT NULL DEFAULT 0 CHECK (tokens_charged >= 0);
ALTER TABLE gateway_audits ADD COLUMN principal TEXT;
ALTER TABLE gateway_audits ADD COLUMN reserved_tokens BIGINT NOT NULL DEFAULT 0 CHECK (reserved_tokens >= 0);
ALTER TABLE gateway_audits ADD COLUMN usage TEXT;
