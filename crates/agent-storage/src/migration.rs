use sea_orm::sea_query::ExprTrait;
mod configuration;
mod memory;
mod memory_projects;
mod sessions;
mod tasks;
use crate::schema;
use sea_orm::{ActiveModelTrait, Set};
use sea_orm_migration::prelude::*;
pub struct Migrator;
#[async_trait::async_trait]
impl MigratorTrait for Migrator {
  fn migrations() -> Vec<Box<dyn MigrationTrait>> {
    vec![
      Box::new(Initial),
      Box::new(memory::Memory),
      Box::new(memory_projects::MemoryProjects),
    ]
  }
}
#[derive(DeriveMigrationName)]
struct Initial;
#[async_trait::async_trait]
impl MigrationTrait for Initial {
  async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    let backend = manager.get_database_backend();
    sessions::create(manager).await?;
    tasks::create(manager).await?;
    configuration::create(manager).await?;
    manager
      .create_index(
        Index::create()
          .name("runs_identity")
          .table(Alias::new("runs"))
          .unique()
          .col(Alias::new("session_id"))
          .col(Alias::new("idempotency_key"))
          .to_owned(),
      )
      .await?;
    manager
      .create_index(
        Index::create()
          .name("tasks_identity")
          .table(Alias::new("tasks"))
          .unique()
          .col(Alias::new("idempotency_key"))
          .to_owned(),
      )
      .await?;
    manager
      .create_index(
        Index::create()
          .name("run_recoveries_identity")
          .table(Alias::new("run_recoveries"))
          .unique()
          .col(Alias::new("idempotency_key"))
          .to_owned(),
      )
      .await?;
    // MySQL has no partial indexes; the write-fence row protects its admission path.
    if backend != sea_orm::DbBackend::MySql {
      manager
        .create_index(
          Index::create()
            .name("one_active_run")
            .table(Alias::new("runs"))
            .col(Alias::new("session_id"))
            .unique()
            .and_where(Expr::col(Alias::new("status")).is_not_in([
              "completed",
              "failed",
              "cancelled",
            ]))
            .to_owned(),
        )
        .await?;
    }
    // Every compound write first updates this row, serializing cross-process admission,
    // epoch changes, event allocation, and approval consumption on all three backends.
    schema::write_lock::ActiveModel {
      id: Set(1),
      revision: Set(0),
    }
    .insert(manager.get_connection())
    .await?;
    Ok(())
  }
  async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    manager
      .drop_table(Table::drop().table(Alias::new("write_lock")).to_owned())
      .await?;
    manager
      .drop_table(
        Table::drop()
          .table(Alias::new("control_directory"))
          .to_owned(),
      )
      .await?;
    manager
      .drop_table(Table::drop().table(Alias::new("control_links")).to_owned())
      .await?;
    manager
      .drop_table(Table::drop().table(Alias::new("run_recoveries")).to_owned())
      .await?;
    manager
      .drop_table(Table::drop().table(Alias::new("operations")).to_owned())
      .await?;
    manager
      .drop_table(
        Table::drop()
          .table(Alias::new("browser_actions"))
          .to_owned(),
      )
      .await?;
    manager
      .drop_table(
        Table::drop()
          .table(Alias::new("provider_profiles"))
          .to_owned(),
      )
      .await?;
    manager
      .drop_table(Table::drop().table(Alias::new("model_jobs")).to_owned())
      .await?;
    manager
      .drop_table(Table::drop().table(Alias::new("model_bindings")).to_owned())
      .await?;
    manager
      .drop_table(Table::drop().table(Alias::new("peers")).to_owned())
      .await?;
    manager
      .drop_table(Table::drop().table(Alias::new("tasks")).to_owned())
      .await?;
    manager
      .drop_table(Table::drop().table(Alias::new("approvals")).to_owned())
      .await?;
    manager
      .drop_table(Table::drop().table(Alias::new("outbox")).to_owned())
      .await?;
    manager
      .drop_table(Table::drop().table(Alias::new("events")).to_owned())
      .await?;
    manager
      .drop_table(Table::drop().table(Alias::new("runs")).to_owned())
      .await?;
    manager
      .drop_table(
        Table::drop()
          .table(Alias::new("harness_sessions"))
          .to_owned(),
      )
      .await?;
    manager
      .drop_table(Table::drop().table(Alias::new("sessions")).to_owned())
      .await?;
    Ok(())
  }
}
// MySQL TEXT/BLOB stop at 64 KiB; snapshots and JSON records can exceed that limit.
fn large_text(name: &str, backend: sea_orm::DbBackend) -> ColumnDef {
  let mut column = ColumnDef::new(Alias::new(name));
  if backend == sea_orm::DbBackend::MySql {
    column.custom(Alias::new("longtext"));
  } else {
    column.text();
  }
  column.not_null().take()
}
fn snapshot_column(backend: sea_orm::DbBackend) -> ColumnDef {
  let mut column = ColumnDef::new(Alias::new("snapshot"));
  if backend == sea_orm::DbBackend::MySql {
    column.custom(Alias::new("longblob"));
  } else {
    column.blob();
  }
  column.not_null().take()
}
