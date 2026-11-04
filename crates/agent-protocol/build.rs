fn main() -> Result<(), Box<dyn std::error::Error>> {
  let mut config = prost_build::Config::new();
  config.skip_debug([
    ".agent.v1.RunImageAttachment",
    ".agent.v1.StartRunWithAttachmentsRequest",
  ]);
  config.protoc_executable(protoc_bin_vendored::protoc_bin_path()?);
  tonic_prost_build::configure().compile_with_config(
    config,
    &["../../proto/agent/v1/agent.proto"],
    &["../../proto"],
  )?;
  println!("cargo:rerun-if-changed=../../proto/agent/v1/agent.proto");
  Ok(())
}
