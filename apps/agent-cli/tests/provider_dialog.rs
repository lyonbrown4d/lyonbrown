use agent_cli::{
  backend::{Action, Update},
  input,
  model::{App, Connection},
  picker::Picker,
  provider_dialog::{CredentialChange, CredentialInput, Focus},
  update,
};
use agent_protocol::v1::Provider;
use agent_secrets::ExposeSecret;
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use tokio::sync::mpsc;
fn enter() -> Event {
  Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
}
#[test]
fn choosing_provider_opens_dialog_and_credentials_never_enter_chat_state() {
  let provider = Provider {
    id: "openai".into(),
    name: "OpenAI".into(),
    runtime_supported: true,
    credential_state: "configured".into(),
    ..Default::default()
  };
  let mut app = App {
    connection: Connection::Connected,
    provider_catalog: vec![provider.clone()],
    picker: Some(Picker::providers(std::slice::from_ref(&provider), &[])),
    ..Default::default()
  };
  let (sender, mut receiver) = mpsc::channel(8);
  input::handle(&mut app, enter(), &sender).unwrap();
  assert!(
    receiver.try_recv().is_err(),
    "opening a dialog is not upload consent"
  );
  assert!(app.provider_dialog.is_some());
  let mut credential = CredentialInput::default();
  let secret = "synthetic-ephemeral-only";
  credential
    .handle(&mut app, &Event::Paste(secret.into()), &sender)
    .unwrap();
  input::handle(
    &mut app,
    Event::Paste("must-not-enter-chat".into()),
    &sender,
  )
  .unwrap();
  assert!(app.input.is_empty());
  assert!(!format!("{app:?}").contains(secret));
  assert!(
    !serde_json::to_string(&app.client_state())
      .unwrap()
      .contains(secret)
  );
  app.provider_dialog.as_mut().unwrap().focus = Focus::Save;
  credential.handle(&mut app, &enter(), &sender).unwrap();
  let Action::ProviderCredential {
    generation,
    dialog,
    change: CredentialChange::Typed(value),
    ..
  } = receiver.try_recv().unwrap()
  else {
    panic!("credential action")
  };
  assert_eq!(value.expose_secret(), secret);
  update::apply(
    &mut app,
    Update::ProviderCredential {
      generation,
      dialog,
      result: Ok(provider),
    },
  );
  credential.sync(&app);
  credential.handle(&mut app, &enter(), &sender).unwrap();
  assert!(
    receiver.try_recv().is_err(),
    "acknowledged secret editor must be empty"
  );
  assert!(!app.provider_dialog.as_ref().unwrap().credential_present);
  assert!(app.input.is_empty());
}
