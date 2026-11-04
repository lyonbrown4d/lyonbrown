use sea_orm::entity::prelude::*;
#[derive(Clone, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "run_media")]
pub struct Model {
  #[sea_orm(primary_key, auto_increment = false)]
  pub run_id: String,
  #[sea_orm(primary_key, auto_increment = false)]
  pub ordinal: i32,
  pub mime_type: String,
  pub sha256: String,
  pub data: Vec<u8>,
}
impl std::fmt::Debug for Model {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.debug_struct("RunMedia")
      .field("run_id", &self.run_id)
      .field("ordinal", &self.ordinal)
      .field("mime_type", &self.mime_type)
      .field("bytes", &self.data.len())
      .finish_non_exhaustive()
  }
}
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}
impl ActiveModelBehavior for ActiveModel {}
