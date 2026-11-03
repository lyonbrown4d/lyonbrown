use crate::{Store, schema::private_scopes, verify_epoch};
use anyhow::{Result, ensure};
use sea_orm::{EntityTrait, Set, sea_query::OnConflict};

impl Store {
  /// Returns encrypted mapping bytes only; decryption belongs to the host.
  pub async fn load_private_scope(&self, session: &str) -> Result<Option<Vec<u8>>> {
    Ok(
      private_scopes::Entity::find_by_id(session)
        .one(self.connection())
        .await?
        .map(|row| row.ciphertext),
    )
  }

  /// The write fence and owner epoch are checked in the same transaction as the upsert.
  pub async fn commit_private_scope(
    &self,
    session: &str,
    epoch: u64,
    ciphertext: &[u8],
  ) -> Result<()> {
    ensure!(
      !ciphertext.is_empty() && ciphertext.len() <= 65_536,
      "invalid private scope ciphertext"
    );
    let transaction = self.begin_write().await?;
    let result: Result<()> = async {
      verify_epoch(&transaction, session, epoch).await?;
      private_scopes::Entity::insert(private_scopes::ActiveModel {
        session_id: Set(session.to_owned()),
        ciphertext: Set(ciphertext.to_vec()),
      })
      .on_conflict(
        OnConflict::column(private_scopes::Column::SessionId)
          .update_column(private_scopes::Column::Ciphertext)
          .to_owned(),
      )
      .exec_without_returning(&transaction)
      .await?;
      Ok(())
    }
    .await;
    match result {
      Ok(()) => {
        transaction.commit().await?;
        Ok(())
      }
      Err(error) => {
        transaction.rollback().await?;
        Err(error)
      }
    }
  }
}
