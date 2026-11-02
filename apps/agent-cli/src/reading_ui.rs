use crate::model::App;
pub(crate) fn search(app: &mut App, query: &str) {
  sync(app);
  app.reading.set_query(query);
  show(app);
}
pub(crate) fn sync(app: &mut App) {
  app.reading.sync(
    app
      .message_keys
      .iter()
      .zip(&app.messages)
      .map(|(key, message)| (key.as_str(), message.content.as_str())),
  );
}
pub(crate) fn next(app: &mut App, forward: bool) {
  sync(app);
  if forward {
    app.reading.next_hit();
  } else {
    app.reading.previous_hit();
  }
  show(app);
}
fn show(app: &mut App) {
  if let Some(hit) = app.reading.current() {
    app.transcript_anchor = Some(hit.anchor.message_id.clone());
    app.scroll = 0;
    app.router.close();
    let (index, total) = app.reading.position().unwrap_or((0, 0));
    app.notice = format!("搜索 {index}/{total} · Alt+N/P 前后命中 · End 返回最新消息");
  } else {
    app.notice = "没有匹配的对话内容。搜索不会发送给模型。".into();
  }
}
