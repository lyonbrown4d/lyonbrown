mod audit;
mod streaming;
use crate::{
  AuditRecord, AuditStatus, Gateway, GatewayError, ModelConfig, request::ValidatedRequest,
};
use audit::AuditGuard;
use axum::{
  http::{StatusCode, header},
  response::{IntoResponse, Response},
};
use futures::StreamExt;
use tokio::{
  sync::OwnedSemaphorePermit,
  time::{Instant, timeout_at},
};

impl Gateway {
  pub(crate) async fn transfer(
    &self,
    request_id: String,
    mut request: ValidatedRequest,
    model: ModelConfig,
    revision: String,
    permit: OwnedSemaphorePermit,
    started: Instant,
  ) -> Result<Response, GatewayError> {
    request.body["model"] = model.upstream_model.into();
    let record = AuditRecord {
      request_id,
      model: model.id,
      status: AuditStatus::Pending,
      elapsed_ms: 0,
      policy_revision: revision,
    };
    let limits = &self.inner.config.limits;
    let total_deadline = started + std::time::Duration::from_millis(limits.total_ms);
    let first_deadline =
      (started + std::time::Duration::from_millis(limits.first_byte_ms)).min(total_deadline);
    let mut audit = AuditGuard::new(
      self.inner.audit.clone(),
      record,
      started,
      total_deadline,
      self.inner.signals.clone(),
      permit,
    )
    .prepare(first_deadline)
    .await?;
    let outgoing = self.inner.client.send(
      &model.api_base,
      model.api_key.as_ref(),
      audit.request_id(),
      &request.body,
    );
    let response = match timeout_at(first_deadline, outgoing).await {
      Ok(Ok(response)) => response,
      Ok(Err(error)) if error.non_success_status().is_some() => {
        let upstream_status = error.non_success_status();
        let status = if upstream_status == Some(StatusCode::TOO_MANY_REQUESTS) {
          StatusCode::TOO_MANY_REQUESTS
        } else {
          StatusCode::BAD_GATEWAY
        };
        let rejected = upstream_status.is_some_and(|status| status.is_client_error());
        drop(error);
        audit
          .finish(if rejected {
            AuditStatus::Failed
          } else {
            AuditStatus::Uncertain
          })
          .await?;
        return Err(GatewayError(
          status,
          "upstream_error",
          "Upstream rejected the request",
        ));
      }
      result => {
        audit.finish(AuditStatus::Uncertain).await?;
        return Err(if result.is_err() {
          GatewayError::timeout()
        } else {
          GatewayError::upstream()
        });
      }
    };
    let expected = if request.stream {
      "text/event-stream"
    } else {
      "application/json"
    };
    if !response
      .headers()
      .get(header::CONTENT_TYPE)
      .and_then(|v| v.to_str().ok())
      .is_some_and(|v| v.split(';').next() == Some(expected))
    {
      drop(response);
      audit.finish(AuditStatus::Uncertain).await?;
      return Err(GatewayError::upstream());
    }
    let mut source = response.into_body();
    let first = match timeout_at(first_deadline, async {
      loop {
        match source.next().await {
          Some(Ok(chunk)) if chunk.is_empty() => continue,
          result => return result,
        }
      }
    })
    .await
    {
      Ok(Some(Ok(chunk))) => chunk,
      result => {
        drop(source);
        audit.finish(AuditStatus::Uncertain).await?;
        return Err(if result.is_err() {
          GatewayError::timeout()
        } else {
          GatewayError::upstream()
        });
      }
    };
    let max_response = limits.max_response_bytes;
    if first.len() > max_response {
      drop(source);
      audit.finish(AuditStatus::Uncertain).await?;
      return Err(GatewayError::upstream());
    }
    if request.stream {
      let mut validator = crate::sse::SseValidator::new();
      if validator.check(&first).is_err() {
        drop(source);
        audit.finish(AuditStatus::Uncertain).await?;
        return Err(GatewayError::upstream());
      }
      let body = streaming::body(
        source,
        first,
        validator,
        audit,
        total_deadline,
        max_response,
      );
      Ok(
        (
          [
            (header::CONTENT_TYPE, "text/event-stream"),
            (header::CACHE_CONTROL, "no-cache"),
          ],
          body,
        )
          .into_response(),
      )
    } else {
      let mut bytes = first.to_vec();
      loop {
        match timeout_at(total_deadline, source.next()).await {
          Ok(Some(Ok(chunk))) if chunk.len() <= max_response.saturating_sub(bytes.len()) => {
            bytes.extend_from_slice(&chunk)
          }
          Ok(None) => break,
          result => {
            drop(source);
            audit.finish(AuditStatus::Uncertain).await?;
            return Err(if result.is_err() {
              GatewayError::timeout()
            } else {
              GatewayError::upstream()
            });
          }
        }
      }
      let valid = serde_json::from_slice::<serde_json::Value>(&bytes).is_ok_and(|value| {
        value.is_object()
          && value.get("error").is_none()
          && value.get("object").and_then(serde_json::Value::as_str) == Some("chat.completion")
          && value
            .get("choices")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|choices| !choices.is_empty())
      });
      if !valid {
        audit.finish(AuditStatus::Uncertain).await?;
        return Err(GatewayError::upstream());
      }
      let completion = audit.body_completion();
      let stream = async_stream::stream! {
        let _completion = completion;
        yield Ok::<_, std::io::Error>(bytes);
      };
      let body = axum::body::Body::from_stream(stream);
      audit.finish(AuditStatus::Succeeded).await?;
      Ok(([(header::CONTENT_TYPE, "application/json")], body).into_response())
    }
  }
}
