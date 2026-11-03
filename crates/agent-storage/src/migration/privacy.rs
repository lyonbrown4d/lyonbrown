use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub(super) struct PrivacyScopes;

#[async_trait::async_trait]
impl MigrationTrait for PrivacyScopes {
  async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    let mut ciphertext = ColumnDef::new(Alias::new("ciphertext"));
    if manager.get_database_backend() == sea_orm::DbBackend::MySql {
      ciphertext.custom(Alias::new("longblob"));
    } else {
      ciphertext.blob();
    }
    manager
      .create_table(
        Table::create()
          .table(Alias::new("private_scopes"))
          .col(
            ColumnDef::new(Alias::new("session_id"))
              .string_len(256)
              .not_null()
              .primary_key(),
          )
          .col(ciphertext.not_null().take())
          .to_owned(),
      )
      .await
  }
  async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    manager
      .drop_table(Table::drop().table(Alias::new("private_scopes")).to_owned())
      .await
  }
}
