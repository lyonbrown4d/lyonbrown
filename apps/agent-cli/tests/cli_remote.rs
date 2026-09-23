use std::path::PathBuf;

use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn openai_cli_uses_secret_reference_and_streams_answer() {
  let server = MockServer::start().await;
  Mock::given(method("POST"))
    .and(path("/v1/chat/completions"))
    .and(header("authorization", "Bearer test-key"))
    .respond_with(ResponseTemplate::new(200).set_body_string(
      "data: {\"choices\":[{\"delta\":{\"content\":\"remote-ok\"}}]}\n\ndata: [DONE]\n\n",
    ))
    .mount(&server)
    .await;

  let config_path: PathBuf = std::env::temp_dir().join(format!(
    "lyonbrown-openai-test-{}-{}.toml",
    std::process::id(),
    std::time::SystemTime::now()
      .duration_since(std::time::UNIX_EPOCH)
      .unwrap()
      .as_nanos()
  ));
  std::fs::write(
    &config_path,
    format!(
      "[model]\nprovider = 'openai'\nmodel = 'gpt-4o'\n[openai]\nendpoint = '{}/v1'\napi_key_ref = 'env:OPENAI_API_KEY'\n",
      server.uri()
    ),
  )
  .unwrap();

  let output = tokio::process::Command::new(env!("CARGO_BIN_EXE_agent"))
    .arg("--config")
    .arg(&config_path)
    .arg("ask")
    .arg("hello")
    .env("OPENAI_API_KEY", "test-key")
    .env("LYONBROWN_STORAGE__ENABLED", "false")
    .output()
    .await
    .unwrap();
  std::fs::remove_file(&config_path).unwrap();

  assert!(
    output.status.success(),
    "{}",
    String::from_utf8_lossy(&output.stderr)
  );
  assert!(String::from_utf8_lossy(&output.stdout).contains("remote-ok"));
}
