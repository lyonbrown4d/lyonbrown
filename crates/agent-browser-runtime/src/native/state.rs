use super::{
  context::SpaceRecord,
  handlers::{DevToolsCallbacks, RuntimeClient, TabView},
  tools,
};
use agent_browser::{Action, BrowserError, BrowserResult, Command, Reply, Tab, ViewInput};
use cef::*;
use std::{
  cell::RefCell,
  collections::{BTreeMap, HashMap},
  path::PathBuf,
  rc::Rc,
  time::{Duration, Instant},
};

pub(super) struct TabRecord {
  pub tab: Tab,
  pub browser: Browser,
  pub view: Rc<RefCell<TabView>>,
  pub snapshot: Option<SnapshotCache>,
  pub _observer: Registration,
}

pub(super) struct SnapshotCache {
  pub id: String,
  pub revision: u64,
  pub elements: BTreeMap<String, i64>,
  pub accessibility: serde_json::Value,
}

pub(super) struct Engine {
  profiles: PathBuf,
  spaces: HashMap<String, SpaceRecord>,
  tabs: HashMap<String, TabRecord>,
}

impl Engine {
  pub fn new(profiles: PathBuf) -> Self {
    Self {
      profiles,
      spaces: HashMap::new(),
      tabs: HashMap::new(),
    }
  }

  pub fn execute(&mut self, command: Command) -> BrowserResult<Reply> {
    match command {
      Command::Hello => Ok(Reply::Ready {
        engine: "cef-154-cpu-osr".into(),
        protocol_version: agent_browser::RUNTIME_PROTOCOL_VERSION,
      }),
      Command::OpenSpace {
        session_id,
        profile_id,
        headless,
      } => self.open_space(session_id, profile_id, headless),
      Command::ListSpaces => Ok(Reply::Spaces(
        self
          .spaces
          .values()
          .map(|record| record.space.clone())
          .collect(),
      )),
      Command::OpenTab { space_id, url } => self.open_tab(&space_id, &url),
      Command::Observe { tab_id, screenshot } => {
        tools::observe(self.tab(&tab_id)?, screenshot).map(Reply::Snapshot)
      }
      Command::Act(action) => self.act(action),
      Command::Takeover {
        space_id,
        human_control,
      } => {
        let record = self
          .spaces
          .get_mut(&space_id)
          .ok_or_else(|| BrowserError::NotFound("space".into()))?;
        record.space.human_control = human_control;
        for tab in self
          .tabs
          .values_mut()
          .filter(|tab| tab.tab.space_id == space_id)
        {
          tab.snapshot = None;
          let mut view = tab.view.borrow_mut();
          view.revision = view.revision.saturating_add(1);
          view.sequence = view.sequence.saturating_add(1);
          let revision = view.revision;
          let sequence = view.sequence;
          if let Some(frame) = &mut view.frame {
            frame.revision = revision;
            frame.sequence = sequence;
          }
        }
        Ok(Reply::Space(record.space.clone()))
      }
      Command::CloseSpace { space_id } => self.close_space(&space_id),
      Command::GetFrame {
        tab_id,
        after_sequence,
      } => {
        let tab = self.tab(&tab_id)?;
        tools::request_paint_if_stale(tab);
        let view = tab.view.borrow();
        Ok(Reply::Frame(view.frame.clone().filter(|frame| {
          frame.sequence > after_sequence && frame.revision == view.revision
        })))
      }
      Command::Input {
        tab_id,
        revision,
        event,
      } => self.input(&tab_id, revision, event),
      Command::Shutdown => {
        self.close_all()?;
        Ok(Reply::Done)
      }
    }
  }

  fn open_space(
    &mut self,
    session_id: String,
    profile_id: String,
    headless: bool,
  ) -> BrowserResult<Reply> {
    let record = SpaceRecord::new(&self.profiles, session_id, profile_id, headless)?;
    let space = record.space.clone();
    self.spaces.insert(space.id.clone(), record);
    self
      .spaces
      .get(&space.id)
      .ok_or_else(|| BrowserError::NotFound("space".into()))?
      .wait_ready()?;
    Ok(Reply::Space(space))
  }

