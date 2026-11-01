#[allow(dead_code)]
mod support;

use agent_test_support::MockModelServer;
use support::{node::Node, pty::Terminal};

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn terminal_model_skill_git_and_credential_workflows_use_daemon_without_inference() {
  let model = MockModelServer::start(vec![]).await.unwrap();
  let node = Node::start(&model).await;
  let mut terminal = Terminal::start_with_environment(
    &node.endpoint(),
    node.daemon.auth_token(),
    node.data.path(),
    node.workspace.path(),
    &[("OPENAI_API_KEY", "synthetic-upload-fixture")],
  );
  terminal.screen("Connected").await;
  terminal.send("\x0e");
  terminal.screen("New conversation").await;
  let session = node.client.list_sessions().await.unwrap().pop().unwrap();

  terminal.paste("/models");
  terminal.send("\r");
  terminal.screen("deepseek-chat").await;
  terminal.snapshot("unified-model-catalog");

  terminal.paste("/models select terminal-fixture configured");
  terminal.send("\r");
  terminal.screen("Session model selected").await;

  let git = std::process::Command::new("git")
    .args(["init", "--quiet"])
    .current_dir(node.workspace.path())
    .status()
    .unwrap();
  assert!(git.success());
  std::fs::write(node.workspace.path().join("tracked.txt"), "before\n").unwrap();
  assert!(
    std::process::Command::new("git")
      .args(["add", "tracked.txt"])
      .current_dir(node.workspace.path())
      .status()
      .unwrap()
      .success()
  );
  assert!(
    std::process::Command::new("git")
      .args([
        "-c",
        "user.name=Acceptance",
        "-c",
        "user.email=acceptance@example.invalid",
        "commit",
        "--quiet",
        "-m",
        "fixture"
      ])
      .current_dir(node.workspace.path())
      .status()
      .unwrap()
      .success()
  );
  std::fs::write(node.workspace.path().join("tracked.txt"), "after\n").unwrap();
  std::fs::write(node.workspace.path().join("fixture.txt"), "untracked").unwrap();
  terminal.paste("/git");
  terminal.send("\r");
  terminal.screen("fixture.txt").await;
  terminal.snapshot("git-status");
  terminal.paste("/diff tracked.txt");
  terminal.send("\r");
  terminal.screen("+after").await;
  terminal.snapshot("git-diff");

  let source = node.workspace.path().join("acceptance-skill");
  std::fs::create_dir(&source).unwrap();
  std::fs::write(source.join("SKILL.md"), "---\nname: acceptance-skill\ndescription: Terminal acceptance fixture\n---\nRead this instruction.\n").unwrap();
  let source = source.to_str().unwrap();
  terminal.paste(&format!("/skills inspect {source} daemon_user"));
  terminal.send("\r");
  terminal.screen("Skill inspection").await;
  terminal.screen("acceptance-skill").await;
  terminal.snapshot("skill-inspection");
  let inspection = node
    .client
    .inspect_skill(source, "daemon_user", &session.id)
    .await
    .unwrap();
  let hash = inspection.skill.unwrap().content_sha256;
  terminal.paste(&format!("/skills install {source} daemon_user {hash}"));
  terminal.send("\r");
  terminal.screen("Skill installed").await;
  assert_eq!(
    node
      .client
      .list_skills("daemon_user", &session.id)
      .await
      .unwrap()
      .len(),
    1
  );
  terminal.snapshot("skill-installed");
  terminal.paste(&format!(
    "/skills remove acceptance-skill daemon_user {hash}"
  ));
  terminal.send("\r");
  terminal.screen("Skill removed").await;
  assert!(
    node
      .client
      .list_skills("daemon_user", &session.id)
      .await
      .unwrap()
      .is_empty()
  );

  terminal.paste("/providers upload openai OPENAI_API_KEY");
  terminal.send("\r");
  terminal.screen("Provider credential uploaded").await;
  assert!(node.client.list_providers().await.unwrap().iter().any(|provider| provider.id == "openai" && provider.credential_source == "uploaded_encrypted"));
  terminal.paste("/providers");
  terminal.send("\r");
  terminal.screen("OpenAI").await;
  assert!(!terminal.contents().contains("synthetic-upload-fixture"));
  assert!(
    model.requests().await.is_empty(),
    "Resource workflows must not invoke inference"
  );
  assert!(node.client.list_runs(&session.id).await.unwrap().is_empty());
  terminal.quit().await;
  node.close().await;
}
