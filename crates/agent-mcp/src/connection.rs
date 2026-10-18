use std::{sync::Arc, time::Duration};

use agent_runtime::Result;
use rmcp::{
  ClientHandler, RoleClient, ServiceExt,
  service::RunningService,
  transport::{
    StreamableHttpClientTransport, common::client_side_sse::NeverRetry,
    streamable_http_client::StreamableHttpClientTransportConfig,
  },
};

use crate::{McpLimits, McpServerConfig, McpTransport, config::invalid};

/// No sampling, elicitation, roots or other privileged client capability is
/// advertised. SDK defaults reject sampling/custom RPC and decline elicitation.
pub(crate) struct RestrictedClient;
impl ClientHandler for RestrictedClient {}

pub(crate) type Connection = RunningService<RoleClient, RestrictedClient>;

pub(crate) async fn connect(config: &McpServerConfig, limits: &McpLimits) -> Result<Connection> {
  agent_security::initialize_tls();
  let future = async {
    match &config.transport {
      McpTransport::Stdio(process) => {
        let transport = crate::stdio::start(process, &config.id, limits)?;
        RestrictedClient
          .serve(transport)
          .await
          .map_err(|_| invalid("MCP stdio handshake failed"))
      }
      McpTransport::StreamableHttp { url, headers } => {
        // Never forward credentials to redirects or ambient proxy endpoints.
        let client = reqwest::Client::builder()
          .redirect(reqwest::redirect::Policy::none())
          .no_proxy()
          .connect_timeout(limits.connection_timeout)
          .timeout(limits.call_timeout)
          .build()
          .map_err(|_| invalid("cannot build MCP HTTP client"))?;
        let mut options = StreamableHttpClientTransportConfig::with_uri(url.clone())
          .custom_headers(headers.validated()?)
          .reinit_on_expired_session(false)
          .max_sse_event_size(wire_limit(limits))
          .control_request_timeout(Duration::from_secs(1))
          .max_concurrent_requests(1);
        options.retry_config = Arc::new(NeverRetry::default());
        let transport = StreamableHttpClientTransport::with_client(
          crate::http_client::BoundedHttp {
            client,
            max_bytes: wire_limit(limits),
          },
          options,
        );
        RestrictedClient
          .serve(transport)
          .await
          .map_err(|_| invalid("MCP HTTP handshake failed"))
      }
    }
  };
  tokio::time::timeout(limits.connection_timeout, future)
    .await
    .map_err(|_| invalid("MCP handshake deadline exceeded"))?
}

pub(crate) fn wire_limit(limits: &McpLimits) -> usize {
  // JSON/base64 overhead has its own bounded envelope before JSON decoding.
  limits.max_text_bytes + limits.max_image_bytes * 4 / 3 + 64 * 1024
}
