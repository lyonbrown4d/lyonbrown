mod delegation_support;
#[allow(dead_code)]
mod peer_support;

use agent_test_support::{MockModelServer, StandardProtocol, text_reply, tool_reply};
use delegation_support::{Delegation, dispatch};
use serde_json::{Value, json};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn every_protocol_delegates_after_exact_approval_and_separately_accepts_reconciled_result() {
  for protocol in StandardProtocol::ALL {
    let main = MockModelServer::start_interactive().await.unwrap();
    let worker = MockModelServer::start(vec![text_reply(protocol, "untrusted worker summary")])
      .await
      .unwrap();
    let node = Delegation::open(&main, &worker, protocol).await;
    main
      .push_reply(tool_reply(
        protocol,
        "delegate_task",
        dispatch(&node.pair.worker.node_id),
      ))
      .await;
    let run = node.run().await;
    let approval = node.approval(&run.id, 1).await;
    let exact: Value = serde_json::from_str(&approval.arguments_json).unwrap();
    let id = exact["envelope"]["task_id"].as_str().unwrap();
    assert_eq!(exact["target"]["id"], node.pair.worker.node_id);
    assert_eq!(
      exact["target"]["pairing_fingerprint"],
      node
        .pair
        .worker_manager
        .public_identity()
        .pairing_fingerprint()
        .unwrap()
    );
    assert_eq!(exact["envelope"]["parent_run_id"], run.id);
    assert_eq!(exact["envelope"]["session_id"], node.pair.session.id);
    assert_eq!(
      exact["envelope"]["owner_epoch"],
      node.pair.session.owner_epoch
    );
    assert!(node.pair.master.store.task(id).unwrap().is_none());
    assert!(worker.requests().await.is_empty());
    node.client.approve(&approval.id, true).await.unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
      while node.pair.master.store.task(id).unwrap().is_none() {
        tokio::task::yield_now().await;
      }
    })
    .await
    .unwrap();
    let task = node.pair.terminal(id).await;
    assert_eq!(task.status, "awaiting_acceptance");
    assert_eq!(task.attempt_status, "succeeded");
    main
      .push_reply(tool_reply(
        protocol,
        "delegate_task",
        json!({"call":{
          "op":"accept","target_id":node.pair.worker.node_id,"task_id":id,"accepted":true
        }}),
      ))
      .await;
    let approval = node.approval(&run.id, 2).await;
    let accepted: Value = serde_json::from_str(&approval.arguments_json).unwrap();
    assert_eq!(accepted["envelope"], exact["envelope"]);
    assert_eq!(accepted["result"], "untrusted worker summary");
    assert_eq!(accepted["accepted"], true);
    assert_eq!(accepted["result_digest"].as_str().unwrap().len(), 64);
    assert_eq!(
      node.pair.master.store.task(id).unwrap().unwrap().status,
      "awaiting_acceptance"
    );
    main
      .push_reply(text_reply(
        protocol,
        "Master explicitly accepted the result",
      ))
      .await;
    node.client.approve(&approval.id, true).await.unwrap();
    assert_eq!(node.terminal(&run.id).await.status, "completed");
    let task = node.pair.master.store.task(id).unwrap().unwrap();
    assert_eq!(task.status, "completed");
    assert_eq!(node.pair.worker_runs(&task.envelope).len(), 1);
    assert_eq!(worker.requests().await.len(), 1);
    assert_eq!(worker.requests().await[0].path, protocol.path());
    let requests = main.requests().await;
    assert_eq!(requests.len(), 3);
    assert!(
      requests
        .iter()
        .all(|request| request.path == protocol.path())
    );
    let schema = requests[0].body["tools"]
      .as_array()
      .unwrap()
      .iter()
      .find_map(|tool| {
        let tool = tool.get("function").unwrap_or(tool);
        (tool["name"] == "delegate_task").then(|| {
          tool
            .get("parameters")
            .or_else(|| tool.get("input_schema"))
            .unwrap()
        })
      })
      .unwrap();
    assert_eq!(schema["type"], "object");
    assert_eq!(
      schema["properties"]["call"]["oneOf"]
        .as_array()
        .unwrap()
        .len(),
      5
    );
    assert!(!schema.to_string().contains("endpoint"));
    assert!(
      requests[1]
        .body
        .to_string()
        .contains("untrusted_worker_content")
    );
    node.close().await;
  }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn denial_revocation_stale_owner_and_pending_cancellation_never_dispatch_a_task() {
  for scenario in ["deny", "revoke", "rekey", "owner", "cancel"] {
    let protocol = StandardProtocol::ChatCompletions;
    let main = MockModelServer::start_interactive().await.unwrap();
    let worker = MockModelServer::start(Vec::new()).await.unwrap();
    let node = Delegation::open(&main, &worker, protocol).await;
    main
      .push_reply(tool_reply(
        protocol,
        "delegate_task",
        dispatch(&node.pair.worker.node_id),
      ))
      .await;
    main
      .push_reply(text_reply(protocol, "Stopped without dispatch"))
      .await;
    let run = node.run().await;
    let approval = node.approval(&run.id, 1).await;
    let exact: Value = serde_json::from_str(&approval.arguments_json).unwrap();
    let id = exact["envelope"]["task_id"].as_str().unwrap();
    match scenario {
      "revoke" => {
        node
          .pair
          .master_manager
          .revoke_local(&node.pair.worker.node_id)
          .unwrap();
      }
      "owner" => {
        node
          .pair
          .master
          .store
          .change_owner(&node.pair.session.id, 1, "new-master")
          .unwrap();
      }
      "rekey" => {
        let mut identity = node.pair.worker_manager.public_identity();
        identity.noise_public_key = "ab".repeat(32);
        let capabilities = node
          .pair
          .master_manager
          .trust(&node.pair.worker.node_id)
          .unwrap()
          .capabilities;
        node
          .pair
          .master_manager
          .pair_local(identity, capabilities)
          .unwrap();
      }
      "cancel" => {
        node.client.cancel_run(&run.id).await.unwrap();
      }
      _ => {}
    }
    if scenario != "cancel" {
      node
        .client
        .approve(&approval.id, scenario != "deny")
        .await
        .unwrap();
    }
    if scenario == "owner" {
      tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while node.client.node().await.unwrap().active_runs != 0 {
          tokio::task::yield_now().await;
        }
      })
      .await
      .unwrap();
    } else {
      node.terminal(&run.id).await;
    }
    assert!(
      node.pair.master.store.task(id).unwrap().is_none(),
      "{scenario}"
    );
    assert!(worker.requests().await.is_empty(), "{scenario}");
    if scenario == "rekey" {
      assert!(
        main
          .requests()
          .await
          .last()
          .unwrap()
          .body
          .to_string()
          .contains("pairing changed after approval")
      );
    }
    node.close().await;
  }
}
