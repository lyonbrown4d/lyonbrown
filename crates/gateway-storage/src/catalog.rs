use crate::{
  GatewayStorage, Result, StorageError,
  error::valid_key,
  schema::{gateway_model_head as head, gateway_model_snapshots as snapshots},
};
use decision_core::ModelTier;
use gateway_core::ModelConfig;
use sea_orm::{ActiveModelTrait, QueryFilter, Set, prelude::*, sea_query::OnConflict};
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
      .write(move |connection| {
        Box::pin(async move {
          let existing = snapshots::Entity::find()
            .filter(snapshots::Column::Revision.eq(&revision))
            .one(connection)
            .await?;
          if existing
            .as_ref()
            .is_some_and(|previous| previous.document != document)
          {
            return Err(StorageError::Conflict);
          }
          if existing.is_none() {
            snapshots::ActiveModel {
              revision: Set(revision.clone()),
              document: Set(document),
              ..Default::default()
            }
            .insert(connection)
            .await?;
          }
          head::Entity::insert(head::ActiveModel {
            id: Set(1),
            revision: Set(revision),
          })
          .on_conflict(
            OnConflict::column(head::Column::Id)
              .update_column(head::Column::Revision)
              .to_owned(),
          )
          .exec(connection)
          .await?;
          Ok(())
        })
      })
      .await
  }
  pub async fn load_models(&self) -> Result<Option<ModelSnapshot>> {
    self
      .run(|connection| {
        Box::pin(async move {
          let Some(head) = head::Entity::find_by_id(1).one(connection).await? else {
            return Ok(None);
          };
          snapshots::Entity::find()
            .filter(snapshots::Column::Revision.eq(head.revision))
            .one(connection)
            .await?
            .map(|row| {
              Ok(ModelSnapshot {
                revision: row.revision,
                models: serde_json::from_str(&row.document)
                  .map_err(|_| StorageError::InvalidData)?,
              })
            })
            .transpose()
        })
      })
      .await
  }
}
