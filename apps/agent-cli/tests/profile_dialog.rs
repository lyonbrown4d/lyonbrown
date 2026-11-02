use agent_cli::{
  backend::Action,
  model::{App, Connection},
  profile_dialog::{self, Focus, ProfileDialog, ProfileInput, ProfileOperation, ProfileResult},
};
use agent_protocol::v1::ProviderProfile;
use agent_secrets::ExposeSecret;
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use tokio::sync::mpsc;

fn fixture() -> (
  App,
  ProfileInput,
  mpsc::Sender<Action>,
  mpsc::Receiver<Action>,
) {
  let app = App {
    connection: Connection::Connected,
    profile_dialog: Some(ProfileDialog::new(Some(ProviderProfile {
      id: "profile-id".into(),
      name: "Private endpoint".into(),
      protocol: 1,
      base_url: "https://example.test/v1".into(),
      model: "model".into(),
      revision: 2,
      credential_configured: true,
    }))),
    ..Default::default()
  };
  let (sender, receiver) = mpsc::channel(8);
  (app, ProfileInput::default(), sender, receiver)
}
fn key(code: KeyCode) -> Event {
  Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
}

#[test]
fn secret_stays_out_of_app_and_ack_discards_secret() -> anyhow::Result<()> {
  let (mut app, mut input, sender, mut receiver) = fixture();
  app.profile_dialog.as_mut().expect("dialog").focus = Focus::Credential;
  input.handle(&mut app, &Event::Paste("super-private-key".into()), &sender)?;
  assert!(!format!("{app:?}").contains("super-private-key"));
  app.profile_dialog.as_mut().expect("dialog").focus = Focus::Save;
  input.handle(&mut app, &key(KeyCode::Enter), &sender)?;
  let Action::ProviderProfile {
    generation,
    dialog,
    operation,
  } = receiver.try_recv()?
  else {
    panic!("profile action expected")
  };
  let ProfileOperation::Save {
    mut profile,
    credential,
  } = operation
  else {
    panic!("save expected")
  };
  assert_eq!(credential.expose_secret(), "super-private-key");
  // Busy dialogs cannot submit twice.
  input.handle(&mut app, &key(KeyCode::Enter), &sender)?;
  assert!(receiver.try_recv().is_err());
  profile.revision += 1;
  profile_dialog::apply(
    &mut app,
    generation,
    &dialog,
    Ok(ProfileResult::Saved(profile)),
  );
  input.sync(&app);
  input.handle(&mut app, &key(KeyCode::Enter), &sender)?;
  let Action::ProviderProfile {
    operation: ProfileOperation::Save { credential, .. },
    ..
  } = receiver.try_recv()?
  else {
    panic!("save expected")
  };
  assert!(credential.expose_secret().is_empty());
  Ok(())
}

#[test]
fn disconnect_and_control_paste_discard_credentials() -> anyhow::Result<()> {
  let (mut app, mut input, sender, mut receiver) = fixture();
  app.profile_dialog.as_mut().expect("dialog").focus = Focus::Credential;
  input.handle(&mut app, &Event::Paste("private".into()), &sender)?;
  app.connection = Connection::Disconnected("offline".into());
  input.sync(&app);
  app.connection = Connection::Connected;
  input.handle(&mut app, &Event::Paste("bad\nkey".into()), &sender)?;
  app.profile_dialog.as_mut().expect("dialog").focus = Focus::Save;
  input.handle(&mut app, &key(KeyCode::Enter), &sender)?;
  let Action::ProviderProfile {
    operation: ProfileOperation::Save { credential, .. },
    ..
  } = receiver.try_recv()?
  else {
    panic!("save expected")
  };
  assert!(credential.expose_secret().is_empty());
  Ok(())
}

#[test]
fn removal_confirmation_is_revision_bound_and_escape_cancels() -> anyhow::Result<()> {
  let (mut app, mut input, sender, mut receiver) = fixture();
  app.profile_dialog.as_mut().expect("dialog").focus = Focus::Remove;
  input.handle(&mut app, &key(KeyCode::Enter), &sender)?;
  assert!(receiver.try_recv().is_err());
  input.handle(&mut app, &key(KeyCode::Esc), &sender)?;
  assert!(!app.profile_dialog.as_ref().expect("dialog").confirm_remove);
  input.handle(&mut app, &key(KeyCode::Enter), &sender)?;
  input.handle(&mut app, &key(KeyCode::Enter), &sender)?;
  let Action::ProviderProfile {
    operation: ProfileOperation::Remove { id, revision },
    ..
  } = receiver.try_recv()?
  else {
    panic!("remove expected")
  };
  assert_eq!((id.as_str(), revision), ("profile-id", 2));
  Ok(())
}

#[test]
fn stale_response_does_not_replace_current_dialog() {
  let (mut app, _, _, _) = fixture();
  let id = app.profile_dialog.as_ref().expect("dialog").id.clone();
  let profile = ProviderProfile {
    id: "stale".into(),
    ..Default::default()
  };
  profile_dialog::apply(&mut app, 1, &id, Ok(ProfileResult::Saved(profile.clone())));
  profile_dialog::apply(
    &mut app,
    0,
    "another-dialog",
    Ok(ProfileResult::Saved(profile)),
  );
  assert!(app.provider_profiles.is_empty());
  assert_eq!(
    app.profile_dialog.as_ref().expect("dialog").profile.id,
    "profile-id"
  );
}
