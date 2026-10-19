use crate::{
  GatewayStorage, Result, StorageError,
  auth::{account, digest, equal, random_token, signed_time, valid_token},
  schema::gateway_admin_sessions as sessions,
};
use diesel::prelude::*;
use gateway_validation::Credentials;
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
pub struct AdminSession {
  token: String,
  csrf: String,
  pub expires_at: u64,
  pub identity: AdminIdentity,
}
impl std::fmt::Debug for AdminSession {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.debug_struct("AdminSession")
      .field("credentials", &"[REDACTED]")
      .field("identity", &self.identity)
      .field("expires_at", &self.expires_at)
      .finish()
  }
}
impl AdminSession {
  pub fn token(&self) -> &str {
    &self.token
  }
  pub fn csrf_token(&self) -> &str {
    &self.csrf
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
    let previous = self.run(account).await?;
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
      token: random_token()?,
      csrf: random_token()?,
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
      .run(move |connection| {
        connection.immediate_transaction(|connection| {
          // The connection is reacquired after CPU verification; a changed credential cannot create a stale session.
          if account(connection)?.as_ref() != Some(&previous) {
            return Err(StorageError::Unauthorized);
          }
          diesel::delete(sessions::table.filter(sessions::expires_at.le(now)))
            .execute(connection)?;
          if sessions::table.count().get_result::<i64>(connection)? >= 1000 {
            return Err(StorageError::Unauthorized);
          }
          diesel::insert_into(sessions::table)
            .values((
              sessions::token_hash.eq(token_hash),
              sessions::csrf_hash.eq(csrf_hash),
              sessions::expires_at.eq(expires_at),
              sessions::credential_revision.eq(previous.revision),
            ))
            .execute(connection)?;
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
        connection.transaction(|connection| {
          let Some(account) = account(connection)?.filter(|account| account.role == 1) else {
            return Ok(None);
          };
          let exists = sessions::table
            .find(hash)
            .filter(sessions::expires_at.gt(now))
            .filter(sessions::credential_revision.eq(account.revision))
            .count()
            .get_result::<i64>(connection)?
            == 1;
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
        connection.transaction(|connection| {
          let Some(account) = account(connection)?.filter(|account| account.role == 1) else {
            return Ok(false);
          };
          let stored = sessions::table
            .find(token_hash)
            .filter(sessions::expires_at.gt(now))
            .filter(sessions::credential_revision.eq(account.revision))
            .select(sessions::csrf_hash)
            .first::<String>(connection)
            .optional()?;
          Ok(stored.is_some_and(|stored| equal(&stored, &csrf_hash)))
        })
      })
      .await
  }
  pub async fn revoke_session(&self, token: &str) -> Result<()> {
    let hash = digest("session", token);
    self
      .run(move |connection| {
        diesel::delete(sessions::table.find(hash)).execute(connection)?;
        Ok(())
      })
      .await
  }
}
