use crate::{
  Store,
  schema::{control_directory, control_links},
};
use anyhow::{Result, ensure};
use sea_orm::{Set, prelude::*, sea_query::OnConflict};
impl Store {
  pub async fn control_replace_links(&self, links: &[(String, String)]) -> Result<()> {
    ensure!(
      links
        .iter()
        .all(|(from, to)| !from.is_empty() && !to.is_empty() && from != to),
      "invalid control link"
    );
    {
      let transaction = self.begin_write().await?;
      let connection = &transaction;
      let result: anyhow::Result<_> = async {
        control_links::Entity::delete_many()
          .exec(connection)
          .await?;
        for (from, to) in links {
          control_links::Entity::insert(control_links::ActiveModel {
            source: Set(from.clone()),
            target: Set(to.clone()),
          })
          .on_conflict(
            OnConflict::columns([control_links::Column::Source, control_links::Column::Target])
              .update_column(control_links::Column::Target)
              .to_owned(),
          )
          .exec_without_returning(connection)
          .await?;
        }
        Ok(())
      }
      .await;
      match result {
        Ok(value) => {
          transaction.commit().await?;
          Ok::<_, anyhow::Error>(value)
        }
        Err(error) => {
          transaction.rollback().await?;
          Err(error)
        }
      }
    }
  }
  pub async fn control_register(&self, id: &str, encoded_record: &str, expires: i64) -> Result<()> {
    control_directory::Entity::insert(control_directory::ActiveModel {
      node_id: Set(id.into()),
      record: Set(encoded_record.into()),
      expires_at_ms: Set(expires),
    })
    .on_conflict(
      OnConflict::column(control_directory::Column::NodeId)
        .update_columns([
          control_directory::Column::Record,
          control_directory::Column::ExpiresAtMs,
        ])
        .to_owned(),
    )
    .exec_without_returning(self.connection())
    .await?;
    Ok(())
  }
  pub async fn control_resolve(&self, id: &str, now: i64) -> Result<Option<String>> {
    Ok(
      control_directory::Entity::find_by_id(id)
        .filter(control_directory::Column::ExpiresAtMs.gt(now))
        .one(self.connection())
        .await?
        .map(|row| row.record),
    )
  }
  pub async fn control_allow_link(&self, from: &str, to: &str, allowed: bool) -> Result<()> {
    ensure!(
      !from.is_empty() && !to.is_empty() && from != to,
      "invalid control link"
    );
    if allowed {
      // Upsert is portable even when the link already exists.
      control_links::Entity::insert(control_links::ActiveModel {
        source: Set(from.into()),
        target: Set(to.into()),
      })
      .on_conflict(
        OnConflict::columns([control_links::Column::Source, control_links::Column::Target])
          .update_column(control_links::Column::Target)
          .to_owned(),
      )
      .exec_without_returning(self.connection())
      .await?;
    } else {
      control_links::Entity::delete_by_id((from.to_owned(), to.to_owned()))
        .exec(self.connection())
        .await?;
    }
    Ok(())
  }
  pub async fn control_link_allowed(&self, from: &str, to: &str) -> Result<bool> {
    Ok(
      control_links::Entity::find_by_id((from.to_owned(), to.to_owned()))
        .one(self.connection())
        .await?
        .is_some(),
    )
  }
}
