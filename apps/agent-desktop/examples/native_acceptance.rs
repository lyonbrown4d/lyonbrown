//! Visible native acceptance fixture; the UI and daemon are production implementations.
//! Model replies come from the dev-only loopback HTTP fixture, never a paid provider.
use agent_desktop::{Desktop, DesktopConfiguration, DesktopModule};
use agent_test_support::{MockModelServer, StandardProtocol, text_reply, tool_reply};
use clap::Parser as _;
use gpui_kit::*;
use std::{collections::BTreeMap, sync::Arc};
use tokio_stream::wrappers::TcpListenerStream;

fn main() -> anyhow::Result<std::process::ExitCode> {
  #[cfg(debug_assertions)]
  let entry = agent_browser_runtime::dispatch_acceptance_entry()?;
  #[cfg(not(debug_assertions))]
  let entry = agent_browser_runtime::dispatch_entry()?;
  if let Some(code) = entry {
    return Ok(code);
  }
  let directory = tempfile::tempdir()?;
  let workspace = directory.path().join("workspace");
  std::fs::create_dir_all(&workspace)?;
  let readme = workspace.join("README.md");
  std::fs::write(&readme, "# Native workspace\n\nOriginal tracked content.\n")?;
  for args in [
    vec!["init", "--quiet"],
    vec!["add", "README.md"],
    vec![
      "-c",
      "user.name=Fixture",
      "-c",
      "user.email=fixture@example.invalid",
      "commit",
      "--quiet",
      "-m",
      "Native acceptance fixture",
    ],
  ] {
    anyhow::ensure!(
      std::process::Command::new("git")
        .current_dir(&workspace)
        .args(args)
        .status()?
        .success(),
      "create native acceptance Git workspace"
    );
  }
  std::fs::write(
    readme,
    "# Native workspace\n\nA real change for the native Diff workbench.\n",
  )?;
  std::env::set_current_dir(&workspace)?;
  let runtime = Arc::new(
    tokio::runtime::Builder::new_multi_thread()
      .worker_threads(3)
      .enable_all()
      .build()?,
  );
  let (model,daemon,endpoint,browser_url)=runtime.block_on(async {
    let protocol=StandardProtocol::ChatCompletions;
    let model=MockModelServer::start(vec![
      text_reply(protocol,"## Native conversation\n\nThis response streamed through the authenticated daemon into the **GPUI Markdown view**.\n\n- Sessions and jobs remain in agentd.\n- The composer and keyboard focus are native.\n\nAsk for a workspace note next to inspect a real tool approval."),
      tool_reply(protocol,"write_file",serde_json::json!({"path":"notes.txt","content":"Created by a real daemon tool after native approval.\n"})),
      text_reply(protocol,"The workspace note was written after your approval. Open **Files** to read or edit `notes.txt`; the **Diff** tab shows the tracked README change."),
      text_reply(protocol,"The native acceptance fixture has completed its scripted model replies. Files and the actual CEF browser remain available."),
    ]).await?;
    let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let endpoint=format!("http://{}",listener.local_addr()?);
    let mut config=agentd::config::Config::try_parse_from([
      "agentd","--listen",&listener.local_addr()?.to_string(),"--data-dir",directory.path().to_str().ok_or_else(||anyhow::anyhow!("UTF-8 fixture path"))?,
      "--provider",protocol.provider(),"--model","native-acceptance-fixture","--api-base",model.base_url(),"--run-timeout-secs","60"
    ])?;
    config.provider_api_key=Some("synthetic-model-key".into());
    let daemon=agentd::Daemon::open(config).await?;
    let client_service=agentd::client_service(daemon.clone());
    let workspace_service=agentd::workspace_service(daemon.clone());
    let browser_service=agentd::browser_service(daemon.clone());
    let runtime_service=agentd::runtime_service(daemon.clone());
    tokio::spawn(async move {
      if let Err(error)=tonic::transport::Server::builder().add_service(client_service).add_service(workspace_service).add_service(browser_service).add_service(runtime_service).serve_with_incoming(TcpListenerStream::new(listener)).await {
        tracing::error!(event="native_acceptance_server_failed",error=%error,"Local acceptance server failed");
      }
    });
    let browser_url=browser_fixture().await?;
    anyhow::Ok((model,daemon,endpoint,browser_url))
  })?;
  let settings = agent_config::load_with_environment(
    directory.path(),
    None,
    &BTreeMap::from([
      ("LYONBROWN_ENDPOINT".into(), endpoint),
      ("LYONBROWN_TOKEN".into(), daemon.auth_token().into()),
    ]),
  )?;
  let _logs = agent_telemetry::init(directory.path(), "native-acceptance", "info")?;
  let module = DesktopModule::new(fluxdi::Shared::new(DesktopConfiguration {
    settings: settings.clone(),
    data_dir: directory.path().into(),
    browser_executable: None,
  }));
  let configuration = module.configuration()?;
  let connector = module.client()?;
  let browser = module.browser()?;
  let computer = module.computer()?;
  std::fs::write(
    directory.path().join("acceptance.json"),
    serde_json::to_vec_pretty(&serde_json::json!({
      "browser_fixture_url": browser_url,
      "endpoint": settings.endpoint,
      "workspace": workspace,
      "scope": "local synthetic model and browser fixtures; no credentials",
    }))?,
  )?;
  println!("Native acceptance workspace: {}", workspace.display());
  println!(
    "Native acceptance data directory: {}",
    directory.path().display()
  );
  println!("Native acceptance endpoint: {}", settings.endpoint);
  println!("Native browser fixture URL: {browser_url}");
  gpui_kit::application().with_assets(gpui_kit::assets::Assets).run({
    let runtime=runtime.clone();
    move|cx| {
      agent_desktop::init(cx);
      cx.on_window_closed(|cx,_|if cx.windows().is_empty(){cx.quit();}).detach();
      if let Err(error)=gpui_kit::open_window(WindowOptions {window_bounds:Some(WindowBounds::Windowed(Bounds::centered(None,size(px(1440.),px(920.)),cx))),..gpui_kit::component::TitleBar::window_options()},cx,move|window,cx| {
        window.set_window_title("Lyonbrown · native acceptance · local model fixture");window.activate_window();
        cx.new(|cx|Desktop::new(runtime,connector,browser,computer,configuration,window,cx))
      }) {tracing::error!(event="native_acceptance_window_failed",error=%error,"Native acceptance window failed");cx.quit();}
      cx.activate(true);
    }
  });
  runtime.block_on(daemon.shutdown());
  drop(model);
  Ok(std::process::ExitCode::SUCCESS)
}

