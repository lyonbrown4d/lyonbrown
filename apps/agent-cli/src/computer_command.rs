use agent_computer_runtime::{ComputerHostFactory, NativeComputerHost};

use crate::{
  computer::{self, ComputerCommand, ComputerError},
  config::ResolvedConfig,
};

pub(crate) async fn execute(
  config: &ResolvedConfig,
  command: ComputerCommand,
) -> anyhow::Result<()> {
  let factory = NativeComputerHost;
  match command {
    ComputerCommand::Targets => {
      let targets = computer::targets(&factory).await?;
      println!("{}", serde_json::to_string_pretty(&targets)?);
    }
    ComputerCommand::Attach(args) => {
      let target = args.resolve_target(&factory).await?;
      let client = crate::bootstrap::connect_client(config).await?;
      let mut attachment = factory
        .attach(client, &args.session, target)
        .await
        .map_err(|error| {
          ComputerError::Native(agent_computer_runtime::computer_error_message(&error))
        })?;
      println!(
        "ComputerRuntime attached to session {} · {}. Keep this process open; Ctrl+C detaches.",
        args.session,
        computer::scope_text(&args.scope()?)
      );
      tokio::select! {
        _ = crate::terminal::shutdown() => attachment.shutdown().await.map_err(|_| ComputerError::Shutdown)?,
        result = attachment.wait_closed() => {
          result.map_err(|_| ComputerError::Disconnected)?;
        }
      }
    }
  }
  Ok(())
}
