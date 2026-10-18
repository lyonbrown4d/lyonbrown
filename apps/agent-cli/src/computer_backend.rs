use std::sync::Arc;

use agent_client::AgentClient;
use agent_computer_runtime::{ComputerHostFactory, DesktopScope, DesktopTarget, RuntimeAttachment};

use crate::{
  backend::Update,
  computer::{self, ComputerError, ComputerStatus, LocalCommand},
};

/// One physical desktop attachment; selection never transfers its authority.
pub(crate) struct ComputerOwner {
  factory: Arc<dyn ComputerHostFactory>,
  attachment: Option<RuntimeAttachment>,
  catalog: Vec<DesktopTarget>,
  status: ComputerStatus,
  detail: String,
}
impl ComputerOwner {
  pub fn new(factory: Arc<dyn ComputerHostFactory>) -> Self {
    let status = if factory.available() {
      ComputerStatus::Detached
    } else {
      ComputerStatus::Unavailable
    };
    Self {
      factory,
      attachment: None,
      catalog: Vec::new(),
      status,
      detail: String::new(),
    }
  }
  pub fn snapshot(&self) -> Update {
    Update::Computer {
      status: self.status.clone(),
      detail: self.detail.clone(),
    }
  }
  pub fn pending(&self, session: Option<&str>, command: &LocalCommand) -> Option<Update> {
    if self.attachment.is_none()
      && self.factory.available()
      && let (Some(session), LocalCommand::Attach(scope)) = (session, command)
    {
      Some(Update::Computer {
        status: ComputerStatus::Attaching {
          session: session.into(),
          scope: scope.clone(),
        },
        detail: String::new(),
      })
    } else {
      None
    }
  }
  pub async fn execute(
    &mut self,
    client: Option<&AgentClient>,
    session: Option<&str>,
    command: LocalCommand,
  ) {
    self.detail.clear();
    let result = match command {
      LocalCommand::Targets => match computer::targets(self.factory.as_ref()).await {
        Ok(targets) => {
          self.detail = if targets.is_empty() {
            "No local desktop targets".into()
          } else {
            targets
              .iter()
              .map(computer::target_text)
              .collect::<Vec<_>>()
              .join("\n")
          };
          self.catalog = targets;
          Ok(())
        }
        Err(error) => {
          self.catalog.clear();
          Err(error)
        }
      },
      LocalCommand::Status => Ok(()),
      LocalCommand::Detach => {
        if let Some(mut attachment) = self.attachment.take() {
          let session = self.bound_session();
          match attachment.shutdown().await {
            Ok(()) => {
              self.status = ComputerStatus::Detached;
              Ok(())
            }
            Err(_) => {
              self.status = ComputerStatus::Stopped {
                session,
                reason: ComputerError::Shutdown,
              };
              Err(ComputerError::Shutdown)
            }
          }
        } else {
          Ok(())
        }
      }
      LocalCommand::Attach(scope) => self.attach(client, session, scope).await,
    };
    if let Err(error) = result {
      self.detail = error.to_string();
    }
  }
  pub async fn attach(
    &mut self,
    client: Option<&AgentClient>,
    session: Option<&str>,
    scope: DesktopScope,
  ) -> Result<(), ComputerError> {
    if self.attachment.is_some() {
      return Err(ComputerError::AlreadyAttached);
    }
    let (Some(client), Some(session)) = (client, session) else {
      return Err(ComputerError::SessionRequired);
    };
    if !self.factory.available() {
      return Err(ComputerError::Unavailable);
    }
    if self.catalog.is_empty() {
      return Err(ComputerError::CatalogRequired);
    }
    let target = self
      .catalog
      .iter()
      .find(|target| target.scope == scope)
      .cloned()
      .ok_or(ComputerError::TargetMissing)?;
    match self.factory.attach(client.clone(), session, target).await {
      Ok(attachment) => {
        self.attachment = Some(attachment);
        self.status = ComputerStatus::Attached {
          session: session.into(),
          scope,
        };
        Ok(())
      }
      Err(error) => {
        let reason = ComputerError::Native(agent_computer_runtime::computer_error_message(&error));
        self.status = ComputerStatus::Stopped {
          session: Some(session.into()),
          reason,
        };
        Err(reason)
      }
    }
  }
  pub fn bound_session(&self) -> Option<String> {
    match &self.status {
      ComputerStatus::Attaching { session, .. } | ComputerStatus::Attached { session, .. } => {
        Some(session.clone())
      }
      ComputerStatus::Stopped { session, .. } => session.clone(),
      _ => None,
    }
  }
  pub fn disconnected(&mut self) {
    self.catalog.clear();
    self.detail.clear();
    if self.attachment.take().is_some() {
      self.status = ComputerStatus::Stopped {
        session: self.bound_session(),
        reason: ComputerError::Disconnected,
      };
      self.detail.clear();
    }
  }
  pub fn poll(&mut self) -> bool {
    if self
      .attachment
      .as_ref()
      .is_some_and(RuntimeAttachment::is_finished)
    {
      self.disconnected();
      true
    } else {
      false
    }
  }
}
