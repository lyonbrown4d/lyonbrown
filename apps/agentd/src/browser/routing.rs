use crate::Daemon;
use agent_browser::{BrowserError, Command, Reply, Snapshot, Space};
use agent_protocol::v1;
use serde_json::json;
use tonic::{Request, Response, Status};
pub(super) fn failure(error: BrowserError) -> Status {
  match error {
    BrowserError::Unavailable(_) => Status::unavailable("browser runtime unavailable"),
    BrowserError::Uncertain | BrowserError::Timeout => {
      Status::unknown("browser action outcome is unknown; it was not replayed")
    }
    BrowserError::StaleSnapshot => Status::failed_precondition("snapshot stale; observe again"),
    BrowserError::HumanControl => Status::failed_precondition("human takeover is active"),
    BrowserError::NotFound(_) => Status::not_found("browser object not found"),
    BrowserError::InvalidInput(_) => Status::invalid_argument("invalid browser request"),
    BrowserError::Protocol(_) => Status::data_loss("browser runtime response invalid"),
  }
}
pub(super) fn space_view(space: Space, host: String) -> v1::BrowserSpace {
  v1::BrowserSpace {
    id: space.id,
    session_id: space.session_id,
    profile_id: space.profile_id,
    host_id: host,
    human_control: space.human_control,
  }
}
impl Daemon {
  pub(super) async fn browser_space_route(
    &self,
    id: &str,
  ) -> Result<(String, String, bool), Status> {
    self
      .browser
      .routes
      .lock()
      .await
      .spaces
      .get(id)
      .map(|binding| {
        (
          binding.host.clone(),
          binding.space.session_id.clone(),
          binding.space.human_control,
        )
      })
      .ok_or_else(|| Status::not_found("browser space not found"))
  }
  pub(super) async fn browser_tab_route(&self, id: &str) -> Result<(String, String, bool), Status> {
    let space = self
      .browser
      .routes
      .lock()
      .await
      .tabs
      .get(id)
      .map(|tab| tab.space_id.clone())
      .ok_or_else(|| Status::not_found("browser tab not found"))?;
    self.browser_space_route(&space).await
  }
  pub(super) async fn browser_mutate(
    &self,
    session_id: &str,
    host: &str,
    id: &str,
    command: Command,
  ) -> Result<Reply, Status> {
    let session = self
      .store
      .session(session_id)
      .map_err(|_| Status::internal("session storage failed"))?
      .ok_or_else(|| Status::not_found("session not found"))?;
    self
      .browser_mutate_owned(session_id, session.owner_epoch, host, id, command)
      .await
  }
  pub(super) async fn browser_mutate_owned(
    &self,
    session_id: &str,
    expected_epoch: u64,
    host: &str,
    id: &str,
    command: Command,
  ) -> Result<Reply, Status> {
    let session = self
      .store
      .session(session_id)
      .map_err(|_| Status::internal("session storage failed"))?
      .ok_or_else(|| Status::not_found("session not found"))?;
    if session.owner_node_id != self.node_id || session.owner_epoch != expected_epoch {
      return Err(Status::failed_precondition(
        "this node does not own the browser session",
      ));
    }
    self
      .store
      .begin_browser_action(
        session_id,
        session.owner_epoch,
        id,
        "browser_command",
        audit_summary(&command),
      )
      .map_err(|_| {
        Status::failed_precondition("browser action already recorded or session ownership changed")
      })?;
    let expected = command.clone();
    let mut guard = MutationGuard {
      store: self.store.clone(),
      id: id.into(),
      epoch: session.owner_epoch,
      finished: false,
    };
    let result = self.browser.request(host, command).await.and_then(|reply| {
      if super::validation::matches_reply(&expected, &reply, session_id) {
        Ok(reply)
      } else {
        Err(BrowserError::Uncertain)
      }
    });
    let status = match &result {
      Ok(_) => "completed",
      Err(BrowserError::Uncertain | BrowserError::Timeout) => "uncertain",
      Err(_) => "failed",
    };
    self
      .store
      .finish_browser_action(id, session.owner_epoch, status, json!({"status":status}))
      .map_err(|_| Status::unknown("browser outcome could not be persisted; do not replay"))?;
    guard.finished = true;
    result.map_err(failure)
  }
  pub(super) async fn browser_snapshot_view(
    &self,
    snapshot: Snapshot,
    expected: &str,
  ) -> Result<v1::BrowserObservation, Status> {
    if snapshot.tab_id != expected
      || snapshot.id.is_empty()
      || snapshot.accessibility_json.len() > agent_browser::MAX_AX_BYTES
    {
      return Err(Status::data_loss("invalid browser snapshot"));
    }
    let artifact = if let Some(frame) = snapshot.frame {
      frame.validate().map_err(failure)?;
      if frame.tab_id != expected || frame.revision != snapshot.revision {
        return Err(Status::data_loss("snapshot frame identity mismatch"));
      }
      let path = self
        .config
        .data_path()
        .map_err(|_| Status::internal("artifact path unavailable"))?
        .join("artifacts");
      let node = self.node_id.clone();
      tokio::task::spawn_blocking(move || -> anyhow::Result<String> {
        use image::ImageEncoder;
        let mut rgba = frame.bgra;
        for pixel in rgba.as_chunks_mut::<4>().0 {
          pixel.swap(0, 2);
          let alpha = u32::from(pixel[3]);
          if alpha > 0 && alpha < 255 {
            for color in &mut pixel[..3] {
              *color = ((u32::from(*color) * 255 + alpha / 2) / alpha).min(255) as u8;
            }
          }
        }
        let mut png = Vec::new();
        image::codecs::png::PngEncoder::new(&mut png).write_image(
          &rgba,
          frame.width,
          frame.height,
          image::ExtendedColorType::Rgba8,
        )?;
        Ok(
          agent_workspace::ArtifactStore::open(&path, &node)?
            .put(&png, "image/png")?
            .digest,
        )
      })
      .await
      .map_err(|_| Status::internal("screenshot encoding task failed"))?
      .map_err(|_| Status::internal("screenshot artifact persistence failed"))?
    } else {
      String::new()
    };
    Ok(v1::BrowserObservation {
      snapshot_id: snapshot.id,
      tab_id: snapshot.tab_id,
      revision: snapshot.revision,
      accessibility_json: snapshot.accessibility_json,
      screenshot_artifact: artifact,
    })
  }
}

