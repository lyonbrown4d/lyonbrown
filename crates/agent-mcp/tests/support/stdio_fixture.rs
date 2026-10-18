//! SDK-only subprocess fixture, enabled explicitly for transport tests.
mod fixture;
use rmcp::ServiceExt;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
  eprintln!("fixture stderr is private");
  let service = fixture::Fixture::default()
    .serve(rmcp::transport::stdio())
    .await?;
  service.waiting().await?;
  Ok(())
}
