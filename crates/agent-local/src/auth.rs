use crate::SecretToken;

pub const AUTHORIZATION_METADATA_KEY: &str = "authorization";

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum AuthError {
  #[error("authorization metadata is missing")]
  Missing,
  #[error("authorization metadata is malformed")]
  Malformed,
  #[error("authorization failed")]
  Unauthorized,
}

pub fn authorize_bearer(
  expected: &SecretToken,
  authorization: Option<&str>,
) -> Result<(), AuthError> {
  let authorization = authorization.ok_or(AuthError::Missing)?;
  let (scheme, encoded) = authorization.split_once(' ').ok_or(AuthError::Malformed)?;
  if !scheme.eq_ignore_ascii_case("Bearer")
    || encoded.is_empty()
    || encoded.contains(char::is_whitespace)
  {
    return Err(AuthError::Malformed);
  }
  let candidate = SecretToken::parse(encoded).map_err(|_| AuthError::Malformed)?;
  if expected.constant_time_eq(&candidate) {
    Ok(())
  } else {
    Err(AuthError::Unauthorized)
  }
}

pub fn bearer_authorization(token: &SecretToken) -> String {
  format!("Bearer {}", token.expose_for_transport())
}
