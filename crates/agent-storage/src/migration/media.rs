use sea_orm_migration::prelude::*;
#[derive(DeriveMigrationName)]
pub(super) struct Media;
#[async_trait::async_trait]
impl MigrationTrait for Media {
  async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    let mut data = ColumnDef::new(Alias::new("data"));
    if manager.get_database_backend() == sea_orm::DbBackend::MySql {
      data.custom(Alias::new("longblob"));
    } else {
      data.blob();
    }
    data.not_null();
    manager
      .create_table(
        Table::create()
          .table(Alias::new("run_media"))
          .col(
            ColumnDef::new(Alias::new("run_id"))
              .string_len(256)
              .not_null(),
          )
          .col(ColumnDef::new(Alias::new("ordinal")).integer().not_null())
          .col(
            ColumnDef::new(Alias::new("mime_type"))
              .string_len(32)
              .not_null(),
          )
          .col(
            ColumnDef::new(Alias::new("sha256"))
              .string_len(64)
              .not_null(),
          )
          .col(&mut data)
          .primary_key(
            Index::create()
              .col(Alias::new("run_id"))
              .col(Alias::new("ordinal")),
          )
          .to_owned(),
      )
      .await?;
    Ok(())
  }
  async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    manager
      .drop_table(Table::drop().table(Alias::new("run_media")).to_owned())
      .await?;
    Ok(())
  }
}
