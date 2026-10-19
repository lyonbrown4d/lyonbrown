diesel::table! { gateway_migrations (version) { version -> Integer, } }
diesel::table! { gateway_audits (sequence) { sequence -> BigInt, request_id -> Text, model -> Text, status -> SmallInt, elapsed_ms -> BigInt, policy_revision -> Text, } }
diesel::table! { gateway_model_snapshots (sequence) { sequence -> BigInt, revision -> Text, document -> Text, } }
diesel::table! { gateway_policy_revisions (revision) { revision -> Text, document -> Text, } }
diesel::table! { gateway_policy_head (id) { id -> Integer, revision -> Text, } }
diesel::table! { gateway_admin_accounts (id) { id -> Integer, username -> Text, password_hash -> Text, role -> SmallInt, credential_revision -> Text, } }
diesel::table! { gateway_admin_sessions (token_hash) { token_hash -> Text, csrf_hash -> Text, expires_at -> BigInt, credential_revision -> Text, } }
diesel::table! { gateway_access_tokens (id) { id -> Text, label -> Text, token_hash -> Text, created_at -> BigInt, expires_at -> BigInt, revoked -> Bool, } }

diesel::table! { gateway_model_head (id) { id -> Integer, revision -> Text, } }
