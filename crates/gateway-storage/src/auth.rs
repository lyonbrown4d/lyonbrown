use crate::{GatewayStorage, Result, StorageError, schema::gateway_admin_accounts as accounts};
use diesel::prelude::*;
use gateway_validation::Credentials;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

// Password PHCs and credential revision strings never implement Debug or serialization.
#[derive(Clone, Eq, PartialEq)]
pub(crate) struct Account {
  pub username: String,
  pub password_hash: String,
  pub role: i16,
  pub revision: String,
}
pub(crate) fn account(connection: &mut SqliteConnection) -> Result<Option<Account>> {
  let row = accounts::table
    .find(1)
    .select((
      accounts::username,
      accounts::password_hash,
      accounts::role,
      accounts::credential_revision,
    ))
    .first::<(String, String, i16, String)>(connection)
    .optional()?;
  Ok(
    row.map(|(username, password_hash, role, revision)| Account {
      username,
      password_hash,
      role,
      revision,
    }),
  )
}
impl GatewayStorage {
  /// Create the sole SuperAdmin once. Environment changes never overwrite persisted credentials.
  pub async fn bootstrap_super_admin(&self, username: &str, password: &str) -> Result<()> {
    if (Credentials { username, password }).validate().is_err() {
      return Err(StorageError::InvalidInput("super admin credentials"));
    }
    for _ in 0..3 {
      let previous = self.run(account).await?;
      if let Some(previous) = previous {
        let matches = previous.username == username && previous.role == 1;
        let verified = self
          .verify_password(
            password.to_owned(),
            matches.then(|| previous.password_hash.clone()),
          )
          .await?;
        return self
          .run(move |connection| {
            connection.immediate_transaction(|connection| {
              if verified && matches && account(connection)?.as_ref() == Some(&previous) {
                Ok(())
              } else {
                Err(StorageError::Conflict)
              }
            })
          })
          .await;
      }
      let password_hash = self.hash_password(password.to_owned()).await?;
      let username = username.to_owned();
      let revision = random_token()?;
      let inserted = self
        .run(move |connection| {
          connection.immediate_transaction(|connection| {
            if account(connection)?.is_some() {
              return Ok(false);
            }
            diesel::insert_into(accounts::table)
              .values((
                accounts::id.eq(1),
                accounts::username.eq(username),
                accounts::password_hash.eq(password_hash),
                accounts::role.eq(1_i16),
                accounts::credential_revision.eq(revision),
              ))
              .execute(connection)?;
            Ok(true)
          })
        })
        .await?;
      if inserted {
        return Ok(());
      }
    }
    Err(StorageError::Conflict)
  }
}
pub(crate) fn digest(purpose: &str, token: &str) -> String {
  let mut digest = Sha256::new();
  digest.update(b"lyonbrown-gateway-v1\0");
  digest.update(purpose.as_bytes());
  digest.update(b"\0");
  digest.update(token.as_bytes());
  hex::encode(digest.finalize())
}
pub(crate) fn equal(left: &str, right: &str) -> bool {
  bool::from(left.as_bytes().ct_eq(right.as_bytes()))
}
pub(crate) fn valid_token(token: &str) -> bool {
  (32..=512).contains(&token.len()) && token.bytes().all(|b| b.is_ascii_graphic())
}
pub(crate) fn random_token() -> Result<String> {
  let mut bytes = [0_u8; 32];
  getrandom::fill(&mut bytes).map_err(|_| StorageError::Worker)?;
  Ok(hex::encode(bytes))
}
pub(crate) fn signed_time(now: u64) -> Result<i64> {
  i64::try_from(now).map_err(|_| StorageError::InvalidInput("timestamp"))
}
pub(crate) fn now_seconds() -> Result<u64> {
  std::time::SystemTime::now()
    .duration_since(std::time::UNIX_EPOCH)
    .map(|elapsed| elapsed.as_secs())
    .map_err(|_| StorageError::Worker)
}
