#![allow(dead_code)]
pub mod approvals {
  use sea_orm::entity::prelude::*;
  #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
  #[sea_orm(table_name = "approvals")]
  pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    #[sea_orm(column_type = "String(StringLen::N(256))")]
    pub id: String,
    #[sea_orm(column_type = "String(StringLen::N(256))")]
    pub run_id: String,
    #[sea_orm(column_type = "String(StringLen::N(256))")]
    pub status: String,
    #[sea_orm(column_type = "Text")]
    pub record: String,
  }
  #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
  pub enum Relation {}
  impl ActiveModelBehavior for ActiveModel {}
}
pub mod browser_actions {
  use sea_orm::entity::prelude::*;
  #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
  #[sea_orm(table_name = "browser_actions")]
  pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    #[sea_orm(column_type = "String(StringLen::N(256))")]
    pub action_id: String,
    #[sea_orm(column_type = "String(StringLen::N(256))")]
    pub session_id: String,
    pub owner_epoch: i64,
    #[sea_orm(column_type = "String(StringLen::N(256))")]
    pub kind: String,
    #[sea_orm(column_type = "Text")]
    pub arguments: String,
    #[sea_orm(column_type = "String(StringLen::N(256))")]
    pub status: String,
    #[sea_orm(column_type = "Text")]
    pub result: String,
  }
  #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
  pub enum Relation {}
  impl ActiveModelBehavior for ActiveModel {}
}
