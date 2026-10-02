use agent_runtime::model_manager::{
  ModelPreset, model_destination_key, normalize_destination_filename, validate_catalog_entries,
};

fn preset(id: &'static str, filename: &'static str, memory: u64) -> ModelPreset {
  ModelPreset {
    id,
    owner: "owner",
    repository: "repository",
    revision: "main",
    filename,
    sha256: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".into(),
    download_bytes: 1,
    max_bytes: 1,
    min_available_memory_bytes: memory,
  }
}

#[test]
fn catalog_rejects_case_insensitive_filename_conflicts() {
  let presets = [
    preset("first", "Model.gguf", 1),
    preset("second", "model.GGUF", 2),
  ];

  let error = validate_catalog_entries(&presets).unwrap_err();

  assert!(error.to_string().contains("case-insensitive filename"));
}

#[test]
fn destination_identity_is_case_insensitive_on_every_platform() {
  assert_eq!(
    normalize_destination_filename("Model.gguf").unwrap(),
    "model.gguf"
  );
  assert_eq!(
    model_destination_key("Model.gguf").unwrap(),
    model_destination_key("model.gguf").unwrap()
  );
}
