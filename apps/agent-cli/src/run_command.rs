use clap::{Args, Subcommand};

/// Explicit recovery of interrupted runs; never automatic prompt replay.
#[derive(Subcommand)]
pub enum RunCommand {
  Inspect {
    run_id: String,
  },
  /// Record that the original uncertain run will be left stopped.
  Abandon(Recovery),
  /// Start fresh in a new session with explicitly supplied input.
  Continue {
    #[command(flatten)]
    recovery: Recovery,
    #[arg(long)]
    prompt: String,
  },
}

#[derive(Args)]
pub struct Recovery {
  pub run_id: String,
  /// Reuse this identity after a lost response; inspect before doing anything else.
  #[arg(long)]
  pub idempotency_key: String,
  /// What you checked about possible file, command or remote side effects.
  #[arg(long)]
  pub note: String,
}
