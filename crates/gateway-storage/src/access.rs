use crate::{
  GatewayStorage, Result, StorageError,
  auth::{digest, equal, now_seconds, random_token, signed_time, valid_token},
  error::valid_key,
  schema::gateway_access_tokens as tokens,
};
use async_trait::async_trait;
use diesel::prelude::*;
use gateway_core::{AccessControl, AccessError};
use secrecy::{ExposeSecret, SecretString};
use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
pub struct AccessTokenMetadata {
  pub id: String,
  pub label: String,
  pub created_at: u64,
  pub expires_at: u64,
  pub revoked: bool,
}
#[derive(Debug)]
pub struct IssuedAccessToken {
  token: SecretString,
  pub metadata: AccessTokenMetadata,
}
impl IssuedAccessToken {
  pub fn token(&self) -> &str {
    self.token.expose_secret()
  }
}
impl GatewayStorage {
  pub async fn bootstrap_access_token(&self, token: &str) -> Result<()> {
    if !valid_token(token) {
      return Err(StorageError::InvalidInput("API bootstrap token"));
    }
    let token_hash = digest("access", token);
    let id = format!("bootstrap-{}", &digest("access-id", token)[..32]);
    let now = signed_time(now_seconds()?)?;
    self
      .run(move |connection| {
        // Revoked bootstrap keys stay revoked across restarts.
        diesel::insert_into(tokens::table)
          .values((
            tokens::id.eq(id),
            tokens::label.eq("bootstrap"),
            tokens::token_hash.eq(token_hash),
            tokens::created_at.eq(now),
            tokens::expires_at.eq(i64::MAX),
            tokens::revoked.eq(false),
          ))
          .on_conflict_do_nothing()
          .execute(connection)?;
        Ok(())
      })
      .await
  }
  pub async fn create_access_token(
    &self,
    label: &str,
    now: u64,
    ttl_seconds: u64,
  ) -> Result<IssuedAccessToken> {
    if !valid_key(label) || !(1..=31_536_000).contains(&ttl_seconds) {
      return Err(StorageError::InvalidInput("API key label or lifetime"));
    }
    let issued = IssuedAccessToken {
      token: random_token()?.into(),
      metadata: AccessTokenMetadata {
        id: random_token()?,
        label: label.into(),
        created_at: now,
        expires_at: now
          .checked_add(ttl_seconds)
          .ok_or(StorageError::InvalidInput("key expiration"))?,
        revoked: false,
      },
    };
    let metadata = issued.metadata.clone();
    let token_hash = digest("access", issued.token());
    let created_at = signed_time(metadata.created_at)?;
    let expires_at = signed_time(metadata.expires_at)?;
    self
      .run(move |connection| {
        diesel::insert_into(tokens::table)
          .values((
            tokens::id.eq(metadata.id),
            tokens::label.eq(metadata.label),
            tokens::token_hash.eq(token_hash),
            tokens::created_at.eq(created_at),
            tokens::expires_at.eq(expires_at),
            tokens::revoked.eq(false),
          ))
          .execute(connection)?;
        Ok(())
      })
      .await?;
    Ok(issued)
  }
  pub async fn list_access_tokens(&self) -> Result<Vec<AccessTokenMetadata>> {
    self
      .run(|connection| {
        tokens::table
          .order((tokens::created_at.desc(), tokens::id.asc()))
          .limit(1000)
          .select((
            tokens::id,
            tokens::label,
            tokens::created_at,
            tokens::expires_at,
            tokens::revoked,
          ))
          .load::<(String, String, i64, i64, bool)>(connection)?
          .into_iter()
          .map(|(id, label, created_at, expires_at, revoked)| {
            Ok(AccessTokenMetadata {
              id,
              label,
              created_at: u64::try_from(created_at).map_err(|_| StorageError::InvalidData)?,
              expires_at: u64::try_from(expires_at).map_err(|_| StorageError::InvalidData)?,
              revoked,
            })
          })
          .collect()
      })
      .await
  }
  pub async fn revoke_access_token(&self, id: &str) -> Result<()> {
    if !valid_key(id) {
      return Err(StorageError::InvalidInput("API key id"));
    }
    let id = id.to_owned();
    self
      .run(move |connection| {
        diesel::update(tokens::table.find(id))
          .set(tokens::revoked.eq(true))
          .execute(connection)?;
        Ok(())
      })
      .await
  }
  pub async fn authorize_access_token(&self, token: &str, now: u64) -> Result<bool> {
    if !valid_token(token) {
      return Ok(false);
    }
    let hash = digest("access", token);
    let now = signed_time(now)?;
    self
      .run(move |connection| {
        let stored = tokens::table
          .filter(tokens::token_hash.eq(&hash))
          .filter(tokens::revoked.eq(false))
          .filter(tokens::expires_at.gt(now))
          .select(tokens::token_hash)
          .first::<String>(connection)
          .optional()?;
        Ok(stored.is_some_and(|stored| equal(&stored, &hash)))
      })
      .await
  }
}
#[async_trait]
impl AccessControl for GatewayStorage {
  async fn authorize(&self, bearer: &str) -> std::result::Result<bool, AccessError> {
    let now = now_seconds().map_err(|_| AccessError)?;
    self
      .authorize_access_token(bearer, now)
      .await
      .map_err(|_| AccessError)
  }
}
