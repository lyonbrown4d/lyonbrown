fn main() {
  // The proc macro cannot tell Cargo which files it reads. Watching the directory
  // also covers new migrations and metadata.toml changes.
  println!("cargo:rerun-if-changed=migrations");
}
