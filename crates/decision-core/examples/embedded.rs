use decision_core::{
  DecisionCancellation, DecisionEngine, DecisionRequest, Operation, Policy, RouteInput,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
  let engine = DecisionEngine::new(Policy::default(), vec![])?;
  let request = DecisionRequest::new(
    "offline-example",
    Operation::RouteModel(RouteInput {
      text: "检查 Rust 代码中的锁竞争".into(),
      ..RouteInput::default()
    }),
  );
  let result = engine.decide(&request, &DecisionCancellation::new()).await;
  println!("{}", serde_json::to_string_pretty(&result)?);
  Ok(())
}
