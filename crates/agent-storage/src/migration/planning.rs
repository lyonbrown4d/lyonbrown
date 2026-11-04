//! Current planning schema; application transactions own relationships.
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Planning;
#[async_trait::async_trait]
impl MigrationTrait for Planning {
  async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    manager
      .create_table(
        Table::create()
          .table(Alias::new("plans"))
          .col(
            ColumnDef::new(Alias::new("id"))
              .string_len(36)
              .not_null()
              .primary_key()
              .comment("计划 UUID"),
          )
          .col(
            ColumnDef::new(Alias::new("session_id"))
              .string_len(256)
              .not_null()
              .comment("服务端绑定的会话"),
          )
          .col(
            ColumnDef::new(Alias::new("run_id"))
              .string_len(256)
              .not_null()
              .comment("服务端绑定的运行"),
          )
          .col(
            ColumnDef::new(Alias::new("revision"))
              .big_integer()
              .not_null()
              .comment("CAS 修订号"),
          )
          .col(
            super::large_text("record", manager.get_database_backend())
              .comment("权威计划、步骤和宿主验证记录"),
          )
          .to_owned(),
      )
      .await?;
    manager
      .create_index(
        Index::create()
          .name("plans_session_cursor")
          .table(Alias::new("plans"))
          .col(Alias::new("session_id"))
          .col(Alias::new("id"))
          .to_owned(),
      )
      .await?;
    manager
      .create_index(
        Index::create()
          .name("plans_run_cursor")
          .table(Alias::new("plans"))
          .col(Alias::new("session_id"))
          .col(Alias::new("run_id"))
          .col(Alias::new("id"))
          .to_owned(),
      )
      .await?;
    manager
      .create_table(
        Table::create()
          .table(Alias::new("plan_requests"))
          .col(
            ColumnDef::new(Alias::new("session_id"))
              .string_len(256)
              .not_null()
              .comment("请求所属会话"),
          )
          .col(
            ColumnDef::new(Alias::new("request_id"))
              .string_len(36)
              .not_null()
              .comment("稳定幂等请求 UUID"),
          )
          .col(
            ColumnDef::new(Alias::new("digest"))
              .string_len(64)
              .not_null()
              .comment("请求内容 SHA-256"),
          )
          .col(
            super::large_text("result", manager.get_database_backend())
              .comment("首次成功提交的结果快照"),
          )
          .primary_key(
            Index::create()
              .col(Alias::new("session_id"))
              .col(Alias::new("request_id")),
          )
          .to_owned(),
      )
      .await?;
    Ok(())
  }
  async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    manager
      .drop_table(Table::drop().table(Alias::new("plan_requests")).to_owned())
      .await?;
    manager
      .drop_table(Table::drop().table(Alias::new("plans")).to_owned())
      .await?;
    Ok(())
  }
}
