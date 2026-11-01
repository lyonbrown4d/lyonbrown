mod support;
use agent_test_support::{MockModelServer, StandardProtocol, text_reply, tool_reply_with_id};
use agent_workspace::Workspace;
use serde_json::{Value, json};
use support::TestNode;

fn route(tools: &[&str]) -> String {
  json!({"task_kind":"code","model_policy":"general","allowed_tools":tools,"execution_mode":"local","target_hint":null,"risk_level":"low","confidence":0.99}).to_string()
}
async fn approval(node: &TestNode, run: &str) -> agent_protocol::v1::Approval {
  tokio::time::timeout(std::time::Duration::from_secs(5), async {
    let mut stream = node.client.watch_run(run, 0).await.unwrap();
    while stream.message().await.unwrap().is_some() {
      if let Some(approval) = node.client.list_approvals(run).await.unwrap().first() {
        return approval.clone();
      }
    }
    panic!("run ended before approval");
  })
  .await
  .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn model_discovers_searches_and_reads_scoped_rules_through_every_protocol() {
  for protocol in StandardProtocol::ALL {
    let classifier = MockModelServer::start(vec![text_reply(
      protocol,
      &route(&["read_file", "list_files", "search_files"]),
    )])
    .await
    .unwrap();
    let main = MockModelServer::start(vec![
      tool_reply_with_id(protocol, "list", "list_files", json!({"path":"src"})),
      tool_reply_with_id(
        protocol,
        "search_none",
        "search_files",
        json!({"path":"src","query":"absent literal"}),
      ),
      tool_reply_with_id(
        protocol,
        "search",
        "search_files",
        json!({"path":"src","query":"unique marker"}),
      ),
      tool_reply_with_id(
        protocol,
        "read",
        "read_file",
        json!({"path":"src/notes.txt"}),
      ),
      text_reply(protocol, "found marker and rules"),
    ])
    .await
    .unwrap();
    let node = TestNode::start(&main, &classifier, protocol).await;
    let workspace = Workspace::open(node.workspace.path()).unwrap();
    workspace.write("AGENTS.md", "Root fixture rules.").unwrap();
    workspace
      .write("src/AGENTS.md", "Nested fixture rules.")
      .unwrap();
    let revision = workspace
      .write_revision("src/notes.txt", "first\nunique marker\n", "")
      .unwrap();
    let run = node
      .run("Find the marker under src and inspect its file")
      .await;
    let terminal = node.terminal(&run.id).await;
    assert_eq!(terminal.status, "completed", "{}", terminal.error);
    assert_eq!(terminal.result, "found marker and rules");
    let requests = main.requests().await;
    assert_eq!(requests.len(), 5, "{protocol:?}");
    assert!(requests[1].body.to_string().contains("src/notes.txt"));
    assert!(
      requests[1]
        .body
        .to_string()
        .contains("Nested fixture rules.")
    );
    assert!(requests[3].body.to_string().contains("unique marker"));
    assert!(requests[3].body.to_string().contains("searched_files"));
    assert!(requests[4].body.to_string().contains(&revision));
    assert_eq!(
      workspace.read("src/notes.txt").unwrap(),
      "first\nunique marker\n"
    );
    assert!(
      node
        .client
        .list_approvals(&run.id)
        .await
        .unwrap()
        .is_empty()
    );
    let events = node.daemon.store.events(&run.id, 0, 1000).unwrap();
    let finished = events
      .iter()
      .filter(|event| event.kind == "tool_finished")
      .map(|event| serde_json::from_str::<Value>(&event.payload_json).unwrap())
      .collect::<Vec<_>>();
    assert_eq!(finished.len(), 4);
    assert!(finished.iter().all(|event| event["is_error"] == false));
    let missing = finished
      .iter()
      .find(|event| event["name"] == "search_files")
      .unwrap();
    let missing: Value = serde_json::from_str(missing["text"].as_str().unwrap()).unwrap();
    assert_eq!(missing["matches"], json!([]));
    assert_eq!(
      missing["repository_instructions"][1]["content"],
      "Nested fixture rules."
    );
    node.close().await;
  }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn approved_model_write_conflicts_with_editor_revision_without_overwrite_for_every_protocol()
{
  for protocol in StandardProtocol::ALL {
    let classifier = MockModelServer::start(vec![text_reply(
      protocol,
      &route(&["read_file", "write_file"]),
    )])
    .await
    .unwrap();
    let main = MockModelServer::start(vec![]).await.unwrap();
    let node = TestNode::start(&main, &classifier, protocol).await;
    let workspace = Workspace::open(node.workspace.path()).unwrap();
    let revision = workspace
      .write_revision("notes.txt", "original", "")
      .unwrap();
    let arguments =
      json!({"path":"notes.txt","content":"model replacement","expected_revision":revision});
    main
      .push_reply(tool_reply_with_id(
        protocol,
        "read",
        "read_file",
        json!({"path":"notes.txt"}),
      ))
      .await;
    main
      .push_reply(tool_reply_with_id(
        protocol,
        "write",
        "write_file",
        arguments.clone(),
      ))
      .await;
    main
      .push_reply(text_reply(protocol, "conflict requires a fresh read"))
      .await;
    let run = node.run("Read notes.txt then propose a replacement").await;
    let approval = approval(&node, &run.id).await;
    assert_eq!(
      serde_json::from_str::<Value>(&approval.arguments_json).unwrap(),
      arguments
    );
    assert_eq!(workspace.read("notes.txt").unwrap(), "original");
    // The actual editor RPC shares the daemon lock with model writes.
    node
      .client
      .write_file(&node.session_id, "notes.txt", "editor wins", &revision)
      .await
      .unwrap();
    node.client.approve(&approval.id, true).await.unwrap();
    let terminal = node.terminal(&run.id).await;
    assert_eq!(terminal.status, "completed", "{}", terminal.error);
    assert_eq!(terminal.result, "conflict requires a fresh read");
    assert_eq!(workspace.read("notes.txt").unwrap(), "editor wins");
    let requests = main.requests().await;
    assert_eq!(requests.len(), 3);
    assert!(
      requests[1].body.to_string().contains(&revision),
      "read exposes exact revision"
    );
    assert!(
      requests[2]
        .body
        .to_string()
        .contains("file revision conflict")
    );
    assert!(!requests[2].body.to_string().contains("File written."));
    let events = node.daemon.store.events(&run.id, 0, 1000).unwrap();
    let write = events
      .iter()
      .filter(|event| event.kind == "tool_finished")
      .map(|event| serde_json::from_str::<Value>(&event.payload_json).unwrap())
      .find(|event| event["name"] == "write_file")
      .unwrap();
    assert_eq!(write["is_error"], true);
    assert!(
      node
        .client
        .list_approvals(&run.id)
        .await
        .unwrap()
        .iter()
        .all(|approval| approval.status == "consumed")
    );
    node.close().await;
  }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn model_write_without_valid_revision_never_requests_approval_or_changes_content() {
  for protocol in StandardProtocol::ALL {
    let classifier = MockModelServer::start(vec![text_reply(protocol, &route(&["write_file"]))])
      .await
      .unwrap();
    let main = MockModelServer::start(vec![
      tool_reply_with_id(
        protocol,
        "missing",
        "write_file",
        json!({"path":"existing.txt","content":"unversioned"}),
      ),
      tool_reply_with_id(
        protocol,
        "malformed",
        "write_file",
        json!({"path":"existing.txt","content":"malformed","expected_revision":"forged"}),
      ),
      text_reply(protocol, "must read before writing"),
    ])
    .await
    .unwrap();
    let node = TestNode::start(&main, &classifier, protocol).await;
    std::fs::write(node.workspace.path().join("existing.txt"), "original").unwrap();
    let run = node.run("Attempt an unversioned write").await;
    let terminal = node.terminal(&run.id).await;
    assert_eq!(terminal.status, "completed", "{}", terminal.error);
    assert_eq!(
      std::fs::read_to_string(node.workspace.path().join("existing.txt")).unwrap(),
      "original"
    );
    assert!(
      node
        .client
        .list_approvals(&run.id)
        .await
        .unwrap()
        .is_empty()
    );
    let requests = main.requests().await;
    assert_eq!(requests.len(), 3);
    assert!(requests[1].body.to_string().contains("expected_revision"));
    assert!(!requests[2].body.to_string().contains("File written."));
    node.close().await;
  }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn direct_model_write_cannot_bypass_malformed_nested_rules() {
  for protocol in StandardProtocol::ALL {
    let classifier = MockModelServer::start(vec![text_reply(protocol, &route(&["write_file"]))])
      .await
      .unwrap();
    let main = MockModelServer::start(vec![
      tool_reply_with_id(
        protocol,
        "direct",
        "write_file",
        json!({"path":"src/new.txt","content":"must not exist","expected_revision":""}),
      ),
      text_reply(protocol, "repository guidance must be fixed first"),
    ])
    .await
    .unwrap();
    let node = TestNode::start(&main, &classifier, protocol).await;
    std::fs::create_dir(node.workspace.path().join("src")).unwrap();
    std::fs::write(node.workspace.path().join("src/AGENTS.md"), b"invalid\xff").unwrap();
    let run = node.run("Create src/new.txt directly").await;
    let terminal = node.terminal(&run.id).await;
    assert_eq!(terminal.status, "completed", "{}", terminal.error);
    assert!(!node.workspace.path().join("src/new.txt").exists());
    assert_eq!(
      std::fs::read(node.workspace.path().join("src/AGENTS.md")).unwrap(),
      b"invalid\xff"
    );
    assert!(
      node
        .client
        .list_approvals(&run.id)
        .await
        .unwrap()
        .is_empty()
    );
    assert!(
      main.requests().await[1]
        .body
        .to_string()
        .contains("src/AGENTS.md")
    );
    node.close().await;
  }
}
