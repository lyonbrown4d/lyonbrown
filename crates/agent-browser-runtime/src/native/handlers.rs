use agent_browser::{Frame as CpuFrame, MAX_FRAME_BYTES};
use cef::{rc::Rc as _, *};
use std::{cell::RefCell, collections::BTreeMap, rc::Rc};

pub struct TabView {
  pub id: String,
  pub revision: u64,
  pub sequence: u64,
  pub viewport: (u32, u32),
  pub frame: Option<CpuFrame>,
  pub closed: bool,
  pub loading: bool,
  pub dom_ready: bool,
  pub results: BTreeMap<i32, serde_json::Value>,
  pub pending_id: Option<i32>,
  pub pending_error: Option<agent_browser::BrowserError>,
}

impl TabView {
  pub fn new(id: String) -> Rc<RefCell<Self>> {
    Rc::new(RefCell::new(Self {
      id,
      revision: 1,
      sequence: 0,
      viewport: (1280, 720),
      frame: None,
      closed: false,
      loading: false,
      dom_ready: false,
      results: BTreeMap::new(),
      pending_id: None,
      pending_error: None,
    }))
  }
}

wrap_app! {
  pub struct RuntimeApp { fixture_keychain: bool, context_ready: Rc<std::cell::Cell<bool>> }
  impl App {
    fn browser_process_handler(&self)->Option<BrowserProcessHandler> {Some(BrowserCallbacks::new(self.context_ready.clone()))}
    fn on_before_command_line_processing(&self,_process_type:Option<&CefString>,command_line:Option<&mut CommandLine>) {
      if cfg!(debug_assertions) && self.fixture_keychain && let Some(command_line)=command_line {
        command_line.append_switch(Some(&"use-mock-keychain".into()));
      }
    }
  }
}

wrap_browser_process_handler! {
  struct BrowserCallbacks { ready: Rc<std::cell::Cell<bool>> }
  impl BrowserProcessHandler {
    fn on_context_initialized(&self) {self.ready.set(true);}
  }
}

wrap_request_context_handler! {
  pub struct ContextCallbacks { ready: Rc<std::cell::Cell<bool>> }
  impl RequestContextHandler {
    fn on_request_context_initialized(&self,_context:Option<&mut RequestContext>) {self.ready.set(true);}
  }
}

wrap_render_handler! {
  struct RenderCallbacks { view: Rc<RefCell<TabView>> }
  impl RenderHandler {
    fn view_rect(&self, _browser: Option<&mut Browser>, rect: Option<&mut Rect>) {
      if let Some(rect) = rect {
        let view = self.view.borrow();
        rect.width = view.viewport.0 as i32;
        rect.height = view.viewport.1 as i32;
      }
    }
    fn on_paint(&self, _browser: Option<&mut Browser>, kind: PaintElementType,
      _dirty_rects: Option<&[Rect]>, buffer: *const u8, width: i32, height: i32) {
      if buffer.is_null() || width <= 0 || height <= 0 || kind != PaintElementType::default() { return; }
      let Some(length) = (width as usize).checked_mul(height as usize).and_then(|length| length.checked_mul(4)) else { return; };
      if length > MAX_FRAME_BYTES { return; }
      // SAFETY: CEF supplies width*height BGRA pixels valid only during this callback.
      // Dimensions and length were checked above; copy before returning or crossing IPC.
      let pixels = unsafe { std::slice::from_raw_parts(buffer, length) }.to_vec();
      let mut view = self.view.borrow_mut();
      if view.frame.is_none() {tracing::info!(event="browser_first_paint",width,height,"CEF produced native pixels");}
      view.sequence = view.sequence.saturating_add(1);
      view.frame = Some(CpuFrame { tab_id: view.id.clone(), revision: view.revision,
        sequence: view.sequence, width: width as u32, height: height as u32,
        stride: width as u32 * 4, bgra: pixels });
    }
  }
}

wrap_life_span_handler! {
  struct LifetimeCallbacks { view: Rc<RefCell<TabView>> }
  impl LifeSpanHandler {
    fn on_before_popup(&self, _browser: Option<&mut Browser>, _frame: Option<&mut Frame>,
      _popup_id: i32, _target_url: Option<&CefString>, _target_frame_name: Option<&CefString>,
      _target_disposition: WindowOpenDisposition, _user_gesture: i32,
      _popup_features: Option<&PopupFeatures>, _window_info: Option<&mut WindowInfo>,
      _client: Option<&mut Option<Client>>, _settings: Option<&mut BrowserSettings>,
      _extra_info: Option<&mut Option<DictionaryValue>>, _no_javascript_access: Option<&mut i32>) -> i32 {
      // Unregistered popup windows cannot bypass daemon tab ownership or permissions.
      1
    }
    fn on_before_close(&self, _browser: Option<&mut Browser>) { self.view.borrow_mut().closed = true; }
  }
}

