fn main() {
  // No screenshot, window title, input injection, OS prompt or permission change.
  println!("{:?}", agent_computer::native::inspect_permissions());
}
