fn main() -> Result<(), Box<dyn std::error::Error>> {
  let protoc = protoc_bin_vendored::protoc_bin_path()?;
  let mut prost = prost_build::Config::new();
  prost.protoc_executable(protoc);
  tonic_prost_build::configure().compile_with_config(
    prost,
    &["proto/agent/v1/agent.proto"],
    &["proto"],
  )?;
  for source in [
    "proto/agent/v1/agent.proto",
    "proto/agent/v1/commands.proto",
    "proto/agent/v1/snapshot.proto",
  ] {
    println!("cargo:rerun-if-changed={source}");
  }
  Ok(())
}
