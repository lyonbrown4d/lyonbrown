mod m0001_initial;
use sea_orm_migration::prelude::*;
pub(crate) struct Migrator;
#[async_trait::async_trait]
impl MigratorTrait for Migrator {
  fn migrations() -> Vec<Box<dyn MigrationTrait>> {
    vec![Box::new(m0001_initial::Migration)]
  }
}