wrap_load_handler! {
  struct LoadCallbacks { view: Rc<RefCell<TabView>> }
  impl LoadHandler {
    fn on_load_start(&self, _browser: Option<&mut Browser>, frame: Option<&mut Frame>, _transition_type: TransitionType) {
      if frame.is_some_and(|frame| frame.is_main() == 1) {
        let mut view = self.view.borrow_mut();
        view.revision = view.revision.saturating_add(1);
        view.dom_ready=false;
      }
    }
    fn on_loading_state_change(&self, _browser: Option<&mut Browser>, loading: i32, _back: i32, _forward: i32) {
      tracing::debug!(event="browser_loading_state",loading,"CEF navigation progress");
      self.view.borrow_mut().loading = loading == 1;
    }
    fn on_load_error(&self,_browser:Option<&mut Browser>,_frame:Option<&mut Frame>,error_code:Errorcode,_text:Option<&CefString>,_url:Option<&CefString>) {
      tracing::warn!(event="browser_navigation_failed",error_code=error_code.get_raw(),"CEF navigation failed");
    }
  }
}

wrap_client! {
  pub struct RuntimeClient { view: Rc<RefCell<TabView>> }
  impl Client {
    fn render_handler(&self) -> Option<RenderHandler> { Some(RenderCallbacks::new(self.view.clone())) }
    fn life_span_handler(&self) -> Option<LifeSpanHandler> { Some(LifetimeCallbacks::new(self.view.clone())) }
    fn load_handler(&self) -> Option<LoadHandler> { Some(LoadCallbacks::new(self.view.clone())) }
    fn dialog_handler(&self)->Option<DialogHandler> {Some(super::privileges::FileDialogs::new())}
    fn download_handler(&self)->Option<DownloadHandler> {Some(super::privileges::Downloads::new())}
    fn permission_handler(&self)->Option<PermissionHandler> {Some(super::privileges::Permissions::new())}
    fn jsdialog_handler(&self)->Option<JsdialogHandler> {Some(super::privileges::ScriptDialogs::new())}
    fn request_handler(&self) -> Option<RequestHandler> { Some(RequestCallbacks::new()) }
  }
}

wrap_request_handler! {
  struct RequestCallbacks {}
  impl RequestHandler {
    fn on_render_view_ready(&self,_browser:Option<&mut Browser>) {tracing::info!(event="browser_renderer_ready","CEF renderer is ready");}
    fn on_render_process_unresponsive(&self,_browser:Option<&mut Browser>,_callback:Option<&mut UnresponsiveProcessCallback>)->i32 {tracing::warn!(event="browser_renderer_unresponsive","CEF renderer is unresponsive");0}
    fn on_render_process_terminated(&self,_browser:Option<&mut Browser>,status:TerminationStatus,error_code:i32,_error:Option<&CefString>) {tracing::error!(event="browser_renderer_terminated",status=status.get_raw(),error_code,"CEF renderer terminated");}
    fn on_before_browse(&self, _browser: Option<&mut Browser>, _frame: Option<&mut Frame>,
      request: Option<&mut Request>, _user_gesture: i32, _redirect: i32) -> i32 {
      let denied=request.is_none_or(|request| agent_browser::validate_url(&CefString::from(&request.url()).to_string()).is_err());
      tracing::debug!(event="browser_navigation_policy",denied,"Browser navigation policy checked");
      i32::from(denied)
    }
  }
}

wrap_dev_tools_message_observer! {
  pub struct DevToolsCallbacks { view: Rc<RefCell<TabView>> }
  impl DevToolsMessageObserver {
    fn on_dev_tools_message(&self, _browser: Option<&mut Browser>, message: Option<&[u8]>) -> i32 {
      let Some(message)=message else{return 1};
      if message.len()>agent_browser::MAX_AX_BYTES {
        let mut view=self.view.borrow_mut();
        if view.pending_id.is_some(){view.pending_error=Some(agent_browser::BrowserError::InvalidInput("Chromium response exceeds 1 MiB".into()));}
        return 1;
      }
      if let Ok(value) = serde_json::from_slice::<serde_json::Value>(message) {
        let mut view = self.view.borrow_mut();
        if let Some(id) = value["id"].as_i64() {if view.pending_id==Some(id as i32) {view.results.insert(id as i32, value);}}
        else if matches!(value["method"].as_str(), Some("DOM.documentUpdated" | "Page.frameNavigated" | "DOM.attributeModified" | "DOM.childNodeCountUpdated" | "DOM.childNodeInserted" | "DOM.childNodeRemoved" | "DOM.characterDataModified")) {
          view.revision = view.revision.saturating_add(1);
          if matches!(value["method"].as_str(),Some("DOM.documentUpdated"|"Page.frameNavigated")) {view.dom_ready=false;}
        }
      }
      1
    }
  }
}
