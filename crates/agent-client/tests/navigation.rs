use agent_client::navigation::Navigation;

#[test]
fn navigation_restores_context_invalidates_old_intents_and_branches_history() {
  let mut navigation = Navigation::new(("chat", 0));
  navigation.update_current(|context| context.1 = 12);
  assert_eq!(navigation.revision(), 0);
  navigation.invalidate();
  assert_eq!(navigation.revision(), 1);
  assert_eq!(navigation.current(), &("chat", 12));
  navigation.navigate(("settings", 0));
  let pending_revision = navigation.revision();
  assert!(navigation.back());
  assert_ne!(navigation.revision(), pending_revision);
  assert_eq!(navigation.current(), &("chat", 12));
  assert!(navigation.forward());
  assert!(navigation.back());
  navigation.navigate(("models", 0));
  assert!(
    !navigation.forward(),
    "new branch discards forward destinations"
  );
  let old = navigation.revision();
  navigation.reset(("chat", 0));
  assert!(navigation.revision() > old);
  assert!(!navigation.back());
}

#[test]
fn histories_are_bounded_and_independent_per_client() {
  let mut first = Navigation::new(0);
  let mut second = Navigation::new(0);
  for route in 1..=100 {
    first.navigate(route);
  }
  let mut count = 0;
  while first.back() {
    count += 1;
  }
  assert_eq!(count, 32);
  assert_eq!(first.current(), &68);
  assert_eq!(second.current(), &0);
  assert!(!second.back());
  for _ in 0..32 {
    assert!(first.forward());
  }
  assert!(!first.forward());
}
