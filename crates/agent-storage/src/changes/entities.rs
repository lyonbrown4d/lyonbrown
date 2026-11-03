pub(super) mod sets {
  use sea_orm::entity::prelude::*;
  #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
  #[sea_orm(table_name = "change_sets")]
  pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    pub run_id: String,
    pub revision: i64,
    pub state: String,
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
  #[sea_orm(table_name = "change_requests")]
  pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    pub body_hash: String,
    #[sea_orm(column_type = "Text")]
    pub record: String,
  }
  #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
  pub enum Relation {}
  impl ActiveModelBehavior for ActiveModel {}
}
pub(super) mod validations {
  use sea_orm::entity::prelude::*;
  #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
  #[sea_orm(table_name = "change_validations")]
  pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    pub change_id: String,
    #[sea_orm(column_type = "Text")]
    pub record: String,
  }
  #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
  pub enum Relation {}
  impl ActiveModelBehavior for ActiveModel {}
}
pub(super) mod user_approvals {
  use sea_orm::entity::prelude::*;
  #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
  #[sea_orm(table_name = "change_user_approvals")]
  pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub action_id: String,
    pub request_id: String,
    pub session_id: String,
    pub owner_epoch: i64,
    pub body_hash: String,
    #[sea_orm(column_type = "Text")]
    pub record: String,
  }
  #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
  pub enum Relation {}
  impl ActiveModelBehavior for ActiveModel {}
}
