mod command_tool_support;
#[allow(dead_code)]
mod support;
use agent_storage::OperationStatus;
use agent_test_support::{MockModelServer, StandardProtocol, text_reply, tool_reply};
use command_tool_support::{approval, command, exact, fixture, route, start, wait_ready};
use serde_json::json;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn protocols_execute_only_exact_approved_commands_and_persist_receipts() {
  for protocol in StandardProtocol::ALL {
    for approved in [false, true] {
      let classifier = MockModelServer::start(vec![text_reply(protocol, &route())])
        .await
        .unwrap();
      let main = MockModelServer::start(vec![
        tool_reply(
          protocol,
          "execute_command",
          command(json!(["mark", "effect"])),
        ),
        text_reply(protocol, "finished"),
      ])
      .await
      .unwrap();
      let node = start(&main, &classifier, protocol).await;
      let run = node.run("execute trusted fixture").await;
      let pending = approval(&node, &run.id).await;
      let exact = exact(&pending);
      assert_eq!(exact["session_id"], node.session_id);
      assert_eq!(exact["run_id"], run.id);
      assert_eq!(exact["owner_epoch"], 1);
      assert_eq!(exact["executable"], json!(fixture()));
      assert_eq!(exact["arguments"], json!(["mark", "effect"]));
      assert_eq!(
        exact["cwd"],
        json!(node.workspace.path().canonicalize().unwrap())
      );
      assert_eq!(exact["timeout_ms"], 5000);
      assert_eq!(exact["output_limit_bytes"], 16384);
      let action = exact["action_id"].as_str().unwrap();
      assert!(!node.workspace.path().join("effect").exists());
      assert!(node.daemon.store.operation(action).unwrap().is_none());
      node.client.approve(&pending.id, approved).await.unwrap();
      let terminal = node.terminal(&run.id).await;
      assert_eq!(terminal.status, "completed", "{}", terminal.error);
      assert_eq!(node.workspace.path().join("effect").exists(), approved);
      assert_eq!(
        node
          .daemon
          .store
          .approval(&pending.id)
          .unwrap()
          .unwrap()
          .status,
        if approved { "consumed" } else { "denied" }
      );
      assert_eq!(
        node
          .daemon
          .store
          .operation(action)
          .unwrap()
          .map(|record| record.status),
        approved.then_some(OperationStatus::Completed)
      );
      node.daemon.recover().await.unwrap();
      assert_eq!(main.requests().await.len(), 2);
      node.close().await;
    }
  }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn direct_arguments_selected_environment_and_output_reach_the_model() {
  let protocol = StandardProtocol::ChatCompletions;
  let classifier = MockModelServer::start(vec![text_reply(protocol, &route())])
    .await
    .unwrap();
  let main = MockModelServer::start(vec![
    tool_reply(
      protocol,
      "execute_command",
      command(json!(["report", "空 白;$(touch injected)"])),
    ),
    text_reply(protocol, "done"),
  ])
  .await
  .unwrap();
  let node = start(&main, &classifier, protocol).await;
  let run = node.run("report exact args and environment").await;
  let pending = approval(&node, &run.id).await;
  assert!(!pending.arguments_json.contains("chosen"));
  node.client.approve(&pending.id, true).await.unwrap();
  assert_eq!(node.terminal(&run.id).await.status, "completed");
  let output = main.requests().await[1].body.to_string();
  assert!(output.contains("arg:空 白;$(touch injected)"));
  assert!(output.contains("selected:chosen"));
  assert!(output.contains("ambient:"));
  if let Ok(home) = std::env::var("HOME")
    && !home.is_empty()
  {
    assert!(!output.contains(&home));
  }
  assert!(output.contains("untrusted_process_output"));
  assert!(!node.workspace.path().join("injected").exists());
  let events = node.daemon.store.events(&run.id, 0, 1000).unwrap();
  let receipts: Vec<_> = events
    .iter()
    .filter(|event| event.kind.starts_with("operation_"))
    .collect();
  assert_eq!(receipts.len(), 2);
  assert!(
    receipts
      .iter()
      .all(|event| !event.payload_json.contains("chosen")
        && !event.payload_json.contains("touch injected"))
  );
  node.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pending_cancel_and_unallowlisted_program_have_no_effect_or_intent() {
  for cancel in [false, true] {
    let protocol = StandardProtocol::ChatCompletions;
    let classifier = MockModelServer::start(vec![text_reply(protocol, &route())])
      .await
      .unwrap();
    let mut arguments = command(json!(["mark", "effect"]));
    if !cancel {
      arguments["executable"] = json!(std::env::current_exe().unwrap());
    }
    let main = MockModelServer::start(vec![
      tool_reply(protocol, "execute_command", arguments),
      text_reply(protocol, "stopped"),
    ])
    .await
    .unwrap();
    let node = start(&main, &classifier, protocol).await;
    let run = node.run("command must not execute").await;
    if cancel {
      let pending = approval(&node, &run.id).await;
      node.client.cancel_run(&run.id).await.unwrap();
      assert_eq!(node.terminal(&run.id).await.status, "cancelled");
      assert!(
        node
          .daemon
          .store
          .operation(exact(&pending)["action_id"].as_str().unwrap())
          .unwrap()
          .is_none()
      );
    } else {
      assert_eq!(node.terminal(&run.id).await.status, "completed");
      assert!(
        node
          .client
          .list_approvals(&run.id)
          .await
          .unwrap()
          .is_empty()
      );
    }
    assert!(!node.workspace.path().join("effect").exists());
    node.close().await;
  }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn command_timeout_output_budget_and_dispatch_cancel_are_durably_uncertain() {
  for mode in ["deadline", "output", "cancel"] {
    let protocol = StandardProtocol::ChatCompletions;
    let classifier = MockModelServer::start(vec![text_reply(protocol, &route())])
      .await
      .unwrap();
    let mut arguments = if mode == "output" {
      command(json!(["flood", "effect"]))
    } else {
      command(json!(["tree", "ready", "effect"]))
    };
    if mode == "deadline" {
      arguments["timeout_ms"] = json!(800);
    }
    if mode == "output" {
      arguments["output_limit_bytes"] = json!(1000);
    }
    let main = MockModelServer::start(vec![tool_reply(protocol, "execute_command", arguments)])
      .await
      .unwrap();
    let node = start(&main, &classifier, protocol).await;
    let run = node.run("interrupt command fixture").await;
    let pending = approval(&node, &run.id).await;
    node.client.approve(&pending.id, true).await.unwrap();
    if mode == "cancel" {
      wait_ready(&node.workspace.path().join("ready")).await;
      node.client.cancel_run(&run.id).await.unwrap();
    }
    let terminal = node.terminal(&run.id).await;
    assert_eq!(terminal.status, "uncertain", "{mode}: {}", terminal.error);
    let record = node
      .daemon
      .store
      .operation(exact(&pending)["action_id"].as_str().unwrap())
      .unwrap()
      .unwrap();
    assert_eq!(record.status, OperationStatus::Uncertain);
    let receipt = record.result_summary.unwrap();
    assert_eq!(
      receipt["reason"],
      match mode {
        "deadline" => "deadline",
        "output" => "output_limit",
        _ => "cancelled",
      }
    );
    if mode == "output" {
      assert_eq!(receipt["output_truncated"], true);
      assert_eq!(
        receipt["stdout_bytes"].as_u64().unwrap() + receipt["stderr_bytes"].as_u64().unwrap(),
        1000
      );
    }
    assert!(!node.workspace.path().join("effect").exists());
    assert_eq!(main.requests().await.len(), 1);
    node.daemon.recover().await.unwrap();
    assert!(
      node
        .client
        .start_run(&node.session_id, "do not replay", &agent_core::new_id())
        .await
        .is_err()
    );
    assert!(!node.workspace.path().join("effect").exists());
    node.close().await;
  }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn default_empty_allowlist_does_not_advertise_commands() {
  let protocol = StandardProtocol::ChatCompletions;
  let classifier = MockModelServer::start(vec![text_reply(protocol, &route())])
    .await
    .unwrap();
  let main = MockModelServer::start(vec![text_reply(protocol, "commands unavailable")])
    .await
    .unwrap();
  let node = support::TestNode::start(&main, &classifier, protocol).await;
  let run = node.run("execute command").await;
  assert_eq!(node.terminal(&run.id).await.status, "completed");
  assert!(
    !main.requests().await[0].body["tools"]
      .to_string()
      .contains("execute_command")
  );
  assert!(
    node
      .client
      .list_approvals(&run.id)
      .await
      .unwrap()
      .is_empty()
  );
  node.close().await;
}
