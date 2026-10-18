use crate::native::state::TabRecord;
use cef::*;

pub(in crate::native) fn request_paint_if_stale(tab: &TabRecord) -> bool {
  let stale = {
    let view = tab.view.borrow();
    view
      .frame
      .as_ref()
      .is_none_or(|frame| frame.revision != view.revision)
  };
  if stale && let Some(host) = tab.browser.host() {
    host.invalidate(PaintElementType::VIEW);
  }
  stale
}
