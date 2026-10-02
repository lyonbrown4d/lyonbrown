use std::fmt;

use agent_local::{AUTHORIZATION_METADATA_KEY, SecretToken, authorize_bearer};
use tonic::metadata::MetadataValue;
use tonic::service::Interceptor;
use tonic::{Request, Status};

const AUTHENTICATION_REQUIRED: &str = "authentication required";

#[derive(Clone)]
pub(crate) struct ServerAuthorization {
  expected: SecretToken,
}

impl ServerAuthorization {
  pub(crate) fn new(expected: SecretToken) -> Self {
    Self { expected }
  }

  fn reject() -> Status {
    Status::unauthenticated(AUTHENTICATION_REQUIRED)
  }
}

impl fmt::Debug for ServerAuthorization {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    formatter
      .debug_struct("ServerAuthorization")
      .field("expected", &"[REDACTED]")
      .finish()
  }
}

impl Interceptor for ServerAuthorization {
  fn call(&mut self, mut request: Request<()>) -> Result<Request<()>, Status> {
    let authorization = {
      let mut values = request
        .metadata()
        .get_all(AUTHORIZATION_METADATA_KEY)
        .iter();
      let value = values.next().ok_or_else(Self::reject)?;
      if values.next().is_some() {
        return Err(Self::reject());
      }
      value.to_str().map_err(|_| Self::reject())?.to_owned()
    };
    authorize_bearer(&self.expected, Some(&authorization)).map_err(|_| Self::reject())?;
    let mut protected = MetadataValue::try_from(authorization).map_err(|_| Self::reject())?;
    protected.set_sensitive(true);
    request
      .metadata_mut()
      .insert(AUTHORIZATION_METADATA_KEY, protected);
    Ok(request)
  }
}
