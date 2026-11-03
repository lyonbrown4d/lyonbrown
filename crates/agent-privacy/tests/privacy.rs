use agent_privacy::{
  Detection, Detector, Evidence, Kind, PrivacyError, PrivacyService, RuleDetector,
};
use secrecy::SecretString;

#[test]
fn protects_content_and_resolves_only_in_the_original_scope() -> Result<(), PrivacyError> {
  let known = [
    SecretString::from("secret-value-123".to_owned()),
    SecretString::from("value-123".to_owned()),
  ];
  let mut service = PrivacyService::new(&known)?;
  let input = "密码 secret-value-123 email: person@example.test";
  let protected = service.protect_text("session-a", input)?;
  assert!(!protected.contains("secret-value-123"));
  assert!(!protected.contains("person@example.test"));
  let placeholder = protected
    .split_whitespace()
    .nth(1)
    .ok_or(PrivacyError::MissingMapping)?;
  assert_eq!(
    service.resolve("session-a", placeholder)?,
    "secret-value-123"
  );
  assert!(service.resolve("session-b", placeholder).is_err());
  assert!(service.resolve("session-a", "[PRIVATE:unknown]").is_err());
  assert_eq!(
    service.protect_text("session-a", "secret-value-123")?,
    placeholder
  );
  assert_ne!(
    service.protect_text("session-b", "secret-value-123")?,
    placeholder
  );
  service.forget_scope("session-a");
  assert!(service.resolve("session-a", placeholder).is_err());
  Ok(())
}

#[test]
fn detects_credentials_private_keys_and_contextual_phone_with_byte_offsets()
-> Result<(), PrivacyError> {
  let detector = RuleDetector::new(&[])?;
  let input = "配置 password=hunter-example phone: +1 202-555-0101\n-----BEGIN PRIVATE KEY-----\nFAKE\n-----END PRIVATE KEY-----";
  let hits = detector.detect(input)?;
  for expected in [
    "hunter-example",
    "+1 202-555-0101",
    "-----BEGIN PRIVATE KEY-----\nFAKE\n-----END PRIVATE KEY-----",
  ] {
    assert!(
      hits
        .iter()
        .any(|hit| &input[hit.start..hit.end] == expected)
    );
  }
  Ok(())
}

struct InvalidDetector;
impl Detector for InvalidDetector {
  fn detect(&self, _: &str) -> Result<Vec<Detection>, PrivacyError> {
    Ok(vec![Detection {
      start: 1,
      end: 2,
      kind: Kind::Credential,
      evidence: Evidence::Pattern,
    }])
  }
}

#[test]
fn rejects_invalid_utf8_boundaries_and_oversized_input() {
  let mut service = PrivacyService::with_detector(Box::new(InvalidDetector));
  assert!(matches!(
    service.protect_text("scope", "中"),
    Err(PrivacyError::InvalidSpan)
  ));
  assert!(matches!(
    service.protect_text("scope", &"a".repeat(1024 * 1024 + 1)),
    Err(PrivacyError::Limit)
  ));
}

#[test]
fn protected_password_assignment_is_stable_on_reinspection() -> Result<(), PrivacyError> {
  let mut service = PrivacyService::new(&[])?;
  let protected = service.protect_text("scope", "password=example-secret")?;
  assert!(!protected.contains("example-secret"));
  assert_eq!(service.protect_text("scope", &protected)?, protected);
  Ok(())
}

#[test]
fn observed_and_registered_secrets_are_reused_after_transfer() -> Result<(), PrivacyError> {
  let mut local = PrivacyService::new(&[])?;
  let first = local.protect_text("scope", "password=shortvalue")?;
  let token = first
    .strip_prefix("password=")
    .ok_or(PrivacyError::MissingMapping)?;
  assert_eq!(local.protect_text("scope", "shortvalue")?, token);
  local.register_secret("scope", "knownkey")?;
  let mut shared = PrivacyService::new(&[])?;
  local.transfer_scope(&mut shared, "scope")?;
  assert_eq!(shared.protect_text("scope", "shortvalue")?, token);
  assert!(
    !shared
      .protect_text("scope", "knownkey")?
      .contains("knownkey")
  );
  assert_eq!(shared.resolve("scope", token)?, "shortvalue");
  assert_eq!(shared.protect_text("other", "shortvalue")?, "shortvalue");
  assert_eq!(shared.protect_text("scope", &first)?, first);
  Ok(())
}

#[test]
fn encrypted_host_port_round_trip_checks_scope_and_conflicts() -> Result<(), PrivacyError> {
  let mut source = PrivacyService::new(&[])?;
  let token = source.protect_text("session-a", "person@example.test")?;
  let saved = source.export_scope("session-a")?;
  let mut restored = PrivacyService::new(&[])?;
  assert!(restored.import_scope("session-b", &saved).is_err());
  restored.import_scope("session-a", &saved)?;
  restored.import_scope("session-a", &saved)?;
  assert_eq!(
    restored.resolve("session-a", &token)?,
    "person@example.test"
  );
  let conflict = SecretString::from(format!(
    r#"{{"scope":"session-a","mappings":[{{"placeholder":"{token}","original":"different"}}]}}"#
  ));
  assert!(restored.import_scope("session-a", &conflict).is_err());
  assert_eq!(
    restored.resolve("session-a", &token)?,
    "person@example.test"
  );
  Ok(())
}

