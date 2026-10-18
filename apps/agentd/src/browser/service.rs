use super::bridge::SpaceBinding;
use super::routing::{failure, space_view};
use crate::Daemon;
use agent_browser::{Action, BrowserError, Command, Reply, wire};
use agent_protocol::v1::{
  self,
  browser_service_server::{BrowserService, BrowserServiceServer},
};
use agent_security::Authenticate;
use tonic::{Request, Response, Status, service::interceptor::InterceptedService};

pub fn browser_service(
  daemon: Daemon,
) -> InterceptedService<BrowserServiceServer<Daemon>, Authenticate> {
  let auth = Authenticate::new(daemon.auth_token());
  InterceptedService::new(
    BrowserServiceServer::new(daemon).max_encoding_message_size(wire::MAX_PACKET_BYTES),
    auth,
  )
}
#[tonic::async_trait]
impl BrowserService for Daemon {
  async fn open_space(
    &self,
    request: Request<v1::OpenSpaceRequest>,
  ) -> Result<Response<v1::BrowserSpace>, Status> {
    let input = request.into_inner();
    if input.profile_id.is_empty()
      || input.profile_id.len() > 96
      || !input
        .profile_id
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
      return Err(Status::invalid_argument("invalid profile identifier"));
    }
    if self.browser.routes.lock().await.spaces.len() >= 128 {
      return Err(Status::resource_exhausted("browser space limit reached"));
    }
    let host = self.browser.host(&input.host_id).await?;
    let reply = self
      .browser_mutate(
        &input.session_id,
        &host,
        &agent_core::new_id(),
        Command::OpenSpace {
          session_id: input.session_id.clone(),
          profile_id: input.profile_id.clone(),
          headless: input.headless,
        },
      )
      .await?;
    let Reply::Space(space) = reply else {
      return Err(Status::data_loss("runtime returned wrong response"));
    };
    if space.id.is_empty()
      || space.session_id != input.session_id
      || space.profile_id != input.profile_id
      || space.headless != input.headless
    {
      return Err(Status::data_loss("runtime space identity mismatch"));
    }
    let view = space_view(space.clone(), host.clone());
    let mut routes = self.browser.routes.lock().await;
    if routes.spaces.contains_key(&space.id) {
      return Err(Status::data_loss("runtime reused space identity"));
    }
    routes
      .spaces
      .insert(space.id.clone(), SpaceBinding { space, host });
    Ok(Response::new(view))
  }
  async fn list_spaces(
    &self,
    _: Request<v1::Empty>,
  ) -> Result<Response<v1::BrowserSpaces>, Status> {
    let spaces = self
      .browser
      .routes
      .lock()
      .await
      .spaces
      .values()
      .map(|binding| space_view(binding.space.clone(), binding.host.clone()))
      .collect();
    Ok(Response::new(v1::BrowserSpaces { spaces }))
  }
  async fn open_tab(
    &self,
    request: Request<v1::OpenTabRequest>,
  ) -> Result<Response<v1::BrowserTab>, Status> {
    let input = request.into_inner();
    agent_browser::validate_url(&input.url).map_err(failure)?;
    let (host, session, _) = self.browser_space_route(&input.space_id).await?;
    if self.browser.routes.lock().await.tabs.len() >= 512 {
      return Err(Status::resource_exhausted("browser tab limit reached"));
    }
    let reply = self
      .browser_mutate(
        &session,
        &host,
        &agent_core::new_id(),
        Command::OpenTab {
          space_id: input.space_id.clone(),
          url: input.url,
        },
      )
      .await?;
    let Reply::Tab(tab) = reply else {
      return Err(Status::data_loss("runtime returned wrong response"));
    };
    if tab.id.is_empty() || tab.space_id != input.space_id {
      return Err(Status::data_loss("runtime tab identity mismatch"));
    }
    let view = v1::BrowserTab {
      id: tab.id.clone(),
      space_id: tab.space_id.clone(),
      url: tab.url.clone(),
      title: tab.title.clone(),
    };
    let mut routes = self.browser.routes.lock().await;
    if routes.tabs.contains_key(&tab.id) {
      return Err(Status::data_loss("runtime reused tab identity"));
    }
    routes.tabs.insert(tab.id.clone(), tab);
    Ok(Response::new(view))
  }
  async fn observe(
    &self,
    request: Request<v1::ObserveRequest>,
  ) -> Result<Response<v1::BrowserObservation>, Status> {
    let input = request.into_inner();
    let (host, _, _) = self.browser_tab_route(&input.tab_id).await?;
    let reply = self
      .browser
      .request(
        &host,
        Command::Observe {
          tab_id: input.tab_id.clone(),
          screenshot: input.screenshot,
        },
      )
      .await
      .map_err(failure)?;
    let Reply::Snapshot(snapshot) = reply else {
      return Err(Status::data_loss("runtime returned wrong response"));
    };
    if input.screenshot && snapshot.frame.is_none() {
      return Err(Status::unavailable("browser screenshot frame unavailable"));
    }
    Ok(Response::new(
      self.browser_snapshot_view(snapshot, &input.tab_id).await?,
    ))
  }
  async fn act(
    &self,
    request: Request<v1::BrowserAction>,
  ) -> Result<Response<v1::BrowserActionResult>, Status> {
    let input = request.into_inner();
    let (host, session, human) = self.browser_tab_route(&input.tab_id).await?;
    if human {
      return Err(failure(BrowserError::HumanControl));
    }
    let action = Action {
      id: input.action_id.clone(),
      tab_id: input.tab_id.clone(),
      snapshot_id: input.snapshot_id,
      revision: input.revision,
      kind: input.kind,
      element_ref: input.element_ref,
      text: input.text,
      timeout_ms: input.timeout_ms,
    };
    agent_browser::validate_action(&action).map_err(failure)?;
    let reply = self
      .browser_mutate(&session, &host, &input.action_id, Command::Act(action))
      .await?;
    let Reply::Snapshot(snapshot) = reply else {
      return Err(Status::data_loss("runtime returned wrong response"));
    };
    let observation = self.browser_snapshot_view(snapshot, &input.tab_id).await?;
    Ok(Response::new(v1::BrowserActionResult {
      action_id: input.action_id,
      status: "completed".into(),
      error: String::new(),
      observation: Some(observation),
    }))
  }
  async fn takeover(
    &self,
    request: Request<v1::TakeoverRequest>,
  ) -> Result<Response<v1::BrowserSpace>, Status> {
    let input = request.into_inner();
    let (host, session, _) = self.browser_space_route(&input.space_id).await?;
    let reply = self
      .browser_mutate(
        &session,
        &host,
        &agent_core::new_id(),
        Command::Takeover {
          space_id: input.space_id.clone(),
          human_control: input.human_control,
        },
      )
      .await?;
    let Reply::Space(space) = reply else {
      return Err(Status::data_loss("runtime returned wrong response"));
    };
    if space.id != input.space_id
      || space.session_id != session
      || space.human_control != input.human_control
    {
      return Err(Status::data_loss("runtime takeover identity mismatch"));
    }
    let view = space_view(space.clone(), host.clone());
    self
      .browser
      .routes
      .lock()
      .await
      .spaces
      .insert(space.id.clone(), SpaceBinding { space, host });
    Ok(Response::new(view))
  }
  async fn close_space(
    &self,
    request: Request<v1::SpaceRequest>,
  ) -> Result<Response<v1::Empty>, Status> {
    let input = request.into_inner();
    let (host, session, _) = self.browser_space_route(&input.space_id).await?;
    let reply = self
      .browser_mutate(
        &session,
        &host,
        &agent_core::new_id(),
        Command::CloseSpace {
          space_id: input.space_id.clone(),
        },
      )
      .await?;
    if !matches!(reply, Reply::Done) {
      return Err(Status::data_loss("runtime returned wrong response"));
    }
    let mut routes = self.browser.routes.lock().await;
    routes.spaces.remove(&input.space_id);
    routes.tabs.retain(|_, tab| tab.space_id != input.space_id);
    Ok(Response::new(v1::Empty {}))
  }
  async fn get_frame(
    &self,
    request: Request<v1::FrameRequest>,
  ) -> Result<Response<v1::BrowserFrame>, Status> {
    self.browser_frame_response(request).await
  }
  async fn send_input(
    &self,
    request: Request<v1::BrowserInput>,
  ) -> Result<Response<v1::Empty>, Status> {
    let input = request.into_inner();
    let (host, session, human) = self.browser_tab_route(&input.tab_id).await?;
    // Resize is presentation state; user input requires explicit takeover.
    if !human && input.kind != "resize" {
      return Err(Status::failed_precondition(
        "enable human takeover before native input",
      ));
    }
    let event = super::input::convert(&input)?;
    let reply = self
      .browser_mutate(
        &session,
        &host,
        &agent_core::new_id(),
        Command::Input {
          tab_id: input.tab_id,
          revision: input.revision,
          event,
        },
      )
      .await?;
    if !matches!(reply, Reply::Done) {
      return Err(Status::data_loss("runtime returned wrong response"));
    }
    Ok(Response::new(v1::Empty {}))
  }
}
