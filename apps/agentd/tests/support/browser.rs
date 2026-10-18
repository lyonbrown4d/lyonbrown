use agent_browser::{
  BrowserResult, Command, Frame, RUNTIME_PROTOCOL_VERSION, Reply, Snapshot, Space, Tab, wire,
};
use agent_client::AgentClient;
use agent_protocol::v1::BrowserRuntimeResponse;
use std::sync::{
  Arc,
  atomic::{AtomicUsize, Ordering},
};
use tokio_stream::wrappers::ReceiverStream;
pub struct MockRuntime {
  pub host_id: String,
  pub calls: Arc<AtomicUsize>,
  task: tokio::task::JoinHandle<()>,
}
impl MockRuntime {
  pub async fn attach(client: &AgentClient, drop_action: bool, large_frame: bool) -> Self {
    let host_id = agent_core::new_id();
    let (sender, receiver) = tokio::sync::mpsc::channel(2);
    sender
      .send(BrowserRuntimeResponse {
        host_id: host_id.clone(),
        protocol_version: RUNTIME_PROTOCOL_VERSION,
        request_id: String::new(),
        payload: Vec::new(),
      })
      .await
      .unwrap();
    let mut requests = client
      .attach_browser_runtime(ReceiverStream::new(receiver))
      .await
      .unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    let host = host_id.clone();
    let task = tokio::spawn(async move {
      let mut space = None;
      let mut tab = None;
      let mut revision = 1;
      while let Ok(Some(request)) = requests.message().await {
        let command: Command = wire::decode(&request.payload).unwrap();
        let result: BrowserResult<Reply> = Ok(match command {
          Command::OpenSpace {
            session_id,
            profile_id,
            headless,
          } => {
            let value = Space {
              id: agent_core::new_id(),
              session_id,
              profile_id,
              headless,
              human_control: false,
            };
            space = Some(value.clone());
            Reply::Space(value)
          }
          Command::OpenTab { space_id, url } => {
            let value = Tab {
              id: agent_core::new_id(),
              space_id,
              url,
              title: "fixture".into(),
            };
            tab = Some(value.clone());
            Reply::Tab(value)
          }
          Command::Takeover { human_control, .. } => {
            let value = space.as_mut().unwrap();
            value.human_control = human_control;
            Reply::Space(value.clone())
          }
          Command::GetFrame {
            tab_id,
            after_sequence,
          } => Reply::Frame(if after_sequence >= 1 {
            None
          } else {
            Some(frame(tab_id, revision, large_frame))
          }),
          Command::Observe { tab_id, screenshot } => {
            Reply::Snapshot(snapshot(tab_id, revision, screenshot))
          }
          Command::Act(action) => {
            count.fetch_add(1, Ordering::SeqCst);
            if drop_action {
              break;
            }
            revision += 1;
            Reply::Snapshot(snapshot(action.tab_id, revision, false))
          }
          Command::CloseSpace { .. } => {
            space = None;
            tab = None;
            Reply::Done
          }
          Command::Input { .. } => {
            count.fetch_add(1, Ordering::SeqCst);
            Reply::Done
          }
          _ => panic!("unexpected fixture command"),
        });
        let _ = &tab;
        sender
          .send(BrowserRuntimeResponse {
            host_id: host.clone(),
            protocol_version: RUNTIME_PROTOCOL_VERSION,
            request_id: request.request_id,
            payload: wire::encode(&result).unwrap(),
          })
          .await
          .unwrap();
      }
    });
    Self {
      host_id,
      calls,
      task,
    }
  }
}
impl Drop for MockRuntime {
  fn drop(&mut self) {
    self.task.abort();
  }
}
fn frame(tab_id: String, revision: u64, large: bool) -> Frame {
  let (width, height) = if large { (1024, 1025) } else { (1, 1) };
  Frame {
    tab_id,
    revision,
    sequence: 1,
    width,
    height,
    stride: width * 4,
    bgra: vec![255; (width * height * 4) as usize],
  }
}
fn snapshot(tab_id: String, revision: u64, screenshot: bool) -> Snapshot {
  Snapshot {
    id: format!("snapshot-{revision}"),
    revision,
    accessibility_json: "{\"role\":\"document\"}".into(),
    frame: screenshot.then(|| frame(tab_id.clone(), revision, false)),
    tab_id,
  }
}
