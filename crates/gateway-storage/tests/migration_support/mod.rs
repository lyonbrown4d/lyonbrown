use diesel::{
  connection::SimpleConnection,
  migration::{Migration, MigrationConnection, MigrationSource},
  prelude::*,
  sqlite::Sqlite,
};
use diesel_migrations::{EmbeddedMigrations, embed_migrations};
use std::path::Path;

diesel::table! { gateway_migrations (version) { version -> Integer, } }
diesel::table! { __diesel_schema_migrations (version) { version -> Text, run_on -> Timestamp, } }
diesel::table! { sqlite_schema (name) { name -> Text, #[sql_name = "type"] kind -> Text, sql -> Nullable<Text>, } }
diesel::table! { gateway_admin_credentials (id) { id -> Integer, token_hash -> Text, } }
diesel::table! { gateway_admin_sessions (token_hash) { token_hash -> Text, csrf_hash -> Text, expires_at -> BigInt, } }
diesel::table! { gateway_audits (sequence) { sequence -> BigInt, request_id -> Text, model -> Text, status -> SmallInt, elapsed_ms -> BigInt, policy_revision -> Text, } }

pub fn connect(path: &Path) -> SqliteConnection {
  SqliteConnection::establish(path.to_str().expect("path")).expect("connection")
}

pub fn known_versions() -> Vec<String> {
  const MIGRATIONS: EmbeddedMigrations = embed_migrations!("migrations");
  let migrations: Vec<Box<dyn Migration<Sqlite>>> = MIGRATIONS.migrations().expect("source");
  let mut versions = migrations
    .iter()
    .map(|migration| migration.name().version().to_string())
    .collect::<Vec<_>>();
  versions.sort();
  versions
}

pub fn v1_fixture(connection: &mut SqliteConnection, versions: &[i32]) {
  connection
    .batch_execute(include_str!("../../migrations/0001_initial/up.sql"))
    .expect("v1 schema");
  diesel::insert_into(gateway_migrations::table)
    .values(
      versions
        .iter()
        .map(|version| gateway_migrations::version.eq(version))
        .collect::<Vec<_>>(),
    )
    .execute(connection)
    .expect("legacy versions");
  diesel::insert_into(gateway_admin_credentials::table)
    .values((
      gateway_admin_credentials::id.eq(1),
      gateway_admin_credentials::token_hash.eq("legacy-admin-hash"),
    ))
    .execute(connection)
    .expect("legacy admin");
  diesel::insert_into(gateway_admin_sessions::table)
    .values((
      gateway_admin_sessions::token_hash.eq("legacy-session-hash"),
      gateway_admin_sessions::csrf_hash.eq("legacy-csrf-hash"),
      gateway_admin_sessions::expires_at.eq(1000_i64),
    ))
    .execute(connection)
    .expect("legacy session");
  diesel::insert_into(gateway_audits::table)
    .values((
      gateway_audits::request_id.eq("pending-migration"),
      gateway_audits::model.eq("local"),
      gateway_audits::status.eq(0_i16),
      gateway_audits::elapsed_ms.eq(0_i64),
      gateway_audits::policy_revision.eq("rules-v1"),
    ))
    .execute(connection)
    .expect("pending intent");
}

pub fn diesel_versions(connection: &mut SqliteConnection, versions: &[String]) {
  connection.setup().expect("Diesel setup");
  diesel::insert_into(__diesel_schema_migrations::table)
    .values(
      versions
        .iter()
        .map(|version| __diesel_schema_migrations::version.eq(version))
        .collect::<Vec<_>>(),
    )
    .execute(connection)
    .expect("Diesel versions");
}

#[derive(Debug, PartialEq)]
pub struct Snapshot {
  schema: Vec<(String, String, Option<String>)>,
  legacy: Option<Vec<i32>>,
  diesel: Option<Vec<String>>,
  credentials: Vec<(i32, String)>,
  sessions: Vec<(String, String, i64)>,
  audits: Vec<(String, i16)>,
}

pub fn snapshot(connection: &mut SqliteConnection) -> Snapshot {
  let schema = sqlite_schema::table
    .select((sqlite_schema::name, sqlite_schema::kind, sqlite_schema::sql))
    .order(sqlite_schema::name.asc())
    .load::<(String, String, Option<String>)>(connection)
    .expect("schema");
  let legacy = schema
    .iter()
    .any(|(name, _, _)| name == "gateway_migrations")
    .then(|| {
      gateway_migrations::table
        .select(gateway_migrations::version)
        .order(gateway_migrations::version.asc())
        .load(connection)
        .expect("legacy ledger")
    });
  let diesel = schema
    .iter()
    .any(|(name, _, _)| name == "__diesel_schema_migrations")
    .then(|| {
      __diesel_schema_migrations::table
        .select(__diesel_schema_migrations::version)
        .order(__diesel_schema_migrations::version.asc())
        .load(connection)
        .expect("Diesel ledger")
    });
  Snapshot {
    schema,
    legacy,
    diesel,
    credentials: gateway_admin_credentials::table
      .load(connection)
      .expect("credentials"),
    sessions: gateway_admin_sessions::table
      .load(connection)
      .expect("sessions"),
    audits: gateway_audits::table
      .select((gateway_audits::request_id, gateway_audits::status))
      .load(connection)
      .expect("audits"),
  }
}
