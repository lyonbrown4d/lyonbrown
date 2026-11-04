use sea_orm_migration::prelude::*;
#[derive(DeriveMigrationName)]
pub struct ContextMaintenance;
#[async_trait::async_trait]
impl MigrationTrait for ContextMaintenance {
  async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    manager
      .create_table(
        Table::create()
          .table(Alias::new("context_maintenance_jobs"))
          .col(
            ColumnDef::new(Alias::new("id"))
              .string_len(36)
              .not_null()
              .primary_key()
              .comment("宿主维护作业 UUID"),
          )
          .col(
            ColumnDef::new(Alias::new("session_id"))
              .string_len(256)
              .not_null()
              .comment("独占维护的会话"),
          )
          .col(
            ColumnDef::new(Alias::new("state"))
              .small_integer()
              .not_null()
              .comment("0准备 1发送 2完成 3失败 4取消 5不确定 6已放弃旧会话"),
          )
          .col(
            super::large_text("record", manager.get_database_backend())
              .comment("安全作业元数据，不含原文或摘要"),
          )
          .to_owned(),
      )
      .await?;
    manager
      .create_index(
        Index::create()
          .name("context_maintenance_session_state")
          .table(Alias::new("context_maintenance_jobs"))
          .col(Alias::new("session_id"))
          .col(Alias::new("state"))
          .col(Alias::new("id"))
          .to_owned(),
      )
      .await?;
    Ok(())
  }
  async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    manager
      .drop_table(
        Table::drop()
          .table(Alias::new("context_maintenance_jobs"))
          .to_owned(),
      )
      .await
  }
}
