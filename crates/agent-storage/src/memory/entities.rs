//! Private memory persistence entities.
pub(super) mod items {
  use sea_orm::entity::prelude::*;
  #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
  #[sea_orm(table_name = "memory_items")]
  pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    #[sea_orm(column_type = "String(StringLen::N(256))")]
    pub id: String,
    #[sea_orm(column_type = "String(StringLen::N(256))")]
    pub scope_key: String,
    #[sea_orm(column_type = "String(StringLen::N(256))")]
    pub state: String,
    pub revision: i64,
    #[sea_orm(column_type = "Text")]
    pub record: String,
  }
  #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
  pub enum Relation {}
  impl ActiveModelBehavior for ActiveModel {}
}
pub(super) mod revisions {
  use sea_orm::entity::prelude::*;
  #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
  #[sea_orm(table_name = "memory_revisions")]
  pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    #[sea_orm(column_type = "String(StringLen::N(256))")]
    pub memory_id: String,
    #[sea_orm(primary_key, auto_increment = false)]
    pub revision: i64,
    #[sea_orm(column_type = "Text")]
    pub record: String,
    #[sea_orm(column_type = "Text")]
    pub source: String,
  }
  #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
  pub enum Relation {}
  impl ActiveModelBehavior for ActiveModel {}
}
pub(super) mod requests {
  use sea_orm::entity::prelude::*;
  #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
  #[sea_orm(table_name = "memory_requests")]
  pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    #[sea_orm(column_type = "String(StringLen::N(256))")]
    pub request_id: String,
    #[sea_orm(column_type = "String(StringLen::N(256))")]
    pub body_hash: String,
    #[sea_orm(column_type = "String(StringLen::N(256))")]
    pub memory_id: String,
    pub revision: i64,
  }
  #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
  pub enum Relation {}
  impl ActiveModelBehavior for ActiveModel {}
}
pub(super) mod jobs {
  use sea_orm::entity::prelude::*;
  #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
  #[sea_orm(table_name = "memory_jobs")]
  pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    #[sea_orm(column_type = "String(StringLen::N(256))")]
    pub id: String,
    #[sea_orm(column_type = "String(StringLen::N(256))")]
    pub memory_id: String,
    pub revision: i64,
    #[sea_orm(column_type = "String(StringLen::N(256))")]
    pub kind: String,
    #[sea_orm(column_type = "String(StringLen::N(256))")]
    pub state: String,
    pub attempts: i64,
    pub error_category: Option<String>,
    pub rebuild_id: Option<String>,
    pub descriptor_key: Option<String>,
  }
  #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
  pub enum Relation {}
  impl ActiveModelBehavior for ActiveModel {}
}
pub(super) mod bindings {
  use sea_orm::entity::prelude::*;
  #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
  #[sea_orm(table_name = "memory_run_bindings")]
  pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    #[sea_orm(column_type = "String(StringLen::N(256))")]
    pub run_id: String,
    #[sea_orm(primary_key, auto_increment = false)]
    #[sea_orm(column_type = "String(StringLen::N(256))")]
    pub memory_id: String,
    pub revision: i64,
    #[sea_orm(column_type = "String(StringLen::N(256))")]
    pub content_hash: String,
  }
  #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
  pub enum Relation {}
  impl ActiveModelBehavior for ActiveModel {}
}

pub(super) mod projects {
  use sea_orm::entity::prelude::*;
  #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
  #[sea_orm(table_name = "memory_projects")]
  pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    #[sea_orm(column_type = "String(StringLen::N(256))")]
    pub id: String,
    #[sea_orm(column_type = "String(StringLen::N(256))")]
    pub owner_id: String,
    #[sea_orm(column_type = "String(StringLen::N(64))")]
    pub workspace_hash: String,
    #[sea_orm(column_type = "Text")]
    pub workspace: String,
  }
  #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
  pub enum Relation {}
  impl ActiveModelBehavior for ActiveModel {}
}

pub(super) mod rebuilds {
  use sea_orm::entity::prelude::*;
  #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
  #[sea_orm(table_name = "memory_rebuilds")]
  pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    pub scope_key: String,
    pub descriptor_key: String,
    pub scan_finished: bool,
    pub generation: i64,
    /// 1 = Pending; 2 = Completed.
    pub state: i16,
    #[sea_orm(column_type = "Text")]
    pub record: String,
  }
  #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
  pub enum Relation {}
  impl ActiveModelBehavior for ActiveModel {}
}
