//! Internal ORM entities; public interfaces use domain values only.
pub(crate) mod sessions {
  use sea_orm::entity::prelude::*;
  #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
  #[sea_orm(table_name = "sessions")]
  pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    #[sea_orm(column_type = "String(StringLen::N(256))")]
    pub id: String,
    pub owner_epoch: i64,
    #[sea_orm(column_type = "Text")]
    pub record: String,
  }
  #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
  pub enum Relation {}
  impl ActiveModelBehavior for ActiveModel {}
}
pub(crate) mod runtime_sessions {
  use sea_orm::entity::prelude::*;
  #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
  #[sea_orm(table_name = "runtime_sessions")]
  pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    #[sea_orm(column_type = "String(StringLen::N(256))")]
    pub session_id: String,
    #[sea_orm(column_type = "Blob")]
    pub snapshot: Vec<u8>,
  }
  #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
  pub enum Relation {}
  impl ActiveModelBehavior for ActiveModel {}
}
pub(crate) mod runs {
  use sea_orm::entity::prelude::*;
  #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
  #[sea_orm(table_name = "runs")]
  pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    #[sea_orm(column_type = "String(StringLen::N(256))")]
    pub id: String,
    #[sea_orm(column_type = "String(StringLen::N(256))")]
    pub session_id: String,
    #[sea_orm(column_type = "String(StringLen::N(256))")]
    pub idempotency_key: String,
    #[sea_orm(column_type = "String(StringLen::N(256))")]
    pub status: String,
    pub created_at_ms: i64,
    #[sea_orm(column_type = "Text")]
    pub record: String,
  }
  #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
  pub enum Relation {}
  impl ActiveModelBehavior for ActiveModel {}
}
pub(crate) mod events {
  use sea_orm::entity::prelude::*;
  #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
  #[sea_orm(table_name = "events")]
  pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    #[sea_orm(column_type = "String(StringLen::N(256))")]
    pub run_id: String,
    #[sea_orm(primary_key, auto_increment = false)]
    pub cursor: i64,
    #[sea_orm(column_type = "Text")]
    pub record: String,
  }
  #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
  pub enum Relation {}
  impl ActiveModelBehavior for ActiveModel {}
}
pub(crate) mod outbox {
  use sea_orm::entity::prelude::*;
  #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
  #[sea_orm(table_name = "outbox")]
  pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    #[sea_orm(column_type = "String(StringLen::N(256))")]
    pub run_id: String,
    #[sea_orm(primary_key, auto_increment = false)]
    pub cursor: i64,
    pub delivered: i32,
  }
  #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
  pub enum Relation {}
  impl ActiveModelBehavior for ActiveModel {}
}
pub(crate) mod approvals {
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
pub(crate) mod tasks {
  use sea_orm::entity::prelude::*;
  #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
  #[sea_orm(table_name = "tasks")]
  pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    #[sea_orm(column_type = "String(StringLen::N(256))")]
    pub id: String,
    #[sea_orm(column_type = "String(StringLen::N(256))")]
    pub idempotency_key: String,
    pub owner_epoch: i64,
    #[sea_orm(column_type = "Text")]
    pub record: String,
    pub session_id: String,
    pub owner_node_id: String,
    pub status: String,
    pub attempt_status: String,
  }
  #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
  pub enum Relation {}
  impl ActiveModelBehavior for ActiveModel {}
}
pub(crate) mod peers {
  use sea_orm::entity::prelude::*;
  #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
  #[sea_orm(table_name = "peers")]
  pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    #[sea_orm(column_type = "String(StringLen::N(256))")]
    pub node_id: String,
    #[sea_orm(column_type = "Text")]
    pub record: String,
  }
  #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
  pub enum Relation {}
  impl ActiveModelBehavior for ActiveModel {}
}
pub(crate) mod model_bindings {
  use sea_orm::entity::prelude::*;
  #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
  #[sea_orm(table_name = "model_bindings")]
  pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    #[sea_orm(column_type = "String(StringLen::N(256))")]
    pub scope_id: String,
    #[sea_orm(column_type = "Text")]
    pub record: String,
  }
  #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
  pub enum Relation {}
  impl ActiveModelBehavior for ActiveModel {}
}
pub(crate) mod model_jobs {
  use sea_orm::entity::prelude::*;
  #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
  #[sea_orm(table_name = "model_jobs")]
  pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    #[sea_orm(column_type = "String(StringLen::N(256))")]
    pub id: String,
    #[sea_orm(column_type = "Text")]
    pub record: String,
  }
  #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
  pub enum Relation {}
  impl ActiveModelBehavior for ActiveModel {}
}
pub(crate) mod provider_profiles {
  use sea_orm::entity::prelude::*;
  #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
  #[sea_orm(table_name = "provider_profiles")]
  pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    #[sea_orm(column_type = "String(StringLen::N(256))")]
    pub id: String,
    pub revision: i64,
    #[sea_orm(column_type = "Text")]
    pub record: String,
  }
  #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
  pub enum Relation {}
  impl ActiveModelBehavior for ActiveModel {}
}
pub(crate) mod browser_actions {
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
pub(crate) mod operations {
  use sea_orm::entity::prelude::*;
  #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
  #[sea_orm(table_name = "operations")]
  pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    #[sea_orm(column_type = "String(StringLen::N(256))")]
    pub action_id: String,
    #[sea_orm(column_type = "String(StringLen::N(256))")]
    pub run_id: String,
    pub owner_epoch: i64,
    #[sea_orm(column_type = "String(StringLen::N(256))")]
    pub status: String,
    #[sea_orm(column_type = "Text")]
    pub record: String,
  }
  #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
  pub enum Relation {}
  impl ActiveModelBehavior for ActiveModel {}
}
pub(crate) mod run_recoveries {
  use sea_orm::entity::prelude::*;
  #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
  #[sea_orm(table_name = "run_recoveries")]
  pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    #[sea_orm(column_type = "String(StringLen::N(256))")]
    pub run_id: String,
    #[sea_orm(column_type = "String(StringLen::N(256))")]
    pub idempotency_key: String,
    #[sea_orm(column_type = "Text")]
    pub record: String,
  }
  #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
  pub enum Relation {}
  impl ActiveModelBehavior for ActiveModel {}
}
pub(crate) mod control_links {
  use sea_orm::entity::prelude::*;
  #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
  #[sea_orm(table_name = "control_links")]
  pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    #[sea_orm(column_type = "String(StringLen::N(256))")]
    pub source: String,
    #[sea_orm(primary_key, auto_increment = false)]
    #[sea_orm(column_type = "String(StringLen::N(256))")]
    pub target: String,
  }
  #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
  pub enum Relation {}
  impl ActiveModelBehavior for ActiveModel {}
}
pub(crate) mod control_directory {
  use sea_orm::entity::prelude::*;
  #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
  #[sea_orm(table_name = "control_directory")]
  pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    #[sea_orm(column_type = "String(StringLen::N(256))")]
    pub node_id: String,
    #[sea_orm(column_type = "Text")]
    pub record: String,
    pub expires_at_ms: i64,
  }
  #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
  pub enum Relation {}
  impl ActiveModelBehavior for ActiveModel {}
}
pub(crate) mod write_lock {
  use sea_orm::entity::prelude::*;
  #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
  #[sea_orm(table_name = "write_lock")]
  pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: i32,
    pub revision: i64,
  }
  #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
  pub enum Relation {}
  impl ActiveModelBehavior for ActiveModel {}
}

pub(crate) mod private_scopes {
  use sea_orm::entity::prelude::*;
  #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
  #[sea_orm(table_name = "private_scopes")]
  pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    #[sea_orm(column_type = "String(StringLen::N(256))")]
    pub session_id: String,
    #[sea_orm(column_type = "Blob")]
    pub ciphertext: Vec<u8>,
  }
  #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
  pub enum Relation {}
  impl ActiveModelBehavior for ActiveModel {}
}
