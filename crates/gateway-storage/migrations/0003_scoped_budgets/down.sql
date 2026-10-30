ALTER TABLE gateway_audits DROP COLUMN usage;
ALTER TABLE gateway_audits DROP COLUMN reserved_tokens;
ALTER TABLE gateway_audits DROP COLUMN principal;
ALTER TABLE gateway_access_tokens DROP COLUMN tokens_charged;
ALTER TABLE gateway_access_tokens DROP COLUMN token_budget;
ALTER TABLE gateway_access_tokens DROP COLUMN max_output_tokens;
ALTER TABLE gateway_access_tokens DROP COLUMN allowed_models;
ALTER TABLE gateway_access_tokens DROP COLUMN project_id;
