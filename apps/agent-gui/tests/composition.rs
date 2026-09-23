use agent_gui::composition::CompositionRoot;

#[test]
fn composition_root_validates_and_creates_cockpit() {
  let root = CompositionRoot::new().unwrap();

  root.validate().unwrap();
  assert!(root.create_cockpit().is_ok());
}
