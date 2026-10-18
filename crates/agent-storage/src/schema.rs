diesel::table! { sessions (id) { id -> Text, owner_epoch -> BigInt, record -> Text, } }
diesel::table! { harness_sessions (session_id) { session_id -> Text, snapshot -> Binary, } }
diesel::table! { runs (id) { id -> Text, session_id -> Text, idempotency_key -> Text, status -> Text, created_at_ms -> BigInt, record -> Text, } }
diesel::table! { events (run_id, cursor) { run_id -> Text, cursor -> BigInt, record -> Text, } }
diesel::table! { outbox (run_id, cursor) { run_id -> Text, cursor -> BigInt, delivered -> Integer, } }
diesel::table! { approvals (id) { id -> Text, run_id -> Text, status -> Text, record -> Text, } }
diesel::table! { tasks (id) { id -> Text, idempotency_key -> Text, owner_epoch -> BigInt, record -> Text, } }
diesel::table! { peers (node_id) { node_id -> Text, record -> Text, } }
diesel::allow_tables_to_appear_in_same_query!(
  sessions,
  harness_sessions,
  runs,
  events,
  outbox,
  approvals,
  tasks,
  peers
);
