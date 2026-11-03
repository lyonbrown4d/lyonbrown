use crate::{
  Store, decode, event_tx,
  schema::{events, provider_profiles},
};
use agent_core::provider_profiles::ProviderProfile;
use anyhow::Result;
use sea_orm::{QueryOrder, QuerySelect, Set, prelude::*};

/// Expected profile mutation failures, available through `anyhow::Error::downcast_ref`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProviderProfileError {
  Conflict,
  NotFound,
  Capacity,
  Invalid,
}

impl std::fmt::Display for ProviderProfileError {
  fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    formatter.write_str(match self {
      Self::Conflict => "provider profile revision conflict",
      Self::NotFound => "provider profile not found",
      Self::Capacity => "provider profile capacity reached",
      Self::Invalid => "invalid provider profile",
    })
  }
}
impl std::error::Error for ProviderProfileError {}

impl Store {
  pub async fn provider_profiles(&self) -> Result<Vec<ProviderProfile>> {
    decode(
      provider_profiles::Entity::find()
        .order_by_asc(provider_profiles::Column::Id)
        .limit(100_u64)
        .select_only()
        .column(provider_profiles::Column::Record)
        .into_tuple::<String>()
        .all(self.connection())
        .await?,
    )
  }

  pub async fn provider_profile(&self, id: &str) -> Result<Option<ProviderProfile>> {
    let record = provider_profiles::Entity::find()
      .filter(provider_profiles::Column::Id.eq(id))
      .select_only()
      .column(provider_profiles::Column::Record)
      .into_tuple::<String>()
      .one(self.connection())
      .await?;
    record
      .map(|record| serde_json::from_str(&record).map_err(Into::into))
      .transpose()
  }

  pub async fn put_provider_profile(
    &self,
    profile: &ProviderProfile,
    expected_revision: u64,
  ) -> Result<ProviderProfile> {
    let revision = validate(profile, expected_revision)?;
    {
      let transaction = self.begin_write().await?;
      let connection = &transaction;
      let result: anyhow::Result<_> = async {
        let actual = provider_profiles::Entity::find()
          .filter(provider_profiles::Column::Id.eq(&profile.id))
          .select_only()
          .column(provider_profiles::Column::Revision)
          .into_tuple::<i64>()
          .one(connection)
          .await?;
        let record = serde_json::to_string(profile)?;
        match actual {
          Some(actual) if u64::try_from(actual)? == expected_revision => {
            provider_profiles::Entity::update_many()
              .filter(provider_profiles::Column::Id.eq(&profile.id))
              .filter(provider_profiles::Column::Revision.eq(actual))
              .col_expr(
                provider_profiles::Column::Revision,
                Expr::value((revision).to_owned()),
              )
              .col_expr(
                provider_profiles::Column::Record,
                Expr::value(record.to_owned()),
              )
              .exec(connection)
              .await
              .map(|result| result.rows_affected)?;
          }
          Some(_) => return Err(ProviderProfileError::Conflict.into()),
          None if expected_revision == 0 => {
            // Retained history fences deleted identities against revision reuse (ABA).
            if events::Entity::find()
              .filter(events::Column::RunId.eq(format!("provider:{}", profile.id)))
              .count(connection)
              .await
              .map(|value| value as i64)?
              > 0
            {
              return Err(ProviderProfileError::Conflict.into());
            }
            if provider_profiles::Entity::find()
              .count(connection)
              .await
              .map(|value| value as i64)?
              >= 100
            {
              return Err(ProviderProfileError::Capacity.into());
            }
            provider_profiles::ActiveModel {
              id: Set(profile.id.to_owned()),
              revision: Set((revision).to_owned()),
              record: Set(record.to_owned()),
            }
            .insert(connection)
            .await?;
          }
          None => return Err(ProviderProfileError::NotFound.into()),
        }
        event_tx(
          connection,
          &format!("provider:{}", profile.id),
          "provider_profile_changed",
          serde_json::json!({"id": profile.id, "revision": profile.revision}),
        )
        .await?;
        Ok(profile.clone())
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

  /// Removes configuration only; immutable secrets and pinned model bindings survive.
  pub async fn remove_provider_profile(&self, id: &str, expected_revision: u64) -> Result<()> {
    {
      let transaction = self.begin_write().await?;
      let connection = &transaction;
      let result: anyhow::Result<_> = async {
        let actual = provider_profiles::Entity::find()
          .filter(provider_profiles::Column::Id.eq(id))
          .select_only()
          .column(provider_profiles::Column::Revision)
          .into_tuple::<i64>()
          .one(connection)
          .await?
          .ok_or(ProviderProfileError::NotFound)?;
        if u64::try_from(actual)? != expected_revision {
          return Err(ProviderProfileError::Conflict.into());
        }
        provider_profiles::Entity::delete_many()
          .filter(provider_profiles::Column::Id.eq(id))
          .filter(provider_profiles::Column::Revision.eq(actual))
          .exec(connection)
          .await
          .map(|result| result.rows_affected)?;
        event_tx(
          connection,
          &format!("provider:{id}"),
          "provider_profile_removed",
          serde_json::json!({"id": id, "revision": expected_revision}),
        )
        .await?;
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
}

fn validate(profile: &ProviderProfile, expected: u64) -> Result<i64> {
  let valid = !profile.id.trim().is_empty()
    && profile.id.len() <= 128
    && !profile.name.trim().is_empty()
    && profile.name.len() <= 256
    && !profile.base_url.trim().is_empty()
    && profile.base_url.len() <= 2048
    && !profile.model.trim().is_empty()
    && profile.model.len() <= 256
    && !profile.credential_id.trim().is_empty()
    && profile.credential_id.len() <= 256
    && expected.checked_add(1) == Some(profile.revision);
  if !valid {
    return Err(ProviderProfileError::Invalid.into());
  }
  i64::try_from(profile.revision).map_err(|_| ProviderProfileError::Invalid.into())
}
