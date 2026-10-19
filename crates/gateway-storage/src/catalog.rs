use crate::{
  GatewayStorage, Result, StorageError,
  error::valid_key,
  schema::{gateway_model_head as head, gateway_model_snapshots as snapshots},
};
use decision_core::ModelTier;
use diesel::prelude::*;
use gateway_core::ModelConfig;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Administrator-visible model metadata. Credentials cannot be represented in this type.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelMetadata {
  pub id: String,
  pub upstream_model: String,
  pub api_base: String,
  pub tier: ModelTier,
  pub local_inference: bool,
  pub capabilities: BTreeSet<String>,
  pub context_tokens: Option<u64>,
}
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ModelSnapshot {
  pub revision: String,
  pub models: Vec<ModelMetadata>,
}
impl GatewayStorage {
  pub async fn seed_models(&self, revision: &str, models: &[ModelConfig]) -> Result<()> {
    if !valid_key(revision) || models.is_empty() || models.len() > 256 {
      return Err(StorageError::InvalidInput("model snapshot"));
    }
    let mut ids = BTreeSet::new();
    let mut metadata = Vec::with_capacity(models.len());
    for model in models {
      model
        .validate()
        .map_err(|_| StorageError::InvalidInput("model metadata"))?;
      if !valid_key(&model.id) || !ids.insert(&model.id) {
        return Err(StorageError::InvalidInput("duplicate model"));
      }
      metadata.push(ModelMetadata {
        id: model.id.clone(),
        upstream_model: model.upstream_model.clone(),
        api_base: model.api_base.clone(),
        tier: model.tier,
        local_inference: model.local_inference,
        capabilities: model.capabilities.clone(),
        context_tokens: model.context_tokens,
      });
    }
    let revision = revision.to_owned();
    let document = serde_json::to_string(&metadata).map_err(|_| StorageError::InvalidData)?;
    if document.len() > 1_048_576 {
      return Err(StorageError::InvalidInput("model snapshot size"));
    }
    self
      .run(move |connection| {
        connection.immediate_transaction(|connection| {
          let existing = snapshots::table
            .filter(snapshots::revision.eq(&revision))
            .select(snapshots::document)
            .first::<String>(connection)
            .optional()?;
          if existing
            .as_ref()
            .is_some_and(|existing| existing != &document)
          {
            return Err(StorageError::Conflict);
          }
          if existing.is_none() {
            diesel::insert_into(snapshots::table)
              .values((
                snapshots::revision.eq(&revision),
                snapshots::document.eq(document),
              ))
              .execute(connection)?;
          }
          diesel::insert_into(head::table)
            .values((head::id.eq(1), head::revision.eq(&revision)))
            .on_conflict(head::id)
            .do_update()
            .set(head::revision.eq(&revision))
            .execute(connection)?;
          Ok(())
        })
      })
      .await
  }
  pub async fn load_models(&self) -> Result<Option<ModelSnapshot>> {
    self
      .run(|connection| {
        let revision = head::table
          .find(1)
          .select(head::revision)
          .first::<String>(connection)
          .optional()?;
        let Some(revision) = revision else {
          return Ok(None);
        };
        let row = snapshots::table
          .filter(snapshots::revision.eq(revision))
          .select((snapshots::revision, snapshots::document))
          .first::<(String, String)>(connection)
          .optional()?;
        row
          .map(|(revision, document)| {
            Ok(ModelSnapshot {
              revision,
              models: serde_json::from_str(&document).map_err(|_| StorageError::InvalidData)?,
            })
          })
          .transpose()
      })
      .await
  }
}
