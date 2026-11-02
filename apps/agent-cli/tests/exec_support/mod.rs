use crate::support::node::Node;
use serde_json::Value;
use std::process::{Output, Stdio};
use tokio::{
  io::AsyncWriteExt as _,
  process::{Child, Command},
};

pub fn command(node: &Node, args: &[&str]) -> Command {
  command_at(node, &node.endpoint(), args)
}

pub fn command_at(node: &Node, endpoint: &str, args: &[&str]) -> Command {
  let mut command = Command::new(env!("CARGO_BIN_EXE_agent-cli"));
  command
    .args([
      "--endpoint",
      endpoint,
      "--data-dir",
      node.data.path().to_str().unwrap(),
    ])
    .arg("exec")
    .args(args)
    .current_dir(node.workspace.path())
    .env("LYONBROWN_TOKEN", node.daemon.auth_token())
    .env("RUST_LOG", "warn")
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .kill_on_drop(true);
  for name in [
    "HTTP_PROXY",
    "HTTPS_PROXY",
    "ALL_PROXY",
    "http_proxy",
    "https_proxy",
    "all_proxy",
    "LYONBROWN_CONFIG",
  ] {
    command.env_remove(name);
  }
  command
}

pub async fn invoke(node: &Node, args: &[&str], input: &[u8]) -> Output {
  let mut child = command(node, args).spawn().unwrap();
  child.stdin.take().unwrap().write_all(input).await.unwrap();
  finish(child).await
}

pub async fn finish(child: Child) -> Output {
  tokio::time::timeout(std::time::Duration::from_secs(25), child.wait_with_output())
    .await
    .expect("exec must terminate")
    .unwrap()
}

pub fn records(output: &Output) -> Vec<Value> {
  let text = std::str::from_utf8(&output.stdout).unwrap();
  assert!(!text.contains('\x1b'));
  text
    .lines()
    .map(|line| serde_json::from_str(line).expect("each stdout line is JSON"))
    .collect()
}
