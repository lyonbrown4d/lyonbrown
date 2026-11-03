//! Planning entities are deliberately isolated from the wire and domain types.
pub(super) mod plans {
  use sea_orm::entity::prelude::*;
  #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
  #[sea_orm(table_name = "plans")]
  pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    pub session_id: String,
    pub run_id: String,
    pub revision: i64,
    #[sea_orm(column_type = "Text")]
    pub record: String,
  }
  #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
  pub enum Relation {}
  impl ActiveModelBehavior for ActiveModel {}
}
pub(super) mod requests {
  use sea_orm::entity::prelude::*;
  #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
  #[sea_orm(table_name = "plan_requests")]
  pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub session_id: String,
    #[sea_orm(primary_key, auto_increment = false)]
    pub request_id: String,
    pub digest: String,
    #[sea_orm(column_type = "Text")]
    pub result: String,
  }
  #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
  pub enum Relation {}
  impl ActiveModelBehavior for ActiveModel {}
}
