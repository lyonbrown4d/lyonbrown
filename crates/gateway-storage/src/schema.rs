pub(crate) mod gateway_audits {
  use sea_orm::entity::prelude::*;
  #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
  #[sea_orm(table_name = "gateway_audits")]
  pub struct Model {
    #[sea_orm(primary_key, auto_increment = true)]
    pub sequence: i64,
    pub request_id: String,
    pub model: String,
    pub status: i16,
    pub elapsed_ms: i64,
    pub policy_revision: String,
    pub principal: Option<String>,
    pub reserved_tokens: i64,
    pub usage: Option<String>,
  }
  #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
  pub enum Relation {}
  impl ActiveModelBehavior for ActiveModel {}
}
pub(crate) mod gateway_model_snapshots {
  use sea_orm::entity::prelude::*;
  #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
  #[sea_orm(table_name = "gateway_model_snapshots")]
  pub struct Model {
    #[sea_orm(primary_key, auto_increment = true)]
    pub sequence: i64,
    pub revision: String,
    pub document: String,
  }
  #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
  pub enum Relation {}
  impl ActiveModelBehavior for ActiveModel {}
}
pub(crate) mod gateway_policy_revisions {
  use sea_orm::entity::prelude::*;
  #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
  #[sea_orm(table_name = "gateway_policy_revisions")]
  pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub revision: String,
    pub document: String,
  }
  #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
  pub enum Relation {}
  impl ActiveModelBehavior for ActiveModel {}
}
pub(crate) mod gateway_policy_head {
  use sea_orm::entity::prelude::*;
  #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
  #[sea_orm(table_name = "gateway_policy_head")]
  pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: i32,
    pub revision: String,
  }
  #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
  pub enum Relation {}
  impl ActiveModelBehavior for ActiveModel {}
}
pub(crate) mod gateway_model_head {
  use sea_orm::entity::prelude::*;
  #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
  #[sea_orm(table_name = "gateway_model_head")]
  pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: i32,
    pub revision: String,
  }
  #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
  pub enum Relation {}
  impl ActiveModelBehavior for ActiveModel {}
}
pub(crate) mod gateway_admin_accounts {
  use sea_orm::entity::prelude::*;
  #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
  #[sea_orm(table_name = "gateway_admin_accounts")]
  pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: i32,
    pub username: String,
    pub password_hash: String,
    pub role: i16,
    pub credential_revision: String,
  }
  #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
  pub enum Relation {}
  impl ActiveModelBehavior for ActiveModel {}
}
pub(crate) mod gateway_admin_sessions {
  use sea_orm::entity::prelude::*;
  #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
  #[sea_orm(table_name = "gateway_admin_sessions")]
  pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub token_hash: String,
    pub csrf_hash: String,
    pub expires_at: i64,
    pub credential_revision: String,
  }
  #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
  pub enum Relation {}
  impl ActiveModelBehavior for ActiveModel {}
}
pub(crate) mod gateway_access_tokens {
  use sea_orm::entity::prelude::*;
  #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
  #[sea_orm(table_name = "gateway_access_tokens")]
  pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    pub label: String,
    pub token_hash: String,
    pub created_at: i64,
    pub expires_at: i64,
    pub revoked: bool,
    pub project_id: String,
    pub allowed_models: String,
    pub max_output_tokens: Option<i64>,
    pub token_budget: Option<i64>,
    pub tokens_charged: i64,
  }
  #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
  pub enum Relation {}
  impl ActiveModelBehavior for ActiveModel {}
}
