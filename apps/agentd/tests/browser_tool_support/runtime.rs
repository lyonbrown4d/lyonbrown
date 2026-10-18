use agent_browser::{Action, BrowserResult, Command, Reply, Snapshot, Space, Tab, wire};
use agent_client::AgentClient;
use agent_protocol::v1::BrowserRuntimeResponse;
use std::{collections::HashMap, sync::Arc};
use tokio::sync::{Mutex, Notify, mpsc};
use tokio_stream::wrappers::ReceiverStream;
#[derive(Clone, Copy)]
pub enum ActionMode {
  Complete,
  Disconnect,
  Hold,
}
pub struct Runtime {
  pub host: String,
  pub actions: Arc<Mutex<Vec<Action>>>,
  pub observed: Arc<Mutex<Vec<String>>>,
  changed: Arc<Notify>,
  task: tokio::task::JoinHandle<()>,
}
impl Runtime {
  pub async fn attach(client: &AgentClient, mode: ActionMode) -> Self {
    let host = agent_core::new_id();
    let (sender, receiver) = mpsc::channel(2);
    sender
      .send(BrowserRuntimeResponse {
        host_id: host.clone(),
        protocol_version: agent_browser::RUNTIME_PROTOCOL_VERSION,
        request_id: String::new(),
        payload: vec![],
      })
      .await
      .unwrap();
    let mut requests = client
      .attach_browser_runtime(ReceiverStream::new(receiver))
      .await
      .unwrap();
    let actions = Arc::new(Mutex::new(Vec::new()));
    let observed = Arc::new(Mutex::new(Vec::new()));
    let changed = Arc::new(Notify::new());
    let recorded = actions.clone();
    let observations = observed.clone();
    let notify = changed.clone();
    let runtime_host = host.clone();
    let task = tokio::spawn(async move {
      let mut spaces: HashMap<String, Space> = HashMap::new();
      let mut tabs = 0;
      while let Ok(Some(request)) = requests.message().await {
        let command: Command = wire::decode(&request.payload).unwrap();
        let response: BrowserResult<Reply> = Ok(match command {
          Command::OpenSpace {
            session_id,
            profile_id,
            headless,
          } => {
            let space = Space {
              id: format!("fixture-space-{}", spaces.len() + 1),
              session_id,
              profile_id,
              headless,
              human_control: false,
            };
            spaces.insert(space.id.clone(), space.clone());
            Reply::Space(space)
          }
          Command::OpenTab { space_id, url } => {
            tabs += 1;
            Reply::Tab(Tab {
              id: format!("fixture-tab-{tabs}"),
              space_id,
              url,
              title: "local fixture".into(),
            })
          }
          Command::Observe { tab_id, screenshot } => {
            observations.lock().await.push(tab_id.clone());
            Reply::Snapshot(snapshot(tab_id, 1, screenshot))
          }
          Command::Act(action) => {
            let tab = action.tab_id.clone();
            recorded.lock().await.push(action);
            notify.notify_waiters();
            match mode {
              ActionMode::Disconnect => break,
              ActionMode::Hold => {
                std::future::pending::<()>().await;
                unreachable!()
              }
              ActionMode::Complete => Reply::Snapshot(snapshot(tab, 2, false)),
            }
          }
          Command::Takeover {
            space_id,
            human_control,
          } => {
            let space = spaces.get_mut(&space_id).unwrap();
            space.human_control = human_control;
            Reply::Space(space.clone())
          }
          Command::CloseSpace { space_id } => {
            spaces.remove(&space_id);
            Reply::Done
          }
          _ => panic!("unexpected browser model fixture command"),
        });
        sender
          .send(BrowserRuntimeResponse {
            host_id: runtime_host.clone(),
            protocol_version: agent_browser::RUNTIME_PROTOCOL_VERSION,
            request_id: request.request_id,
            payload: wire::encode(&response).unwrap(),
          })
          .await
          .unwrap();
      }
    });
    Self {
      host,
      actions,
      observed,
      changed,
      task,
    }
  }
  pub async fn wait_action(&self) {
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
      loop {
        let notified = self.changed.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();
        if !self.actions.lock().await.is_empty() {
          break;
        }
        notified.await;
      }
    })
    .await
    .unwrap();
  }
}
impl Drop for Runtime {
  fn drop(&mut self) {
    self.task.abort();
  }
}
fn snapshot(tab_id: String, revision: u64, screenshot: bool) -> Snapshot {
  let frame = screenshot.then(|| agent_browser::Frame {
    tab_id: tab_id.clone(),
    revision,
    sequence: 1,
    width: 1,
    height: 1,
    stride: 4,
    bgra: vec![255; 4],
  });
  Snapshot { id: format!("fixture-snapshot-{revision}"), tab_id, revision,
    accessibility_json: r#"{"role":"document","children":[{"ref":"submit","role":"button","name":"Submit local fixture"}]}"#.into(), frame }
}
