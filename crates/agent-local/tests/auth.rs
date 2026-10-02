use agent_local::{
  AUTHORIZATION_METADATA_KEY, AuthError, SecretToken, authorize_bearer, bearer_authorization,
};

#[test]
fn generated_token_is_canonical_base64url_without_padding() {
  let token = SecretToken::generate().expect("token generation");
  let encoded = token.expose_for_transport();

  assert_eq!(encoded.len(), 43);
  assert!(!encoded.contains('='));
  assert!(
    encoded
      .bytes()
      .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
  );
  assert_eq!(SecretToken::parse(&encoded).expect("parse token"), token);
}

#[test]
fn token_debug_and_display_are_redacted() {
  let token = SecretToken::generate().expect("token generation");
  let secret = token.expose_for_transport().to_owned();

  assert!(!format!("{token:?}").contains(&secret));
  assert!(!format!("{token}").contains(&secret));
  assert_eq!(format!("{token:?}"), "SecretToken([REDACTED])");
  assert_eq!(format!("{token}"), "[REDACTED]");
}

#[test]
fn bearer_authorization_accepts_only_the_expected_token() {
  let expected = SecretToken::generate().expect("expected token");
  let other = SecretToken::generate().expect("other token");
  let valid = format!("Bearer {}", expected.expose_for_transport());
  let invalid = format!("Bearer {}", other.expose_for_transport());

  assert_eq!(authorize_bearer(&expected, Some(&valid)), Ok(()));
  assert_eq!(
    authorize_bearer(&expected, Some(&invalid)),
    Err(AuthError::Unauthorized)
  );
  assert_eq!(authorize_bearer(&expected, None), Err(AuthError::Missing));
  assert_eq!(
    authorize_bearer(&expected, Some("Basic abc")),
    Err(AuthError::Malformed)
  );
  assert_eq!(
    authorize_bearer(&expected, Some("Bearer  token")),
    Err(AuthError::Malformed)
  );
}

#[test]
fn bearer_metadata_uses_the_standard_authorization_key() {
  let token = SecretToken::generate().expect("token generation");
  assert_eq!(AUTHORIZATION_METADATA_KEY, "authorization");
  assert_eq!(
    bearer_authorization(&token),
    format!("Bearer {}", token.expose_for_transport())
  );
}
