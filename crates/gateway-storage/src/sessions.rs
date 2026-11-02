use crate::{
  GatewayStorage, Result, StorageError,
  auth::{account, digest, equal, random_token, signed_time, valid_token},
  schema::gateway_admin_sessions as sessions,
};
use gateway_validation::Credentials;
use sea_orm::{ActiveModelTrait, PaginatorTrait, QueryFilter, Set, prelude::*};
use secrecy::{ExposeSecret, SecretString};
use serde::Serialize;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AdminRole {
  SuperAdmin,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct AdminIdentity {
  pub username: String,
  pub role: AdminRole,
}
/// The raw credentials are returned once and never implement serialization.
#[derive(Debug)]
pub struct AdminSession {
  token: SecretString,
  csrf: SecretString,
  pub expires_at: u64,
  pub identity: AdminIdentity,
}
impl AdminSession {
  pub fn token(&self) -> &str {
    self.token.expose_secret()
  }
  pub fn csrf_token(&self) -> &str {
    self.csrf.expose_secret()
  }
}
impl GatewayStorage {
  /// All unsuccessful logins use the same public error, including unknown accounts.
  pub async fn login(
    &self,
    username: &str,
    password: &str,
    now: u64,
    ttl_seconds: u64,
  ) -> Result<AdminSession> {
    self
      .login_inner(username, password, now, ttl_seconds)
      .await
      .map_err(|_| StorageError::Unauthorized)
  }
  async fn login_inner(
    &self,
    username: &str,
    password: &str,
    now: u64,
    ttl_seconds: u64,
  ) -> Result<AdminSession> {
    if (Credentials { username, password }).validate().is_err()
      || !(1..=86_400).contains(&ttl_seconds)
    {
      return Err(StorageError::Unauthorized);
    }
    let previous = self.run(|connection| Box::pin(account(connection))).await?;
    let previous = previous.filter(|account| account.username == username && account.role == 1);
    let verified = self
      .verify_password(
        password.to_owned(),
        previous
          .as_ref()
          .map(|account| account.password_hash.clone()),
      )
      .await?;
    let previous = previous
      .filter(|_| verified)
      .ok_or(StorageError::Unauthorized)?;
    let session = AdminSession {
      token: random_token()?.into(),
      csrf: random_token()?.into(),
      expires_at: now
        .checked_add(ttl_seconds)
        .ok_or(StorageError::Unauthorized)?,
      identity: AdminIdentity {
        username: previous.username.clone(),
        role: AdminRole::SuperAdmin,
      },
    };
    let expires_at = signed_time(session.expires_at)?;
    let now = signed_time(now)?;
    let token_hash = digest("session", session.token());
    let csrf_hash = digest("csrf", session.csrf_token());
    self
      .write(move |connection| {
        Box::pin(async move {
          if account(connection).await?.as_ref() != Some(&previous) {
            return Err(StorageError::Unauthorized);
          }
          sessions::Entity::delete_many()
            .filter(sessions::Column::ExpiresAt.lte(now))
            .exec(connection)
            .await?;
          if sessions::Entity::find().count(connection).await? >= 1000 {
            return Err(StorageError::Unauthorized);
          }
          sessions::ActiveModel {
            token_hash: Set(token_hash),
            csrf_hash: Set(csrf_hash),
            expires_at: Set(expires_at),
            credential_revision: Set(previous.revision),
          }
          .insert(connection)
          .await?;
          Ok(())
        })
      })
      .await?;
    Ok(session)
  }
  pub async fn session_identity(&self, token: &str, now: u64) -> Result<Option<AdminIdentity>> {
    if !valid_token(token) {
      return Ok(None);
    }
    let hash = digest("session", token);
    let now = signed_time(now)?;
    self
      .run(move |connection| {
        Box::pin(async move {
          let Some(account) = account(connection)
            .await?
            .filter(|account| account.role == 1)
          else {
            return Ok(None);
          };
          let exists = sessions::Entity::find_by_id(hash)
            .filter(sessions::Column::ExpiresAt.gt(now))
            .filter(sessions::Column::CredentialRevision.eq(account.revision))
            .one(connection)
            .await?
            .is_some();
          Ok(exists.then_some(AdminIdentity {
            username: account.username,
            role: AdminRole::SuperAdmin,
          }))
        })
      })
      .await
  }
  pub async fn authenticate_session(&self, token: &str, now: u64) -> Result<bool> {
    Ok(self.session_identity(token, now).await?.is_some())
  }
  pub async fn verify_csrf(&self, token: &str, csrf: &str, now: u64) -> Result<bool> {
    if !valid_token(token) || !valid_token(csrf) {
      return Ok(false);
    }
    let token_hash = digest("session", token);
    let csrf_hash = digest("csrf", csrf);
    let now = signed_time(now)?;
    self
      .run(move |connection| {
        Box::pin(async move {
          let Some(account) = account(connection)
            .await?
            .filter(|account| account.role == 1)
          else {
            return Ok(false);
          };
          let stored = sessions::Entity::find_by_id(token_hash)
            .filter(sessions::Column::ExpiresAt.gt(now))
            .filter(sessions::Column::CredentialRevision.eq(account.revision))
            .one(connection)
            .await?;
          Ok(stored.is_some_and(|stored| equal(&stored.csrf_hash, &csrf_hash)))
        })
      })
      .await
  }
  pub async fn revoke_session(&self, token: &str) -> Result<()> {
    let hash = digest("session", token);
    self
      .write(move |connection| {
        Box::pin(async move {
          sessions::Entity::delete_by_id(hash)
            .exec(connection)
            .await?;
          Ok(())
        })
      })
      .await
  }
}
