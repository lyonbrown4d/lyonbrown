use agent_browser::BrowserError;
use agent_browser_runtime::CacheRootLease;

#[test]
fn persistent_product_roots_have_exclusive_process_ownership() {
  let directory = tempfile::tempdir().expect("private runtime storage");
  let desktop = directory.path().join("desktop");
  let cli = directory.path().join("cli");
  std::fs::create_dir_all(&desktop).expect("desktop root");
  std::fs::create_dir_all(&cli).expect("CLI root");
  let first = CacheRootLease::acquire(&desktop).expect("first desktop runtime");
  assert!(matches!(
    CacheRootLease::acquire(&desktop),
    Err(BrowserError::Unavailable(message)) if message.contains("already owned")
  ));
  let _cli = CacheRootLease::acquire(&cli).expect("independent CLI runtime root");
  let profile = desktop.join("persistent-profile");
  std::fs::create_dir_all(&profile).expect("persistent profile");
  std::fs::write(profile.join("fixture-state"), "retained profile state").expect("profile state");
  drop(first);
  let _reopened = CacheRootLease::acquire(&desktop).expect("released root can be reopened");
  assert_eq!(
    std::fs::read_to_string(profile.join("fixture-state")).expect("retained state"),
    "retained profile state"
  );
}
