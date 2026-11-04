mod support;
use gateway_core::ModelGroup;
use serde_json::{Value, json};
use std::sync::{Arc, atomic::Ordering};
use support::{Audit, config, gateway, mock_json, post};

const PNG: &str =
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC";
fn image() -> Value {
  json!({"type":"image_url","image_url":{"url":format!("data:image/png;base64,{PNG}"),"detail":"auto"}})
}
fn request(model: &str, part: Value) -> Value {
  json!({"model":model,"messages":[{"role":"user","content":[{"type":"text","text":"Describe this fixture image."},part]}],"max_tokens":32})
}
#[tokio::test]
async fn inline_images_require_vision_for_direct_models_and_every_group_candidate() {
  let (capture, upstream) = mock_json().await;
  let mut settings = config(&upstream.base);
  let mut vision = settings.models[0].clone();
  settings.models[0].id = "text-only".into();
  vision.id = "vision".into();
  vision.upstream_model = "vision-upstream".into();
  vision.capabilities.insert("vision".into());
  settings.models.push(vision);
  settings.groups = vec![ModelGroup {
    id: "mixed".into(),
    models: vec!["text-only".into(), "vision".into()],
  }];
  let (_, server) = gateway(settings, Arc::new(Audit::default())).await;
  let response = post(&server.base, request("text-only", image()))
    .send()
    .await
    .expect("response");
  assert_eq!(response.status(), 404);
  assert_eq!(capture.calls.load(Ordering::SeqCst), 0);
  let response = post(&server.base, request("mixed", image()))
    .send()
    .await
    .expect("response");
  assert_eq!(response.status(), 200);
  let _: Value = response.json().await.expect("completion");
  assert_eq!(capture.calls.load(Ordering::SeqCst), 1);
  let requests = capture.bodies.lock().expect("capture");
  assert_eq!(requests[0]["model"], "vision-upstream");
  assert_eq!(
    requests[0]["messages"][0]["content"][1],
    image(),
    "validated image bytes must never be rewritten by text masking"
  );
}
#[tokio::test]
async fn non_inline_mismatched_or_opaque_parts_cannot_reach_an_upstream() {
  let (capture, upstream) = mock_json().await;
  let mut settings = config(&upstream.base);
  settings.models[0].capabilities.insert("vision".into());
  let (_, server) = gateway(settings, Arc::new(Audit::default())).await;
  for part in [
    json!({"type":"image_url","image_url":{"url":"https://example.test/image.png"}}),
    json!({"type":"image_url","image_url":{"url":"data:image/png;base64,c2VjcmV0"}}),
    json!({"type":"image_url","image_url":{"url":format!("data:image/jpeg;base64,{PNG}")}}),
    json!({"type":"image_url","image_url":{"url":format!("data:image/png;base64,{PNG}"),"extra":"forbidden"}}),
  ] {
    let response = post(&server.base, request("public-model", part))
      .send()
      .await
      .expect("response");
    assert!(response.status().is_client_error());
  }
  assert_eq!(capture.calls.load(Ordering::SeqCst), 0);
}
