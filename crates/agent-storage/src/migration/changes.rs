use super::large_text;
use sea_orm_migration::prelude::*;
#[derive(DeriveMigrationName)]
pub(super) struct Changes;
#[async_trait::async_trait]
impl MigrationTrait for Changes {
  async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    let backend = manager.get_database_backend();
    manager
      .create_table(
        Table::create()
          .table(Alias::new("change_sets"))
          .col(
            ColumnDef::new(Alias::new("id"))
              .string_len(256)
              .not_null()
              .primary_key(),
          )
          .col(
            ColumnDef::new(Alias::new("run_id"))
              .string_len(256)
              .not_null(),
          )
          .col(
            ColumnDef::new(Alias::new("revision"))
              .big_integer()
              .not_null(),
          )
          .col(
            ColumnDef::new(Alias::new("state"))
              .string_len(32)
              .not_null(),
          )
          .col(large_text("record", backend))
          .to_owned(),
      )
      .await?;
    manager
      .create_table(
        Table::create()
          .table(Alias::new("change_requests"))
          .col(
            ColumnDef::new(Alias::new("id"))
              .string_len(256)
              .not_null()
              .primary_key(),
          )
          .col(
            ColumnDef::new(Alias::new("body_hash"))
              .string_len(64)
              .not_null(),
          )
          .col(large_text("record", backend))
          .to_owned(),
      )
      .await?;
    manager
      .create_table(
        Table::create()
          .table(Alias::new("change_validations"))
          .col(
            ColumnDef::new(Alias::new("id"))
              .string_len(256)
              .not_null()
              .primary_key(),
          )
          .col(
            ColumnDef::new(Alias::new("change_id"))
              .string_len(256)
              .not_null(),
          )
          .col(large_text("record", backend))
          .to_owned(),
      )
      .await?;
    manager
      .create_table(
        Table::create()
          .table(Alias::new("change_user_approvals"))
          .col(
            ColumnDef::new(Alias::new("action_id"))
              .string_len(256)
              .not_null()
              .primary_key(),
          )
          .col(
            ColumnDef::new(Alias::new("request_id"))
              .string_len(256)
              .not_null(),
          )
          .col(
            ColumnDef::new(Alias::new("session_id"))
              .string_len(256)
              .not_null(),
          )
          .col(
            ColumnDef::new(Alias::new("owner_epoch"))
              .big_integer()
              .not_null(),
          )
          .col(
            ColumnDef::new(Alias::new("body_hash"))
              .string_len(64)
              .not_null(),
          )
          .col(large_text("record", backend))
          .to_owned(),
      )
      .await?;
    manager
      .create_index(
        Index::create()
          .name("change_sets_run_state")
          .table(Alias::new("change_sets"))
          .col(Alias::new("run_id"))
          .col(Alias::new("state"))
          .to_owned(),
      )
      .await?;
    Ok(())
  }
  async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    for table in [
      "change_user_approvals",
      "change_validations",
      "change_requests",
      "change_sets",
    ] {
      manager
        .drop_table(Table::drop().table(Alias::new(table)).to_owned())
        .await?;
    }
    Ok(())
  }
}
