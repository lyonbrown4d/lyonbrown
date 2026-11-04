//! Rig's OpenAI protocol adapter over the in-process Gateway admission pipeline.
use axum::body::{Body, to_bytes};
use futures::StreamExt;
use rig_core::{
  DynModel,
  http_client::{
    Bytes, Error, HttpClientExt, LazyBody, MultipartForm, Request, Response, StreamingResponse,
  },
  operation::Completion,
  providers::openai::OpenAIConfig,
};

/// Constructs a Rig chat model backed by the shared Gateway instance.
/// The supplied token is authenticated by the same access port as network requests.
pub fn gateway_model(
  gateway: gateway_core::Gateway,
  model: &str,
  token: &str,
) -> crate::Result<DynModel<Completion>> {
  if model.trim().is_empty() || token.is_empty() {
    return Err(crate::ProviderError::Configuration(
      "gateway model and credential are required".into(),
    ));
  }
  let client = GatewayClient { gateway };
  Ok(
    OpenAIConfig::new(token.to_owned())
      .with_base_url("http://gateway.internal/v1")
      .connect(client)
      .chat(model)
      .erase(),
  )
}

/// Uses the Gateway only for its supported protocol; other protocols fail closed.
pub fn configured_with_gateway(
  config: &crate::ModelConfig,
  gateway: gateway_core::Gateway,
  token: &str,
) -> crate::Result<DynModel<Completion>> {
  crate::configuration::validate(config)?;
  if !crate::supports_gateway_chat(&config.provider) {
    return Err(crate::ProviderError::Configuration(
      "embedded gateway requires the native Chat Completions wire".into(),
    ));
  }
  let provider =
    crate::configuration::selection(&config.provider, token, Some("http://gateway.internal/v1"))?;
  let id = provider
    .id()
    .ok_or_else(|| crate::ProviderError::Configuration("unregistered gateway provider".into()))?;
  Ok(provider.completion_model(
    &config.model,
    rig_core::http_client::DynHttpClient::new(GatewayClient { gateway }),
    rig_core::catalog::Catalog::builtin().facts(id, &config.model),
  ))
}

#[derive(Clone)]
struct GatewayClient {
  gateway: gateway_core::Gateway,
}

impl GatewayClient {
  async fn execute<T: Into<Bytes>>(
    &self,
    request: Request<T>,
  ) -> Result<Response<Body>, Box<Error>> {
    if request.method() != http::Method::POST
      || request.uri().path() != "/v1/chat/completions"
      || request.uri().query().is_some()
    {
      return Err(Box::new(Error::instance(std::io::Error::other(
        "unsupported gateway request",
      ))));
    }
    let (parts, body) = request.into_parts();
    let response = self
      .gateway
      .invoke(Request::from_parts(parts, Body::from(body.into())))
      .await;
    if !response.status().is_success() {
      return Err(Box::new(Error::non_success_with_details(
        response.status(),
        response.headers().clone(),
        String::new(),
      )));
    }
    Ok(response)
  }
}

impl HttpClientExt for GatewayClient {
  fn send<T, U>(
    &self,
    request: Request<T>,
  ) -> impl std::future::Future<Output = Result<Response<LazyBody<U>>, Error>> + Send + 'static
  where
    T: Into<Bytes> + Send,
    U: From<Bytes> + Send + 'static,
  {
    let client = self.clone();
    let (parts, body) = request.into_parts();
    let request = Request::from_parts(parts, body.into());
    async move {
      let (parts, body) = client
        .execute(request)
        .await
        .map_err(|error| *error)?
        .into_parts();
      let future: LazyBody<U> = Box::pin(async move {
        let bytes = to_bytes(body, 67_108_864).await.map_err(Error::instance)?;
        Ok(U::from(bytes))
      });
      Ok(Response::from_parts(parts, future))
    }
  }
  fn send_multipart<U>(
    &self,
    _request: Request<MultipartForm>,
  ) -> impl std::future::Future<Output = Result<Response<LazyBody<U>>, Error>> + Send + 'static
  where
    U: From<Bytes> + Send + 'static,
  {
    std::future::ready(Err(Error::instance(std::io::Error::other(
      "gateway multipart is unsupported",
    ))))
  }
  #[allow(clippy::result_large_err, reason = "Rig public transport error type")]
  async fn send_streaming<T>(&self, request: Request<T>) -> Result<StreamingResponse, Error>
  where
    T: Into<Bytes> + Send,
  {
    let (parts, body) = self
      .execute(request)
      .await
      .map_err(|error| *error)?
      .into_parts();
    let stream = body
      .into_data_stream()
      .map(|chunk| chunk.map_err(Error::instance));
    Ok(Response::from_parts(parts, Box::pin(stream)))
  }
}
