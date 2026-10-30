use std::{collections::BTreeSet, time::Duration};

use gateway_core::{EndpointAcquireError, EndpointRegistry, ProviderEndpoint, Secret};

fn endpoint(
  id: &str,
  api_base: &str,
  priority: u32,
  max_concurrent: usize,
  model_ids: &[&str],
) -> ProviderEndpoint {
  ProviderEndpoint {
    id: id.to_owned(),
    provider_id: "provider".to_owned(),
    api_base: api_base.to_owned(),
    api_key: None,
    api_key_env: None,
    enabled: true,
    priority,
    max_concurrent,
    model_ids: model_ids
      .iter()
      .map(|id| (*id).to_owned())
      .collect::<BTreeSet<_>>(),
  }
}

#[test]
fn chooses_lowest_priority_then_id_deterministically() {
  let registry = EndpointRegistry::new([
    endpoint("z", "https://example.test/v1", 3, 1, &["m"]),
    endpoint("b", "https://example.test/v1", 1, 1, &["m"]),
    endpoint("a", "https://example.test/v1", 1, 1, &["m"]),
  ])
  .expect("valid endpoints");

  let lease = registry.acquire("m", false).expect("endpoint available");
  assert_eq!(lease.endpoint().id, "a");
}

#[test]
fn distinguishes_capacity_from_unavailable() {
  let registry =
    EndpointRegistry::new([endpoint("single", "https://example.test/v1", 0, 1, &["m"])])
      .expect("valid endpoint");

  let lease = registry.acquire("m", false).expect("first lease");
  assert_eq!(
    registry
      .acquire("m", false)
      .expect_err("capacity exhausted"),
    EndpointAcquireError::CapacityExhausted
  );
  drop(lease);
  assert_eq!(
    registry.acquire("other", false).expect_err("no match"),
    EndpointAcquireError::Unavailable
  );
}

#[tokio::test]
async fn cooldown_skips_endpoint_until_expiry() {
  let registry = EndpointRegistry::new([
    endpoint("primary", "https://primary.test", 0, 2, &["m"]),
    endpoint("backup", "https://backup.test", 1, 2, &["m"]),
  ])
  .expect("valid endpoints");

  let primary = registry.acquire("m", false).expect("primary selected");
  primary.mark_rate_limited(Duration::from_millis(20));
  assert_eq!(
    registry
      .acquire("m", false)
      .expect("backup selected")
      .endpoint()
      .id,
    "backup"
  );
  tokio::time::sleep(Duration::from_millis(30)).await;
  assert_eq!(
    registry
      .acquire("m", false)
      .expect("primary recovered")
      .endpoint()
      .id,
    "primary"
  );

  let next = registry
    .acquire("m", false)
    .expect("primary selected again");
  next.mark_unavailable(Duration::from_secs(10_000));
  assert_eq!(
    registry
      .acquire("m", false)
      .expect("backup remains available")
      .endpoint()
      .id,
    "backup"
  );
}

#[test]
fn applies_enabled_model_and_local_address_filters() {
  let mut disabled = endpoint("disabled", "https://local.test", 0, 1, &["m"]);
  disabled.enabled = false;
  let registry = EndpointRegistry::new([
    disabled,
    endpoint("remote", "https://provider.test", 1, 1, &["m"]),
    endpoint("loopback", "http://127.0.0.1:9000/v1", 2, 1, &["m"]),
  ])
  .expect("valid endpoints");

  assert_eq!(
    registry.acquire("unknown", false).unwrap_err(),
    EndpointAcquireError::Unavailable
  );
  assert_eq!(
    registry
      .acquire("m", true)
      .expect("only loopback qualifies")
      .endpoint()
      .id,
    "loopback"
  );
  assert_eq!(
    registry
      .acquire("m", false)
      .expect("remote has priority")
      .endpoint()
      .id,
    "remote"
  );
}

#[test]
fn validates_remote_transport_and_rejects_duplicate_ids() {
  assert!(
    EndpointRegistry::new([endpoint("remote", "http://provider.test/v1", 0, 1, &["m"],)]).is_err()
  );
  assert!(
    EndpointRegistry::new([
      endpoint("same", "https://one.test", 0, 1, &["m"]),
      endpoint("same", "https://two.test", 1, 1, &["m"]),
    ])
    .is_err()
  );
}

#[test]
fn configuration_defaults_are_safe_and_positive() {
  let endpoint: ProviderEndpoint = serde_json::from_str(
    r#"{"id":"e","provider_id":"p","api_base":"https://p.test","model_ids":["m"]}"#,
  )
  .expect("endpoint defaults deserialize");
  assert!(endpoint.enabled);
  assert_eq!(endpoint.priority, 0);
  assert_eq!(endpoint.max_concurrent, 1);
}

#[test]
fn debug_redacts_endpoint_url_and_api_key() {
  let mut endpoint = endpoint("e", "https://secret-host.test/token-path", 0, 1, &["m"]);
  endpoint.api_key = Some(Secret::new("very-secret-api-key"));
  let debug = format!("{endpoint:?}");
  assert!(!debug.contains("secret-host"));
  assert!(!debug.contains("token-path"));
  assert!(!debug.contains("very-secret-api-key"));
}

#[test]
fn local_filter_does_not_turn_a_remote_endpoint_into_local_inference() {
  let registry =
    EndpointRegistry::new([endpoint("remote", "https://provider.test/v1", 0, 1, &["m"])])
      .expect("valid endpoint");
  assert_eq!(
    registry.acquire("m", true).unwrap_err(),
    EndpointAcquireError::Unavailable
  );
}
