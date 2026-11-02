use crate::{
  Store, decode, event_tx,
  schema::{events, provider_profiles},
};
use agent_core::provider_profiles::ProviderProfile;
use anyhow::Result;
use diesel::prelude::*;

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
  pub fn provider_profiles(&self) -> Result<Vec<ProviderProfile>> {
    decode(
      provider_profiles::table
        .order(provider_profiles::id.asc())
        .limit(100)
        .select(provider_profiles::record)
        .load::<String>(&mut *self.lock()?)?,
    )
  }

  pub fn provider_profile(&self, id: &str) -> Result<Option<ProviderProfile>> {
    let record = provider_profiles::table
      .find(id)
      .select(provider_profiles::record)
      .first::<String>(&mut *self.lock()?)
      .optional()?;
    record
      .map(|record| serde_json::from_str(&record).map_err(Into::into))
      .transpose()
  }

  pub fn put_provider_profile(
    &self,
    profile: &ProviderProfile,
    expected_revision: u64,
  ) -> Result<ProviderProfile> {
    let revision = validate(profile, expected_revision)?;
    self.lock()?.immediate_transaction(|connection| {
      let actual = provider_profiles::table
        .find(&profile.id)
        .select(provider_profiles::revision)
        .first::<i64>(connection)
        .optional()?;
      let record = serde_json::to_string(profile)?;
      match actual {
        Some(actual) if u64::try_from(actual)? == expected_revision => {
          diesel::update(
            provider_profiles::table
              .find(&profile.id)
              .filter(provider_profiles::revision.eq(actual)),
          )
          .set((
            provider_profiles::revision.eq(revision),
            provider_profiles::record.eq(&record),
          ))
          .execute(connection)?;
        }
        Some(_) => return Err(ProviderProfileError::Conflict.into()),
        None if expected_revision == 0 => {
          // Retained history fences deleted identities against revision reuse (ABA).
          if events::table
            .filter(events::run_id.eq(format!("provider:{}", profile.id)))
            .count()
            .get_result::<i64>(connection)?
            > 0
          {
            return Err(ProviderProfileError::Conflict.into());
          }
          if provider_profiles::table
            .count()
            .get_result::<i64>(connection)?
            >= 100
          {
            return Err(ProviderProfileError::Capacity.into());
          }
          diesel::insert_into(provider_profiles::table)
            .values((
              provider_profiles::id.eq(&profile.id),
              provider_profiles::revision.eq(revision),
              provider_profiles::record.eq(&record),
            ))
            .execute(connection)?;
        }
        None => return Err(ProviderProfileError::NotFound.into()),
      }
      event_tx(
        connection,
        &format!("provider:{}", profile.id),
        "provider_profile_changed",
        serde_json::json!({"id": profile.id, "revision": profile.revision}),
      )?;
      Ok(profile.clone())
    })
  }

  /// Removes configuration only; immutable secrets and pinned model bindings survive.
  pub fn remove_provider_profile(&self, id: &str, expected_revision: u64) -> Result<()> {
    self.lock()?.immediate_transaction(|connection| {
      let actual = provider_profiles::table
        .find(id)
        .select(provider_profiles::revision)
        .first::<i64>(connection)
        .optional()?
        .ok_or(ProviderProfileError::NotFound)?;
      if u64::try_from(actual)? != expected_revision {
        return Err(ProviderProfileError::Conflict.into());
      }
      diesel::delete(
        provider_profiles::table
          .find(id)
          .filter(provider_profiles::revision.eq(actual)),
      )
      .execute(connection)?;
      event_tx(
        connection,
        &format!("provider:{id}"),
        "provider_profile_removed",
        serde_json::json!({"id": id, "revision": expected_revision}),
      )?;
      Ok(())
    })
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
