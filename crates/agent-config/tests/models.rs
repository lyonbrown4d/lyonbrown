use agent_config::{LocalModelSettings, load_with_environment};
use std::collections::BTreeMap;

#[test]
fn local_profiles_load_without_cloud_credentials_and_keep_cpu_defaults() {
  let data = tempfile::tempdir().unwrap();
  let path = data.path().join("config.toml");
  std::fs::write(
    &path,
    r#"
provider = "local"
model = "assistant"
[[local_models]]
id = "assistant"
backend = "llama"
path = "/models/assistant.gguf"
context_window = 2048
max_output_tokens = 128
[decision]
provider = "local"
model = "assistant"
"#,
  )
  .unwrap();
  let settings = load_with_environment(data.path(), Some(&path), &BTreeMap::new()).unwrap();
  assert_eq!(settings.local_models.len(), 1);
  assert_eq!(settings.local_models[0].parallel_requests, 1);
  assert!(!settings.local_models[0].supports_tools);
  assert!(settings.credential("openai").is_none());
  assert_eq!(settings.decision.model.as_deref(), Some("assistant"));
}
#[test]
fn invalid_local_profile_budgets_and_duplicate_ids_are_rejected() {
  let mut profile = LocalModelSettings {
    id: "fixture".into(),
    path: "/models/fixture.gguf".into(),
    ..Default::default()
  };
  agent_config::validate_local_models(&[profile.clone()]).unwrap();
  profile.supports_tools = true;
  assert!(
    profile.validate().is_err(),
    "llama cannot claim SDK tool support"
  );
  profile.backend = agent_config::LocalModelBackend::Mistral;
  profile.validate().unwrap();
  profile.supports_tools = false;
  assert!(agent_config::validate_local_models(&[profile.clone(), profile.clone()]).is_err());
  profile.max_output_tokens = profile.context_window;
  assert!(profile.validate().is_err());
  profile.max_output_tokens = 1;
  profile.parallel_requests = 0;
  assert!(profile.validate().is_err());
}

#[test]
fn template_override_requires_supported_backend_and_bounded_regular_jinja_file() {
  let data = tempfile::tempdir().unwrap();
  let path = data.path().join("chat.jinja");
  std::fs::write(&path, "{{ messages }}").unwrap();
  let mut profile = LocalModelSettings {
    id: "template".into(),
    path: "/models/fixture.gguf".into(),
    chat_template: Some(path.clone()),
    ..Default::default()
  };
  assert!(profile.validate().is_err(), "llama override is unsupported");
  profile.backend = agent_config::LocalModelBackend::Mistral;
  profile.validate().unwrap();
  std::fs::write(&path, []).unwrap();
  assert!(profile.validate().is_err());
  std::fs::write(&path, vec![b'a'; 1024 * 1024 + 1]).unwrap();
  assert!(profile.validate().is_err());
  profile.chat_template = Some("relative.jinja".into());
  assert!(profile.validate().is_err());
  #[cfg(unix)]
  {
    let link = data.path().join("symlink.jinja");
    std::os::unix::fs::symlink(&path, &link).unwrap();
    profile.chat_template = Some(link);
    assert!(profile.validate().is_err());
  }
}
