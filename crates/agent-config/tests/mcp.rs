use agent_config::{McpTransportSettings, load_with_environment};
use std::{collections::BTreeMap, fs};

#[test]
fn typed_mcp_configuration_resolves_only_explicit_secret_references_with_layer_precedence() {
  let data = tempfile::tempdir().expect("data directory");
  let executable = data.path().join("mcp-server");
  let configuration = r#"
[[mcp]]
id = "local"
allowed_tools = ["echo"]
[mcp.transport]
type = "stdio"
executable = "/configured/mcp-server"
arguments = ["synthetic-private-argument"]
environment = { CHILD_TOKEN = "MCP_TOKEN" }
[[mcp]]
id = "remote"
allowed_tools = ["lookup"]
[mcp.transport]
type = "streamable_http"
url = "https://mcp.example.test/rpc"
headers = { Authorization = "MCP_AUTH" }
"#
  .replace(
    "executable = \"/configured/mcp-server\"",
    &format!("executable = '{}'", executable.display()),
  );
  fs::write(data.path().join("config.toml"), configuration).expect("typed config");
  fs::write(data.path().join(".env"), "MCP_TOKEN=synthetic-dotenv-token\nMCP_AUTH=synthetic-dotenv-auth\nOPENAI_API_KEY=synthetic-provider-secret\n").expect("dotenv");
  let environment = BTreeMap::from([("MCP_TOKEN".into(), "synthetic-real-env-token".into())]);
  let settings = load_with_environment(data.path(), None, &environment).expect("load config");
  assert_eq!(settings.mcp.len(), 2);
  assert_eq!(
    settings.mcp_environment_value("MCP_TOKEN"),
    Some("synthetic-real-env-token")
  );
  assert_eq!(
    settings.mcp_environment_value("MCP_AUTH"),
    Some("synthetic-dotenv-auth")
  );
  assert_eq!(settings.mcp_environment_value("OPENAI_API_KEY"), None);
  assert_eq!(
    settings.credential("openai").as_deref(),
    Some("synthetic-provider-secret")
  );
  match &settings.mcp[0].transport {
    McpTransportSettings::Stdio {
      environment,
      arguments,
      ..
    } => {
      assert_eq!(
        environment.get("CHILD_TOKEN").map(String::as_str),
        Some("MCP_TOKEN")
      );
      assert_eq!(arguments, &["synthetic-private-argument"]);
    }
    _ => panic!("expected typed stdio transport"),
  }
  let debug = format!("{settings:?}");
  for private in [
    "synthetic-dotenv",
    "synthetic-real-env",
    "synthetic-provider",
    "synthetic-private",
  ] {
    assert!(!debug.contains(private));
  }
  let dotenv = load_with_environment(data.path(), None, &BTreeMap::new()).expect("dotenv config");
  assert_eq!(
    dotenv.mcp_environment_value("MCP_TOKEN"),
    Some("synthetic-dotenv-token")
  );
}

#[test]
fn missing_mcp_reference_fails_closed_without_echoing_other_credentials() {
  let data = tempfile::tempdir().expect("data directory");
  fs::write(
    data.path().join("config.toml"),
    r#"
[[mcp]]
id = "fixture"
allowed_tools = ["echo"]
[mcp.transport]
type = "streamable_http"
url = "https://mcp.example.test/rpc"
headers = { Authorization = "MISSING_MCP_TOKEN" }
"#,
  )
  .expect("config");
  let environment =
    BTreeMap::from([("OPENAI_API_KEY".into(), "synthetic-private-provider".into())]);
  let error =
    load_with_environment(data.path(), None, &environment).expect_err("missing explicit reference");
  let message = format!("{error:#}");
  assert!(message.contains("MISSING_MCP_TOKEN"));
  assert!(!message.contains("synthetic-private-provider"));
}

#[test]
fn unsafe_mcp_urls_and_unknown_secret_fields_are_rejected_without_echoing_values() {
  let data = tempfile::tempdir().expect("data directory");
  for url in [
    "http://public.example.test/mcp",
    "https://user:synthetic-private@mcp.example.test/",
    "https://mcp.example.test/?token=synthetic-private",
    "https://mcp.example.test/#synthetic-private",
    "file:///synthetic-private",
  ] {
    fs::write(data.path().join("config.toml"), format!("[[mcp]]\nid='fixture'\nallowed_tools=['echo']\n[mcp.transport]\ntype='streamable_http'\nurl='{url}'\n")).expect("config");
    let error =
      load_with_environment(data.path(), None, &BTreeMap::new()).expect_err("unsafe endpoint");
    assert!(!format!("{error:#}").contains("synthetic-private"));
  }
  fs::write(data.path().join("config.toml"), "[[mcp]]\nid='fixture'\nallowed_tools=['echo']\n[mcp.transport]\ntype='streamable_http'\nurl='https://mcp.example.test/'\napi_key='synthetic-private-inline'\n").expect("unknown field config");
  let error = load_with_environment(data.path(), None, &BTreeMap::new())
    .expect_err("inline secret field rejected");
  assert!(!format!("{error:#}").contains("synthetic-private-inline"));
}
