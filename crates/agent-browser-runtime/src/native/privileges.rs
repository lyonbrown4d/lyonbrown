//! Web content cannot open privileged OS UI outside the daemon's permission flow.
use cef::*;

wrap_dialog_handler! {
  pub struct FileDialogs {}
  impl DialogHandler {
    fn on_file_dialog(&self,_browser:Option<&mut Browser>,_mode:FileDialogMode,
      _title:Option<&CefString>,_default_path:Option<&CefString>,
      _filters:Option<&mut CefStringList>,_extensions:Option<&mut CefStringList>,
      _descriptions:Option<&mut CefStringList>,callback:Option<&mut FileDialogCallback>)->i32 {
      if let Some(callback)=callback {callback.cancel();} 1
    }
  }
}
wrap_download_handler! {
  pub struct Downloads {}
  impl DownloadHandler {
    fn can_download(&self,_browser:Option<&mut Browser>,_url:Option<&CefString>,_method:Option<&CefString>)->i32 {0}
  }
}
wrap_permission_handler! {
  pub struct Permissions {}
  impl PermissionHandler {
    fn on_request_media_access_permission(&self,_browser:Option<&mut Browser>,_frame:Option<&mut Frame>,
      _origin:Option<&CefString>,_permissions:u32,callback:Option<&mut MediaAccessCallback>)->i32 {
      if let Some(callback)=callback {callback.cancel();} 1
    }
    fn on_show_permission_prompt(&self,_browser:Option<&mut Browser>,_id:u64,
      _origin:Option<&CefString>,_permissions:u32,callback:Option<&mut PermissionPromptCallback>)->i32 {
      if let Some(callback)=callback {callback.cont(PermissionRequestResult::DENY);} 1
    }
  }
}
wrap_jsdialog_handler! {
  pub struct ScriptDialogs {}
  impl JsdialogHandler {
    fn on_jsdialog(&self,_browser:Option<&mut Browser>,_origin:Option<&CefString>,_kind:JsdialogType,
      _message:Option<&CefString>,_default:Option<&CefString>,_callback:Option<&mut JsdialogCallback>,suppress:Option<&mut i32>)->i32 {
      if let Some(suppress)=suppress {*suppress=1;} 0
    }
    fn on_before_unload_dialog(&self,_browser:Option<&mut Browser>,_message:Option<&CefString>,_reload:i32,
      callback:Option<&mut JsdialogCallback>)->i32 {
      if let Some(callback)=callback {callback.cont(1,None);} 1
    }
  }
}
