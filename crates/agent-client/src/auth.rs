use std::fmt;

use agent_local::{
  AUTHORIZATION_METADATA_KEY, DiscoveryDescriptor, SecretToken, bearer_authorization,
};
use tonic::metadata::{Ascii, MetadataValue};
use tonic::service::Interceptor;
use tonic::{Request, Status};

/// A loopback daemon endpoint coupled to its per-launch credential.
#[derive(Clone)]
pub struct AuthenticatedTarget {
  endpoint: String,
  token: SecretToken,
}

impl AuthenticatedTarget {
  /// Builds an authenticated target from a validated, loopback-only discovery record.
  pub fn from_descriptor(descriptor: &DiscoveryDescriptor) -> Self {
    Self {
      endpoint: format!("http://{}", descriptor.address()),
      token: descriptor.token().clone(),
    }
  }

  pub fn endpoint(&self) -> &str {
    &self.endpoint
  }

  pub(crate) fn into_parts(self) -> (String, AuthorizationInterceptor) {
    let interceptor = AuthorizationInterceptor::new(&self.token);
    (self.endpoint, interceptor)
  }
}

impl fmt::Debug for AuthenticatedTarget {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    formatter
      .debug_struct("AuthenticatedTarget")
      .field("endpoint", &self.endpoint)
      .field("token", &self.token)
      .finish()
  }
}

/// Adds one redaction-aware bearer value to every outgoing RPC.
#[derive(Clone)]
pub struct AuthorizationInterceptor {
  authorization: MetadataValue<Ascii>,
}

impl AuthorizationInterceptor {
  pub fn new(token: &SecretToken) -> Self {
    let mut authorization = MetadataValue::try_from(bearer_authorization(token))
      .expect("generated bearer metadata is valid ASCII");
    authorization.set_sensitive(true);
    Self { authorization }
  }
}

impl fmt::Debug for AuthorizationInterceptor {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    formatter
      .debug_struct("AuthorizationInterceptor")
      .field("authorization", &"[REDACTED]")
      .finish()
  }
}

impl Interceptor for AuthorizationInterceptor {
  fn call(&mut self, mut request: Request<()>) -> Result<Request<()>, Status> {
    request
      .metadata_mut()
      .insert(AUTHORIZATION_METADATA_KEY, self.authorization.clone());
    Ok(request)
  }
}