#[test]
fn fabricated_reference_cannot_hide_a_known_secret() -> Result<(), PrivacyError> {
  let mut service = PrivacyService::new(&[])?;
  service.register_secret("scope", "abcdef")?;
  let input = "[PRIVATE:abcdef1234567890abcdef1234567890]";
  assert!(!service.protect_text("scope", input)?.contains("abcdef"));
  Ok(())
}

#[test]
fn protects_config_values_and_common_synthetic_credential_shapes() -> Result<(), PrivacyError> {
  let github = format!("ghp_{}", "A".repeat(36));
  let github_fine_grained = format!("github_pat_{}", "b1".repeat(30));
  let google = format!("AIza{}", "c".repeat(35));
  let slack = "xoxb-1234567890-fakevalue".to_owned();
  let aws_access = "AKIA1234567890ABCDEF".to_owned();
  let aws_session = "ASIA1234567890ABCDEF".to_owned();
  let jwt = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJmaXh0dXJlIn0.c2lnbmF0dXJlLWZpeHR1cmU".to_owned();
  let cases = vec![
    (
      "DB_PASSWORD=\"my secret phrase\"".to_owned(),
      "my secret phrase".to_owned(),
    ),
    (
      r#"{"CLIENT_SECRET":"hello world"}"#.to_owned(),
      "hello world".to_owned(),
    ),
    (
      "SERVICE_TOKEN='two words'".to_owned(),
      "two words".to_owned(),
    ),
    (
      "CLIENT_SECRET=opaque-value".to_owned(),
      "opaque-value".to_owned(),
    ),
    (
      r#"{"password":"quoted \"value\" here"}"#.to_owned(),
      r#"quoted \"value\" here"#.to_owned(),
    ),
    (github.clone(), github),
    (github_fine_grained.clone(), github_fine_grained),
    (google.clone(), google),
    (slack.clone(), slack),
    (aws_access.clone(), aws_access),
    (aws_session.clone(), aws_session),
    (jwt.clone(), jwt),
  ];
  for (index, (input, original)) in cases.into_iter().enumerate() {
    let mut privacy = PrivacyService::new(&[])?;
    let scope = format!("fixture-{index}");
    let output = privacy.protect_text(&scope, &input)?;
    assert!(
      !output.contains(&original),
      "fixture {index} leaked sensitive content"
    );
    let start = output
      .find("[PRIVATE:")
      .ok_or(PrivacyError::MissingMapping)?;
    let token = output
      .get(start..start + 42)
      .ok_or(PrivacyError::InvalidSpan)?;
    assert_eq!(
      privacy.resolve(&scope, token)?,
      original,
      "fixture {index} lost bytes"
    );
    if input.starts_with('{') {
      let _: serde_json::Value =
        serde_json::from_str(&output).map_err(|_| PrivacyError::InvalidSpan)?;
    }
  }
  Ok(())
}

#[test]
fn global_known_secrets_apply_to_existing_and_new_scopes_without_granting_access()
-> Result<(), PrivacyError> {
  use secrecy::ExposeSecret;
  let mut privacy = PrivacyService::new(&[])?;
  privacy.register_secret("existing", "prior known value")?;
  let before = privacy.export_scope("existing")?;
  let old = SecretString::from("violet orchard mist".to_owned());
  privacy.register_known_secret(&old)?;
  privacy.register_known_secret(&old)?;
  assert_eq!(
    before.expose_secret(),
    privacy.export_scope("existing")?.expose_secret()
  );
  let existing = privacy.protect_text("existing", old.expose_secret())?;
  assert_eq!(privacy.resolve("existing", &existing)?, old.expose_secret());
  assert!(privacy.resolve("new", &existing).is_err());
  let new_scope = privacy.protect_text("new", old.expose_secret())?;
  assert_ne!(existing, new_scope);
  assert_eq!(privacy.resolve("new", &new_scope)?, old.expose_secret());
  assert!(privacy.resolve("existing", &new_scope).is_err());

  let rotated = SecretString::from("cobalt meadow dusk".to_owned());
  privacy.register_known_secret(&rotated)?;
  assert_eq!(
    privacy.protect_text("existing", old.expose_secret())?,
    existing
  );
  let protected = privacy.protect_text("new", rotated.expose_secret())?;
  assert_eq!(privacy.resolve("new", &protected)?, rotated.expose_secret());
  let oversized = SecretString::from("x".repeat(1024 * 1024));
  assert!(matches!(
    privacy.register_known_secret(&oversized),
    Err(PrivacyError::Limit)
  ));
  assert_eq!(
    privacy.protect_text("existing", old.expose_secret())?,
    existing
  );
  assert_eq!(
    privacy.protect_text("new", rotated.expose_secret())?,
    protected
  );
  Ok(())
}

struct EmptyDetector;
impl Detector for EmptyDetector {
  fn detect(&self, _: &str) -> Result<Vec<Detection>, PrivacyError> {
    Ok(vec![])
  }
}

#[test]
fn global_registration_preserves_the_host_supplied_detector() -> Result<(), PrivacyError> {
  let mut privacy = PrivacyService::with_detector(Box::new(EmptyDetector));
  privacy.register_known_secret(&SecretString::from("amber meadow phrase".to_owned()))?;
  assert_eq!(
    privacy.protect_text("scope", "person@example.test")?,
    "person@example.test"
  );
  let protected = privacy.protect_text("scope", "amber meadow phrase")?;
  assert_eq!(privacy.resolve("scope", &protected)?, "amber meadow phrase");
  Ok(())
}
