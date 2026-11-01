//! Visible native acceptance using only synthetic loopback models and paired daemons.
#![cfg_attr(not(unix), allow(unused_imports))]
#[cfg(unix)]
#[path = "../../agentd/tests/peer_support/mod.rs"]
#[allow(dead_code)]
mod peer_support;
#[cfg(unix)]
#[path = "../tests/recovery_support/mod.rs"]
mod recovery_support;

#[cfg(unix)]
gpui_kit::actions!(recovery_acceptance, [QuitAcceptance]);
#[cfg(unix)]
#[path = "../../agentd/tests/remote_support/mod.rs"]
#[allow(dead_code)]
mod remote_support;

#[cfg(not(unix))]
fn main() -> anyhow::Result<()> {
  anyhow::bail!("this acceptance fixture requires Unix /bin/sh; it does not verify Windows")
}

#[cfg(unix)]
fn main() -> anyhow::Result<()> {
  use agent_desktop::{Desktop, DesktopConfiguration, DesktopModule};
  use clap::Parser;
  use gpui_kit::*;
  use std::{collections::BTreeMap, sync::Arc};

  #[derive(Parser)]
  struct Arguments {
    #[arg(long)]
    evidence_dir: std::path::PathBuf,
  }
  let args = Arguments::parse();
  std::fs::create_dir_all(&args.evidence_dir)?;
  let evidence_dir = args.evidence_dir.canonicalize()?;
  // GPUI retains the main-thread event loop; fixture daemons use this held runtime.
  let runtime = Arc::new(
    tokio::runtime::Builder::new_multi_thread()
      .worker_threads(4)
      .enable_all()
      .build()?,
  );
  let scenario = Arc::new(runtime.block_on(recovery_support::Scenario::open(120_000)));
  std::env::set_current_dir(&scenario.remote.pair.session.workspace)?;
  let directory = tempfile::tempdir()?;
  let settings = agent_config::load_with_environment(
    directory.path(),
    None,
    &BTreeMap::from([
      (
        "LYONBROWN_ENDPOINT".into(),
        scenario.remote.endpoint.clone(),
      ),
      (
        "LYONBROWN_TOKEN".into(),
        scenario.remote.pair.master.auth_token().into(),
      ),
    ]),
  )?;
  let _logs = agent_telemetry::init(directory.path(), "recovery-acceptance", "info")?;
  let module = DesktopModule::new(fluxdi::Shared::new(DesktopConfiguration {
    settings,
    data_dir: directory.path().into(),
    browser_executable: None,
  }));
  let configuration = module.configuration()?;
  let connector = module.client()?;
  let browser = module.browser()?;
  let computer = module.computer()?;
  let target = scenario.remote.target();
  std::fs::write(
    evidence_dir.join("fixture.json"),
    serde_json::to_vec_pretty(&serde_json::json!({
      "original_run":scenario.original.id, "task_id":scenario.task_id,
      "worker_id":target.id, "worker_endpoint":target.endpoint,
      "effect_file":scenario.effect(), "platform":std::env::consts::OS,
      "scope":"synthetic loopback models; isolated paired Master/Worker; no installed service or paid models"
    }))?,
  )?;
  gpui_kit::application().with_assets(gpui_kit::assets::Assets).run({
    let runtime = runtime.clone();
    let quit_runtime = runtime.clone();
    let quit_scenario = scenario.clone();
    move |cx| {
      agent_desktop::init(cx);
      // Cocoa terminates the process instead of returning from Application::run.
      // Capture while GPUI still awaits its supported asynchronous quit hooks.
      cx.on_app_quit(move |_| {
        let runtime = quit_runtime.clone();
        let scenario = quit_scenario.clone();
        let directory = evidence_dir.clone();
        async move {
          let writer = runtime.spawn(async move {
            let result = async {
              let mut report = scenario.evidence().await?;
              report["scope"] = serde_json::json!("visible native GPUI, authenticated daemon and paired mTLS Worker, loopback synthetic models; Unix command cancellation after file append; no CEF, real model, other-platform, installation or public-network acceptance");
              std::fs::write(directory.join("runtime.json"), serde_json::to_vec_pretty(&report)?)?;
              anyhow::Ok(())
            }.await;
            if let Err(error) = result {
              tracing::error!(event = "recovery_evidence_failed", error = %error, "Could not save acceptance evidence");
            }
            scenario.dispatch.abort();
            scenario.remote.pair.master.shutdown().await;
            scenario.remote.pair.worker.shutdown().await;
          });
          if let Err(error) = writer.await {
            tracing::error!(event = "recovery_evidence_writer_failed", error = %error, "Acceptance writer stopped");
          }
        }
      }).detach();
      cx.bind_keys([KeyBinding::new("cmd-q", QuitAcceptance, None)]);
      cx.on_action(|_: &QuitAcceptance, cx| cx.quit());
      cx.on_window_closed(|cx, _| if cx.windows().is_empty() { cx.quit(); }).detach();
      let result = gpui_kit::open_window(WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(None, size(px(1440.), px(960.)), cx))),
        ..gpui_kit::component::TitleBar::window_options()
      }, cx, move |window, cx| {
        window.set_window_title("Lyonbrown · recovery acceptance · local mock");
        window.activate_window();
        cx.new(|cx| Desktop::new(runtime, connector, browser, computer, configuration, window, cx))
      });
      if let Err(error) = result {
        tracing::error!(event = "recovery_acceptance_window_failed", error = %error, "Native window failed");
        cx.quit();
      }
      cx.activate(true);
    }
  });
  Ok(())
}
