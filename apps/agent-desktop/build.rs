fn main() {
  println!("cargo:rerun-if-env-changed=CARGO_CFG_TARGET_OS");
  println!("cargo:rerun-if-env-changed=CARGO_FEATURE_NATIVE_CEF");
  if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("linux")
    && std::env::var_os("CARGO_FEATURE_NATIVE_CEF").is_some()
  {
    // The native package places the matching CEF library beside this binary.
    println!("cargo:rustc-link-arg=-Wl,-rpath,$ORIGIN");
  }
}
