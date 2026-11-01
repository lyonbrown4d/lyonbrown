use crate::{
  backend::Update,
  model::{App, Session},
};

pub(crate) fn apply(
  app: &mut App,
  session: Option<String>,
  generation: u64,
  source: String,
  value: serde_json::Value,
) {
  if generation != app.generation
    || session.as_deref() != app.selected_session().map(|session| session.id.as_str())
  {
    return;
  }
  if let Some(session) = value
    .get("session")
    .or_else(|| value.pointer("/resolution/session"))
    && let (Some(id), Some(title)) = (
      session.get("id").and_then(|v| v.as_str()),
      session.get("title").and_then(|v| v.as_str()),
    )
    && !app.sessions.iter().any(|session| session.id == id)
  {
    app.sessions.push(Session {
      id: id.into(),
      title: title.into(),
    });
  }
  crate::update::apply(
    app,
    Update::TaskResult {
      session,
      generation,
      source: Some(source),
      append: false,
      value,
    },
  );
}
