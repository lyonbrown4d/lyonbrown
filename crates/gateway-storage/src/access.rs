use crate::{
  GatewayStorage, Result, StorageError,
  auth::{digest, equal, now_seconds, random_token, signed_time, valid_token},
  error::valid_key,
  schema::gateway_access_tokens as tokens,
};
use async_trait::async_trait;
use gateway_core::{AccessControl, AccessError, AccessPrincipal};
use sea_orm::{
  ActiveModelTrait, QueryFilter, QueryOrder, QuerySelect, Set, prelude::*, sea_query::OnConflict,
};
use secrecy::{ExposeSecret, SecretString};
use serde::Serialize;
use std::collections::BTreeSet;

#[derive(Clone, Debug, Serialize)]
pub struct AccessTokenMetadata {
  pub id: String,
  pub label: String,
  pub created_at: u64,
  pub expires_at: u64,
  pub revoked: bool,
  pub project_id: String,
  pub allowed_models: BTreeSet<String>,
  pub max_output_tokens: Option<u64>,
  pub token_budget: Option<u64>,
  /// Lifetime usage plus outstanding conservative reservations.
  pub tokens_charged: u64,
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
      .write(move |connection| {
        Box::pin(async move {
          tokens::Entity::insert(tokens::ActiveModel {
            id: Set(id),
            label: Set("bootstrap".into()),
            token_hash: Set(token_hash),
            created_at: Set(now),
            expires_at: Set(i64::MAX),
            revoked: Set(false),
            project_id: Set("default".into()),
            allowed_models: Set("[]".into()),
            max_output_tokens: Set(None),
            token_budget: Set(None),
            tokens_charged: Set(0),
          })
          .on_conflict(
            OnConflict::column(tokens::Column::Id)
              .do_nothing()
              .to_owned(),
          )
          .try_insert()
          .exec(connection)
          .await?;
          Ok(())
        })
      })
      .await
  }
  pub async fn create_access_token(
    &self,
    label: &str,
    now: u64,
    ttl_seconds: u64,
  ) -> Result<IssuedAccessToken> {
    self
      .create_scoped_access_token(
        label,
        now,
        ttl_seconds,
        "default",
        BTreeSet::new(),
        None,
        None,
      )
      .await
  }
  #[allow(clippy::too_many_arguments)]
  pub async fn create_scoped_access_token(
    &self,
    label: &str,
    now: u64,
    ttl_seconds: u64,
    project_id: &str,
    allowed_models: BTreeSet<String>,
    max_output_tokens: Option<u64>,
    token_budget: Option<u64>,
  ) -> Result<IssuedAccessToken> {
    if !valid_key(project_id)
      || allowed_models.len() > 1000
      || allowed_models.iter().any(|model| !valid_key(model))
    {
      return Err(StorageError::InvalidInput("API key scope"));
    }
    let output_limit = max_output_tokens.map(signed_time).transpose()?;
    let budget = token_budget.map(signed_time).transpose()?;
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
        project_id: project_id.into(),
        allowed_models,
        max_output_tokens,
        token_budget,
        tokens_charged: 0,
      },
    };
    let metadata = issued.metadata.clone();
    let token_hash = digest("access", issued.token());
    let models =
      serde_json::to_string(&metadata.allowed_models).map_err(|_| StorageError::InvalidData)?;
    let created_at = signed_time(metadata.created_at)?;
    let expires_at = signed_time(metadata.expires_at)?;
    self
      .write(move |connection| {
        Box::pin(async move {
          tokens::ActiveModel {
            id: Set(metadata.id),
            label: Set(metadata.label),
            token_hash: Set(token_hash),
            created_at: Set(created_at),
            expires_at: Set(expires_at),
            revoked: Set(false),
            project_id: Set(metadata.project_id),
            allowed_models: Set(models),
            max_output_tokens: Set(output_limit),
            token_budget: Set(budget),
            tokens_charged: Set(0),
          }
          .insert(connection)
          .await?;
          Ok(())
        })
      })
      .await?;
    Ok(issued)
  }
  pub async fn list_access_tokens(&self) -> Result<Vec<AccessTokenMetadata>> {
    self
      .run(|connection| {
        Box::pin(async move {
          tokens::Entity::find()
            .order_by_desc(tokens::Column::CreatedAt)
            .order_by_asc(tokens::Column::Id)
            .limit(1000)
            .all(connection)
            .await?
            .into_iter()
            .map(|row| {
              Ok(AccessTokenMetadata {
                id: row.id,
                label: row.label,
                created_at: unsigned(row.created_at)?,
                expires_at: unsigned(row.expires_at)?,
                revoked: row.revoked,
                project_id: row.project_id,
                allowed_models: serde_json::from_str(&row.allowed_models)
                  .map_err(|_| StorageError::InvalidData)?,
                max_output_tokens: row.max_output_tokens.map(unsigned).transpose()?,
                token_budget: row.token_budget.map(unsigned).transpose()?,
                tokens_charged: unsigned(row.tokens_charged)?,
              })
            })
            .collect()
        })
      })
      .await
  }
  pub async fn revoke_access_token(&self, id: &str) -> Result<()> {
    if !valid_key(id) {
      return Err(StorageError::InvalidInput("API key id"));
    }
    let id = id.to_owned();
    self
      .write(move |connection| {
        Box::pin(async move {
          tokens::Entity::update_many()
            .col_expr(
              tokens::Column::Revoked,
              sea_orm::sea_query::Expr::value(true),
            )
            .filter(tokens::Column::Id.eq(id))
            .exec(connection)
            .await?;
          Ok(())
        })
      })
      .await
  }
  pub async fn authorize_access_token(&self, token: &str, now: u64) -> Result<bool> {
    Ok(self.authenticate_access_token(token, now).await?.is_some())
  }
  pub async fn authenticate_access_token(
    &self,
    token: &str,
    now: u64,
  ) -> Result<Option<AccessPrincipal>> {
    if !valid_token(token) {
      return Ok(None);
    }
    let hash = digest("access", token);
    let now = signed_time(now)?;
    self
      .run(move |connection| {
        Box::pin(async move {
          let stored = tokens::Entity::find()
            .filter(tokens::Column::TokenHash.eq(&hash))
            .filter(tokens::Column::Revoked.eq(false))
            .filter(tokens::Column::CreatedAt.lte(now))
            .filter(tokens::Column::ExpiresAt.gt(now))
            .one(connection)
            .await?;
          let Some(stored) = stored else {
            return Ok(None);
          };
          if !equal(&stored.token_hash, &hash) {
            return Ok(None);
          }
          Ok(Some(AccessPrincipal {
            key_id: stored.id,
            project_id: stored.project_id,
            allowed_models: serde_json::from_str(&stored.allowed_models)
              .map_err(|_| StorageError::InvalidData)?,
            max_output_tokens: stored.max_output_tokens.map(unsigned).transpose()?,
          }))
        })
      })
      .await
  }
}
#[async_trait]
impl AccessControl for GatewayStorage {
  async fn authenticate(
    &self,
    bearer: &str,
  ) -> std::result::Result<Option<AccessPrincipal>, AccessError> {
    let now = now_seconds().map_err(|_| AccessError)?;
    self
      .authenticate_access_token(bearer, now)
      .await
      .map_err(|_| AccessError)
  }
  async fn authorize(&self, bearer: &str) -> std::result::Result<bool, AccessError> {
    let now = now_seconds().map_err(|_| AccessError)?;
    self
      .authorize_access_token(bearer, now)
      .await
      .map_err(|_| AccessError)
  }
}

pub(crate) fn unsigned(value: i64) -> Result<u64> {
  u64::try_from(value).map_err(|_| StorageError::InvalidData)
}
