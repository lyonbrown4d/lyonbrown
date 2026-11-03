use agent_cli::{
  backend::{Action, Update},
  input,
  media::{Reply, Request},
  model::{App, Connection, Session},
};
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
fn ready() -> anyhow::Result<App> {
  let mut app = App::default();
  app.connection = Connection::Connected;
  app.state_loaded = true;
  app.sessions = vec![Session {
    id: "session".into(),
    title: "images".into(),
  }];
  let image = agent_media::ImageAttachment::from_bytes(
    include_bytes!("../../../crates/agent-media/tests/fixtures/one.png").to_vec(),
  )?;
  agent_cli::update::apply(
    &mut app,
    Update::Media(Reply::Attached {
      session: "session".into(),
      name: "chosen.png".into(),
      image,
    }),
  );
  Ok(app)
}
#[test]
fn terminal_keeps_images_and_exact_request_until_acknowledgement() -> anyhow::Result<()> {
  let mut app = ready()?;
  app.input.replace_text("describe");
  let (actions, mut received) = tokio::sync::mpsc::channel(8);
  let enter = || Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
  input::handle(&mut app, enter(), &actions)?;
  let Action::Media(Request::Submit {
    submission: first, ..
  }) = received.try_recv()?
  else {
    panic!("image submission required");
  };
  agent_cli::update::apply(
    &mut app,
    Update::Notice("unconfirmed transport result".into()),
  );
  input::handle(&mut app, enter(), &actions)?;
  let Action::Media(Request::Submit {
    submission: retry, ..
  }) = received.try_recv()?
  else {
    panic!("same image request required");
  };
  assert_eq!(first.request_id, retry.request_id);
  assert_eq!(first.images, retry.images);
  agent_cli::update::apply(
    &mut app,
    Update::Media(Reply::Confirmed {
      session: "session".into(),
      request_id: first.request_id,
    }),
  );
  let generation = app.generation;
  agent_cli::update::apply(
    &mut app,
    Update::Submitted {
      session: "session".into(),
      prompt: "describe".into(),
      generation,
    },
  );
  app.input.replace_text("next text task");
  input::handle(&mut app, enter(), &actions)?;
  assert!(matches!(received.try_recv()?, Action::Submit { .. }));
  Ok(())
}
#[test]
fn attached_images_never_turn_into_implicit_follow_up_or_steering() -> anyhow::Result<()> {
  let mut app = ready()?;
  app.input.replace_text("describe");
  app.active_run = Some("running".into());
  let (actions, mut received) = tokio::sync::mpsc::channel(8);
  input::handle(
    &mut app,
    Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
    &actions,
  )?;
  assert!(received.try_recv().is_err());
  assert_eq!(app.input.text(), "describe");
  Ok(())
}