async fn browser_fixture() -> std::io::Result<String> {
  use tokio::io::{AsyncReadExt, AsyncWriteExt};
  let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
  let url = format!("http://{}/", listener.local_addr()?);
  tokio::spawn(async move {
    while let Ok((mut socket, _)) = listener.accept().await {
      tokio::spawn(async move {
        let mut request = [0; 4096];
        if socket.read(&mut request).await.unwrap_or(0) == 0 {
          return;
        }
        let page = r#"<!doctype html><meta charset="utf-8"><title>Native CEF acceptance</title>
<style>body{font:16px system-ui;background:#f7f8fa;color:#18202b;margin:0}main{padding:28px;max-width:560px}h1{font-size:24px}p{line-height:1.6}input{box-sizing:border-box;width:100%;padding:12px;border:1px solid #bcc4cf;border-radius:8px;font:inherit}button{margin-top:16px;padding:10px 16px;background:#18202b;color:white;border:0;border-radius:8px;font:inherit}</style>
<main><h1>Native browser</h1><p>This local acceptance page is rendered by real Chromium in the bundled CEF runtime.</p><label>Human input<input aria-label="Human input" placeholder="Take control, then type here"></label><button onclick="document.getElementById('result').textContent='Click reached real Chromium.'">Verify click</button><p id="result">Ready for native takeover.</p></main>"#;
        let response = format!(
          "HTTP/1.1 200 OK\r\nContent-Type: text/html;charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{page}",
          page.len()
        );
        let _ = socket.write_all(response.as_bytes()).await;
      });
    }
  });
  Ok(url)
}
