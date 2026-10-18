//! Client composition port; target discovery never grants daemon authority.
use crate::{DesktopTarget, RuntimeAttachment};
use agent_client::AgentClient;

/// Implementations retain SDK objects behind the client boundary. The UI owns
/// only target metadata and the exact session/target selected by its user.
#[async_trait::async_trait]
pub trait ComputerHostFactory: Send + Sync {
  /// Build availability only. Permission and session checks happen on use.
  fn available(&self) -> bool;
  async fn targets(&self) -> anyhow::Result<Vec<DesktopTarget>>;
  async fn attach(
    &self,
    client: AgentClient,
    session: &str,
    target: DesktopTarget,
  ) -> anyhow::Result<RuntimeAttachment>;
}

pub struct NativeComputerHost;

/// Only owned categories cross into UI status. Upstream errors may contain
/// window titles or private desktop content and must never be formatted here.
pub fn computer_error_message(error: &anyhow::Error) -> &'static str {
  use agent_computer::DesktopError;
  match error.downcast_ref::<DesktopError>() {
    Some(DesktopError::SessionStateUnknown) => {
      "ComputerRuntime unavailable: desktop lock state cannot be confirmed safely (SessionStateUnknown)"
    }
    Some(DesktopError::Locked) => "ComputerRuntime unavailable: the desktop session is locked",
    Some(DesktopError::SessionUnavailable) => {
      "ComputerRuntime unavailable: no active interactive desktop session"
    }
    Some(DesktopError::PermissionDenied(_)) => {
      "ComputerRuntime unavailable: required screen or input permission is not granted"
    }
    Some(DesktopError::StaleSnapshot) => {
      "Selected desktop target changed; refresh targets and select again"
    }
    _ => "ComputerRuntime unavailable; check the connection, session and desktop permissions",
  }
}

#[async_trait::async_trait]
impl ComputerHostFactory for NativeComputerHost {
  fn available(&self) -> bool {
    cfg!(feature = "native")
  }

  async fn targets(&self) -> anyhow::Result<Vec<DesktopTarget>> {
    #[cfg(feature = "native")]
    {
      tokio::time::timeout(
        std::time::Duration::from_secs(5),
        tokio::task::spawn_blocking(agent_computer::native::list_targets),
      )
      .await
      .map_err(|_| anyhow::anyhow!("desktop target discovery timed out"))?
      .map_err(|_| anyhow::anyhow!("desktop target discovery worker unavailable"))?
      .map_err(Into::into)
    }
    #[cfg(not(feature = "native"))]
    anyhow::bail!("native ComputerRuntime unavailable; build the client with native-computer")
  }

  async fn attach(
    &self,
    client: AgentClient,
    session: &str,
    target: DesktopTarget,
  ) -> anyhow::Result<RuntimeAttachment> {
    #[cfg(feature = "native")]
    {
      crate::attach_native(client, session, target).await
    }
    #[cfg(not(feature = "native"))]
    {
      let _ = (client, session, target);
      anyhow::bail!("native ComputerRuntime unavailable; build the client with native-computer")
    }
  }
}