/// Human keystrokes, fill values and navigation URLs can contain credentials.
/// Receipts need identity and outcome, never a reconstructable input stream.
fn audit_summary(command: &Command) -> serde_json::Value {
  match command {
    Command::Input {
      tab_id, revision, ..
    } => json!({"kind":"input","tab_id":tab_id,"revision":revision}),
    Command::Act(action) => {
      json!({"kind":"act","action_id":action.id,"tab_id":action.tab_id,"snapshot_id":action.snapshot_id,"revision":action.revision,"operation":action.kind})
    }
    Command::OpenTab { space_id, .. } => json!({"kind":"open_tab","space_id":space_id}),
    Command::OpenSpace {
      session_id,
      profile_id,
      headless,
    } => {
      json!({"kind":"open_space","session_id":session_id,"profile_id":profile_id,"headless":headless})
    }
    Command::Takeover {
      space_id,
      human_control,
    } => json!({"kind":"takeover","space_id":space_id,"human_control":human_control}),
    Command::CloseSpace { space_id } => json!({"kind":"close_space","space_id":space_id}),
    _ => json!({"kind":"runtime_command"}),
  }
}

struct MutationGuard {
  store: agent_storage::Store,
  id: String,
  epoch: u64,
  finished: bool,
}
impl Drop for MutationGuard {
  fn drop(&mut self) {
    if !self.finished
      && self
        .store
        .finish_browser_action(
          &self.id,
          self.epoch,
          "uncertain",
          json!({"reason":"caller_disconnected"}),
        )
        .is_err()
    {
      tracing::warn!(action_id=%self.id,"browser intent remains unresolved after caller disconnected");
    }
  }
}

impl Daemon {
  pub(super) async fn browser_frame_response(
    &self,
    request: Request<v1::FrameRequest>,
  ) -> Result<Response<v1::BrowserFrame>, Status> {
    let input = request.into_inner();
    let (host, _, _) = self.browser_tab_route(&input.tab_id).await?;
    let reply = self
      .browser
      .request(
        &host,
        Command::GetFrame {
          tab_id: input.tab_id.clone(),
          after_sequence: input.after_sequence,
        },
      )
      .await
      .map_err(failure)?;
    let Reply::Frame(frame) = reply else {
      return Err(Status::data_loss("runtime returned wrong response"));
    };
    let frame = frame.ok_or_else(|| Status::unavailable("no newer browser frame"))?;
    frame.validate().map_err(failure)?;
    if frame.tab_id != input.tab_id || frame.sequence <= input.after_sequence {
      return Err(Status::data_loss(
        "runtime frame identity or sequence mismatch",
      ));
    }
    Ok(Response::new(v1::BrowserFrame {
      tab_id: frame.tab_id,
      revision: frame.revision,
      sequence: frame.sequence,
      width: frame.width,
      height: frame.height,
      stride: frame.stride,
      bgra8: frame.bgra,
    }))
  }
}
