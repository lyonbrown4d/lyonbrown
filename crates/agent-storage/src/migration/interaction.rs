use sea_orm_migration::prelude::*;
#[derive(DeriveMigrationName)]
pub struct Interaction;
#[async_trait::async_trait]
impl MigrationTrait for Interaction {
  async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    manager
      .create_table(
        Table::create()
          .table(Alias::new("interaction_policies"))
          .col(
            ColumnDef::new(Alias::new("scope_id"))
              .string_len(256)
              .not_null()
              .primary_key(),
          )
          .col(super::large_text("record", manager.get_database_backend()))
          .to_owned(),
      )
      .await
  }
  async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    manager
      .drop_table(
        Table::drop()
          .table(Alias::new("interaction_policies"))
          .to_owned(),
      )
      .await
  }
}