  fn open_tab(&mut self, space_id: &str, url: &str) -> BrowserResult<Reply> {
    agent_browser::validate_url(url)?;
    let space = self
      .spaces
      .get_mut(space_id)
      .ok_or_else(|| BrowserError::NotFound("space".into()))?;
    let id = uuid::Uuid::now_v7().to_string();
    let view = TabView::new(id.clone());
    let info = WindowInfo {
      windowless_rendering_enabled: 1,
      shared_texture_enabled: 0,
      runtime_style: RuntimeStyle::ALLOY,
      ..Default::default()
    };
    let settings = BrowserSettings {
      windowless_frame_rate: 30,
      background_color: 0xffffffff,
      ..Default::default()
    };
    let mut client = RuntimeClient::new(view.clone());
    let browser = browser_host_create_browser_sync(
      Some(&info),
      Some(&mut client),
      Some(&"about:blank".into()),
      Some(&settings),
      None,
      Some(&mut space.context),
    )
    .ok_or_else(|| BrowserError::Unavailable("CEF browser creation failed".into()))?;
    let host = browser
      .host()
      .ok_or_else(|| BrowserError::Unavailable("CEF browser host missing".into()))?;
    host.was_hidden(0);
    host.was_resized();
    let mut observer = DevToolsCallbacks::new(view.clone());
    let registration = host
      .add_dev_tools_message_observer(Some(&mut observer))
      .ok_or_else(|| {
        BrowserError::Unavailable("CEF DevTools observer registration failed".into())
      })?;
    browser
      .main_frame()
      .ok_or_else(|| BrowserError::Unavailable("CEF main frame missing".into()))?
      .load_url(Some(&url.into()));
    let tab = Tab {
      id: id.clone(),
      space_id: space_id.into(),
      url: url.into(),
      title: String::new(),
    };
    self.tabs.insert(
      id,
      TabRecord {
        tab: tab.clone(),
        browser,
        view,
        snapshot: None,
        _observer: registration,
      },
    );
    Ok(Reply::Tab(tab))
  }

  fn tab(&mut self, id: &str) -> BrowserResult<&mut TabRecord> {
    self
      .tabs
      .get_mut(id)
      .ok_or_else(|| BrowserError::NotFound("tab".into()))
  }

  fn act(&mut self, action: Action) -> BrowserResult<Reply> {
    agent_browser::validate_action(&action)?;
    let space = self
      .tabs
      .get(&action.tab_id)
      .and_then(|tab| self.spaces.get(&tab.tab.space_id))
      .ok_or_else(|| BrowserError::NotFound("tab space".into()))?;
    if space.space.human_control {
      return Err(BrowserError::HumanControl);
    }
    let tab = self.tab(&action.tab_id)?;
    tools::act(tab, action)?;
    tools::observe(tab, false)
      .map(Reply::Snapshot)
      .map_err(|_| {
        tools::invalidate_after_effect(tab);
        BrowserError::Uncertain
      })
  }

  fn input(&mut self, id: &str, revision: u64, event: ViewInput) -> BrowserResult<Reply> {
    let space = self
      .tabs
      .get(id)
      .and_then(|tab| self.spaces.get(&tab.tab.space_id))
      .ok_or_else(|| BrowserError::NotFound("tab space".into()))?;
    if !space.space.human_control && !matches!(&event, ViewInput::Resize { .. }) {
      return Err(BrowserError::InvalidInput(
        "take human control before sending view input".into(),
      ));
    }
    let tab = self.tab(id)?;
    if tab.view.borrow().revision != revision {
      return Err(BrowserError::StaleSnapshot);
    }
    tools::input(tab, event)?;
    Ok(Reply::Done)
  }

  fn close_space(&mut self, id: &str) -> BrowserResult<Reply> {
    if !self.spaces.contains_key(id) {
      return Err(BrowserError::NotFound("space".into()));
    }
    for tab in self.tabs.values().filter(|tab| tab.tab.space_id == id) {
      if let Some(host) = tab.browser.host() {
        host.close_browser(1);
      }
    }
    let deadline = Instant::now() + Duration::from_secs(3);
    while self
      .tabs
      .values()
      .any(|tab| tab.tab.space_id == id && !tab.view.borrow().closed)
    {
      if Instant::now() >= deadline {
        return Err(BrowserError::Uncertain);
      }
      crate::native::pump();
      std::thread::sleep(Duration::from_millis(8));
    }
    self.tabs.retain(|_, tab| tab.tab.space_id != id);
    self.spaces.remove(id);
    Ok(Reply::Done)
  }

  pub fn close_all(&mut self) -> BrowserResult<()> {
    let ids: Vec<_> = self.spaces.keys().cloned().collect();
    for id in ids {
      self.close_space(&id)?;
    }
    Ok(())
  }
}
