use agent_launch::{
  model::Registration,
  native::{launchd_definition, systemd_definition},
};
use anyhow::Result;

fn registration() -> Registration {
  Registration {
    executable: "/Users/Example User/Library/daemon/agentd".into(),
    data_dir: "/Users/Example User/Library/Application Support/Lyonbrown".into(),
    version: "1.0.0".parse().expect("test version"),
    storage_schema_min: 1,
    storage_schema_max: 1,
  }
}

#[test]
fn staged_launchd_definition_has_shared_identity_and_separate_safe_arguments() -> Result<()> {
  let definition = launchd_definition(&registration())?;
  let parsed = plist::Value::from_reader_xml(definition.as_bytes())?;
  let dictionary = parsed.as_dictionary().expect("plist dictionary");
  assert_eq!(
    dictionary["Label"].as_string(),
    Some("org.lyonbrown.agentd")
  );
  let arguments = dictionary["ProgramArguments"].as_array().expect("argv");
  assert_eq!(arguments.len(), 5);
  assert_eq!(arguments[0].as_string(), registration().executable.to_str());
  assert_eq!(arguments[4].as_string(), registration().data_dir.to_str());
  assert_eq!(dictionary["RunAtLoad"].as_boolean(), Some(true));
  Ok(())
}

#[test]
fn systemd_definition_escapes_specifiers_and_rejects_line_injection() -> Result<()> {
  let mut registration = registration();
  registration.executable = "/home/a%user/$bin/agentd".into();
  let definition = systemd_definition(&registration)?;
  assert!(definition.contains("ExecStart=\"/home/a%%user/$$bin/agentd\""));
  assert!(definition.contains("Restart=on-failure"));
  registration.data_dir = "/home/user\nExecStart=/untrusted".into();
  assert!(systemd_definition(&registration).is_err());
  Ok(())
}
