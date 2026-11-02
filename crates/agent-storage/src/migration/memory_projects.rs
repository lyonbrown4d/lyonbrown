use super::large_text;
use sea_orm_migration::prelude::*;
#[derive(DeriveMigrationName)]
pub(super) struct MemoryProjects;
#[async_trait::async_trait]
impl MigrationTrait for MemoryProjects {
  async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    manager
      .create_table(
        Table::create()
          .table(Alias::new("memory_projects"))
          .col(
            ColumnDef::new(Alias::new("id"))
              .string_len(256)
              .not_null()
              .primary_key(),
          )
          .col(
            ColumnDef::new(Alias::new("owner_id"))
              .string_len(256)
              .not_null(),
          )
          .col(
            ColumnDef::new(Alias::new("workspace_hash"))
              .string_len(64)
              .not_null(),
          )
          .col(large_text("workspace", manager.get_database_backend()))
          .to_owned(),
      )
      .await?;
    manager
      .create_index(
        Index::create()
          .name("memory_project_owner_workspace")
          .table(Alias::new("memory_projects"))
          .col(Alias::new("owner_id"))
          .col(Alias::new("workspace_hash"))
          .unique()
          .to_owned(),
      )
      .await?;
    Ok(())
  }
  async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    manager
      .drop_table(
        Table::drop()
          .table(Alias::new("memory_projects"))
          .to_owned(),
      )
      .await?;
    Ok(())
  }
}
