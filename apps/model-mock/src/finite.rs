//! Wiremock owns finite response templates and delays. Only synthetic route IDs
//! are sent to this private server; user prompts and credentials never enter it.
use crate::{AppConfig, Reply};
use anyhow::Result;
use std::time::Duration;
use wiremock::{
  Mock, MockServer, ResponseTemplate,
  matchers::{method, path},
};

pub async fn start(settings: &AppConfig) -> Result<MockServer> {
  let server = MockServer::builder()
    .disable_request_recording()
    .start()
    .await;
  for (scenario_index, scenario) in settings.scenarios.iter().enumerate() {
    for (reply_index, reply) in scenario.replies.iter().enumerate() {
      if matches!(reply, Reply::Stall) {
        continue;
      }
      // Streaming bodies retain their fragmentation through the shared Axum fixture.
      let scripted = reply.scripted(scenario.protocol, &scenario.model, false);
      let Some(finite) = scripted.finite_body() else {
        continue;
      };
      let mut template = ResponseTemplate::new(finite.status)
        .set_body_bytes(finite.body)
        .set_delay(Duration::from_millis(reply.delay_ms()));
      for (name, value) in finite.headers {
        template = template.insert_header(name, value);
      }
      Mock::given(method("GET"))
        .and(path(format!("/{scenario_index}/{reply_index}")))
        .respond_with(template)
        .mount(&server)
        .await;
    }
  }
  Ok(server)
}
