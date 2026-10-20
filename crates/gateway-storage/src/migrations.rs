use crate::{Result, StorageError, schema};
use diesel::{
  connection::SimpleConnection,
  migration::{Migration, MigrationConnection, MigrationSource},
  prelude::*,
  sqlite::Sqlite,
};
use diesel_migrations::{EmbeddedMigrations, MigrationHarness, embed_migrations};

const MIGRATIONS: EmbeddedMigrations = embed_migrations!("migrations");
const LEGACY_NAMES: [&str; 2] = ["0001_initial", "0002_super_admin"];

/// Called inside the startup immediate transaction, including audit reconciliation.
pub(crate) fn run(connection: &mut SqliteConnection) -> Result<()> {
  let known = known_versions()?;
  let tables = schema::sqlite_schema::table
    .filter(schema::sqlite_schema::kind.eq("table"))
    .select(schema::sqlite_schema::name)
    .load::<String>(connection)?;
  let legacy = if tables.iter().any(|name| name == "gateway_migrations") {
    let versions = schema::gateway_migrations::table
      .select(schema::gateway_migrations::version)
      .order(schema::gateway_migrations::version.asc())
      .load::<i32>(connection)?;
    if !matches!(versions.as_slice(), [1] | [1, 2]) {
      return Err(StorageError::InvalidData);
    }
    Some(versions.len())
  } else {
    None
  };
  connection.setup()?;
  let mut applied = connection
    .applied_migrations()
    .map_err(StorageError::Migration)?
    .into_iter()
    .map(|version| version.to_string())
    .collect::<Vec<_>>();
  applied.sort();
  if !known.starts_with(&applied) {
    return Err(StorageError::InvalidData);
  }
  match legacy {
    Some(count) if applied.is_empty() => {
      // Version identifiers come from the public embedded source, never an integer cast.
      let rows = known[..count]
        .iter()
        .map(|version| schema::__diesel_schema_migrations::version.eq(version))
        .collect::<Vec<_>>();
      diesel::insert_into(schema::__diesel_schema_migrations::table)
        .values(rows)
        .execute(connection)?;
    }
    Some(count) if count != applied.len() => return Err(StorageError::InvalidData),
    None if applied.is_empty() && tables.iter().any(|name| name.starts_with("gateway_")) => {
      // An untracked application schema must not replay the credential-revoking migration.
      return Err(StorageError::InvalidData);
    }
    _ => {}
  }
  connection
    .run_pending_migrations(MIGRATIONS)
    .map_err(StorageError::Migration)?;
  // The frozen initial SQL also creates this table on fresh installations. Retire it
  // atomically so Diesel is the only active migration ledger after startup succeeds.
  connection.batch_execute("DROP TABLE IF EXISTS gateway_migrations;")?;
  Ok(())
}

fn known_versions() -> Result<Vec<String>> {
  let mut migrations: Vec<Box<dyn Migration<Sqlite>>> =
    MIGRATIONS.migrations().map_err(StorageError::Migration)?;
  migrations.sort_by(|left, right| left.name().version().cmp(&right.name().version()));
  // Pin the legacy mapping to the immutable migration names while allowing later migrations.
  if migrations.len() < LEGACY_NAMES.len()
    || migrations
      .iter()
      .zip(LEGACY_NAMES)
      .any(|(migration, expected)| migration.name().to_string() != expected)
  {
    return Err(StorageError::InvalidData);
  }
  let versions = migrations
    .iter()
    .map(|migration| migration.name().version().to_string())
    .collect::<Vec<_>>();
  if versions.windows(2).any(|pair| pair[0] == pair[1]) {
    return Err(StorageError::InvalidData);
  }
  Ok(versions)
}
