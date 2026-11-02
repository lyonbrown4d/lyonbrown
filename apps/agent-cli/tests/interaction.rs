use agent_cli::{
  composer::Composer,
  interaction::{
    InputHistory, InteractionState, MAX_HISTORY_BYTES, MAX_HISTORY_ENTRIES, MAX_HISTORY_SESSIONS,
    MAX_PROMPT_BYTES,
  },
  picker::{Picker, PickerItem, PickerValue},
};

#[test]
fn filtered_provider_confirmation_uses_original_identity_and_empty_results_cannot_confirm() {
  let mut picker = Picker::loading("Providers");
  picker.loading = false;
  picker.items = ["alpha", "beta", "gamma"]
    .into_iter()
    .map(|provider| PickerItem {
      label: provider.into(),
      detail: String::new(),
      value: PickerValue::Provider {
        provider: provider.into(),
        environment: format!("{provider}_KEY"),
      },
    })
    .collect();
  picker.set_query("BETA");
  picker.next();
  assert_eq!(picker.visible_indices(), vec![1]);
  assert_eq!(
    picker.selected_value(),
    Some(&PickerValue::Provider {
      provider: "beta".into(),
      environment: "beta_KEY".into(),
    })
  );
  picker.filter('!');
  assert!(picker.is_empty());
  assert_eq!(picker.selected_value(), None);
  picker.backspace();
  assert_eq!(picker.selected, 1);
  picker.set_query("a");
  picker.next();
  assert_eq!(picker.selected, 2);
  picker.previous();
  assert_eq!(picker.selected, 1);
}

#[test]
fn recall_search_and_session_switch_only_refill_editor_and_preserve_original_draft() {
  let mut state = InteractionState::default();
  state.bind_connection("daemon-account-a");
  state.record_acknowledged("a", "First prompt");
  state.record_acknowledged("a", "Second prompt");
  state.record_acknowledged("b", "Other session");
  assert_eq!(
    state.previous("a", "unsent draft").as_deref(),
    Some("Second prompt")
  );
  assert_eq!(
    state.previous("b", "other draft").as_deref(),
    Some("Other session")
  );
  assert_eq!(
    state.previous("a", "Second prompt").as_deref(),
    Some("First prompt")
  );
  assert_eq!(state.next("a").as_deref(), Some("Second prompt"));
  assert_eq!(state.next("a").as_deref(), Some("unsent draft"));
  assert_eq!(state.next("a"), None);
  assert_eq!(
    state.history.search("a", "FIRST"),
    vec![(0, "First prompt")]
  );
  let recalled = state.select("a", 0).expect("existing match");
  let mut composer = Composer::from("unsubmitted");
  composer.replace_text(&recalled);
  assert_eq!(composer.text(), "First prompt");
  assert_eq!(state.history.entries("a").len(), 2);
  assert_eq!(state.select("b", 5), None);
}

#[test]
fn acknowledged_history_deduplicates_and_evicts_by_count_session_and_bytes() {
  let mut history = InputHistory::default();
  for index in 0..MAX_HISTORY_ENTRIES + 1 {
    history.record_acknowledged("a", &format!("prompt {index}"));
  }
  assert_eq!(history.entries("a").len(), MAX_HISTORY_ENTRIES);
  assert_eq!(history.entries("a")[0], "prompt 1");
  history.record_acknowledged("a", "prompt 1");
  assert_eq!(history.entries("a").len(), MAX_HISTORY_ENTRIES);
  assert_eq!(
    history.entries("a").last().map(String::as_str),
    Some("prompt 1")
  );
  assert!(!history.record_acknowledged("a", &"x".repeat(MAX_PROMPT_BYTES + 1)));
  assert!(!history.record_acknowledged("a", "  \n"));
  for index in 0..MAX_HISTORY_SESSIONS {
    history.record_acknowledged(&format!("session-{index}"), "small prompt");
  }
  assert!(history.entries("a").is_empty());
  for index in 0..10 {
    let prompt = format!("{index}{}", "x".repeat(MAX_PROMPT_BYTES - 1));
    assert!(history.record_acknowledged("large", &prompt));
  }
  assert!(history.total_bytes() <= MAX_HISTORY_BYTES);
  let retained = (MAX_HISTORY_BYTES / MAX_PROMPT_BYTES).min(10);
  assert_eq!(history.entries("large").len(), retained);
  assert!(history.entries("large")[0].starts_with(&(10 - retained).to_string()));
}

#[test]
fn persistence_excludes_drafts_and_rejects_history_from_another_connection() {
  let mut state = InteractionState::default();
  state.bind_connection("first-account");
  state.record_acknowledged("a", "acknowledged");
  state.previous("a", "sensitive draft never acknowledged");
  let json = serde_json::to_string(&state).expect("serialize");
  assert!(!json.contains("sensitive draft"));
  let stored: InteractionState = serde_json::from_str(&json).expect("deserialize");
  state.bind_connection("second-account");
  assert!(state.history.entries("a").is_empty());
  assert_eq!(state.next("a"), None);
  assert!(!state.import_history(stored.clone()));
  state.bind_connection("first-account");
  assert!(state.import_history(stored));
  assert_eq!(
    state.previous("a", "new draft").as_deref(),
    Some("acknowledged")
  );
  assert_eq!(state.next("a").as_deref(), Some("new draft"));
}

#[test]
fn imported_history_is_normalized_to_runtime_limits() {
  let json = serde_json::json!({"sessions": [{"session": "a", "prompts":
    (0..150).map(|index| format!("prompt {index}")).collect::<Vec<_>>()
  }]});
  let history: InputHistory = serde_json::from_value(json).expect("valid history");
  assert_eq!(history.entries("a").len(), MAX_HISTORY_ENTRIES);
  assert_eq!(history.entries("a")[0], "prompt 50");
}
