use crate::schema::*;
use sea_orm::{Schema, sea_query::ExprTrait};
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub(super) struct Migration;
#[async_trait::async_trait]
impl MigrationTrait for Migration {
  fn use_transaction(&self) -> Option<bool> {
    Some(true)
  }
  async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    let schema = Schema::new(manager.get_database_backend());
    let mut table = schema.create_table_from_entity(gateway_audits::Entity);
    table.check(Expr::col(Alias::new("reserved_tokens")).gte(0));
    manager.create_table(table).await?;
    let table = schema.create_table_from_entity(gateway_attempts::Entity);
    manager.create_table(table).await?;
    manager
      .create_index(
        Index::create()
          .name("gateway_attempt_identity")
          .table(gateway_attempts::Entity)
          .col(Alias::new("request_id"))
          .col(Alias::new("sequence"))
          .unique()
          .to_owned(),
      )
      .await?;
    let table = schema.create_table_from_entity(gateway_model_snapshots::Entity);
    manager.create_table(table).await?;
    let table = schema.create_table_from_entity(gateway_policy_revisions::Entity);
    manager.create_table(table).await?;
    let table = schema.create_table_from_entity(gateway_policy_head::Entity);
    manager.create_table(table).await?;
    let table = schema.create_table_from_entity(gateway_model_head::Entity);
    manager.create_table(table).await?;
    let mut table = schema.create_table_from_entity(gateway_access_tokens::Entity);
    table.check(Expr::col(Alias::new("tokens_charged")).gte(0));
    table.check(Expr::col(Alias::new("max_output_tokens")).gte(0));
    table.check(Expr::col(Alias::new("token_budget")).gte(0));
    manager.create_table(table).await?;
    manager
      .create_index(
        Index::create()
          .name("gateway_audits_request_id_unique")
          .table(gateway_audits::Entity)
          .col(Alias::new("request_id"))
          .unique()
          .to_owned(),
      )
      .await?;
    manager
      .create_index(
        Index::create()
          .name("gateway_model_snapshots_revision_unique")
          .table(gateway_model_snapshots::Entity)
          .col(Alias::new("revision"))
          .unique()
          .to_owned(),
      )
      .await?;
    manager
      .create_index(
        Index::create()
          .name("gateway_access_tokens_token_hash_unique")
          .table(gateway_access_tokens::Entity)
          .col(Alias::new("token_hash"))
          .unique()
          .to_owned(),
      )
      .await?;
    manager
      .create_index(
        Index::create()
          .name("gateway_access_tokens_expiry")
          .table(gateway_access_tokens::Entity)
          .col(Alias::new("expires_at"))
          .to_owned(),
      )
      .await?;
    Ok(())
  }
  async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    manager
      .drop_table(
        Table::drop()
          .table(gateway_access_tokens::Entity)
          .to_owned(),
      )
      .await?;
    manager
      .drop_table(Table::drop().table(gateway_model_head::Entity).to_owned())
      .await?;
    manager
      .drop_table(Table::drop().table(gateway_policy_head::Entity).to_owned())
      .await?;
    manager
      .drop_table(
        Table::drop()
          .table(gateway_policy_revisions::Entity)
          .to_owned(),
      )
      .await?;
    manager
      .drop_table(
        Table::drop()
          .table(gateway_model_snapshots::Entity)
          .to_owned(),
      )
      .await?;
    manager
      .drop_table(Table::drop().table(gateway_attempts::Entity).to_owned())
      .await?;
    manager
      .drop_table(Table::drop().table(gateway_audits::Entity).to_owned())
      .await?;
    Ok(())
  }
}
