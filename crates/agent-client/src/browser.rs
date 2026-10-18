use crate::AgentClient;
use agent_protocol::v1;
use anyhow::Result;
impl AgentClient {
  pub async fn attach_browser_runtime(
    &self,
    response: impl futures::Stream<Item = v1::BrowserRuntimeResponse> + Send + 'static,
  ) -> Result<tonic::Streaming<v1::BrowserRuntimeRequest>> {
    Ok(
      self
        .browser_runtime
        .clone()
        .attach(response)
        .await?
        .into_inner(),
    )
  }
  pub async fn browser_spaces(&self) -> Result<Vec<v1::BrowserSpace>> {
    Ok(
      self
        .browser
        .clone()
        .list_spaces(v1::Empty {})
        .await?
        .into_inner()
        .spaces,
    )
  }
  pub async fn open_browser_space(
    &self,
    session_id: &str,
    profile_id: &str,
    headless: bool,
  ) -> Result<v1::BrowserSpace> {
    self
      .open_browser_space_on_host(session_id, profile_id, headless, "")
      .await
  }
  pub async fn open_browser_space_on_host(
    &self,
    session_id: &str,
    profile_id: &str,
    headless: bool,
    host_id: &str,
  ) -> Result<v1::BrowserSpace> {
    Ok(
      self
        .browser
        .clone()
        .open_space(v1::OpenSpaceRequest {
          session_id: session_id.into(),
          profile_id: profile_id.into(),
          headless,
          host_id: host_id.into(),
        })
        .await?
        .into_inner(),
    )
  }
  pub async fn open_browser_tab(&self, space_id: &str, url: &str) -> Result<v1::BrowserTab> {
    Ok(
      self
        .browser
        .clone()
        .open_tab(v1::OpenTabRequest {
          space_id: space_id.into(),
          url: url.into(),
        })
        .await?
        .into_inner(),
    )
  }
  pub async fn observe_browser(
    &self,
    tab_id: &str,
    screenshot: bool,
  ) -> Result<v1::BrowserObservation> {
    Ok(
      self
        .browser
        .clone()
        .observe(v1::ObserveRequest {
          tab_id: tab_id.into(),
          screenshot,
        })
        .await?
        .into_inner(),
    )
  }
  pub async fn browser_action(&self, action: v1::BrowserAction) -> Result<v1::BrowserActionResult> {
    Ok(self.browser.clone().act(action).await?.into_inner())
  }
  pub async fn browser_frame(&self, tab_id: &str, after_sequence: u64) -> Result<v1::BrowserFrame> {
    Ok(
      self
        .browser
        .clone()
        .get_frame(v1::FrameRequest {
          tab_id: tab_id.into(),
          after_sequence,
        })
        .await?
        .into_inner(),
    )
  }
  pub async fn browser_input(&self, input: v1::BrowserInput) -> Result<()> {
    self.browser.clone().send_input(input).await?;
    Ok(())
  }
  pub async fn browser_takeover(
    &self,
    space_id: &str,
    human_control: bool,
  ) -> Result<v1::BrowserSpace> {
    Ok(
      self
        .browser
        .clone()
        .takeover(v1::TakeoverRequest {
          space_id: space_id.into(),
          human_control,
        })
        .await?
        .into_inner(),
    )
  }
  pub async fn close_browser_space(&self, space_id: &str) -> Result<()> {
    self
      .browser
      .clone()
      .close_space(v1::SpaceRequest {
        space_id: space_id.into(),
      })
      .await?;
    Ok(())
  }
}
