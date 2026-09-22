use agent_cli::cli::{Cli, Command};
use clap::Parser;

#[test]
fn installed_binary_is_named_agent() {
    let binary = std::path::Path::new(env!("CARGO_BIN_EXE_agent"));

    assert_eq!(binary.file_stem().unwrap(), "agent");
    let output = std::process::Command::new(binary)
        .arg("--help")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("Usage: agent"));
}

#[test]
fn ask_accepts_prompt_and_provider_overrides_after_subcommand() {
    let cli = Cli::try_parse_from([
        "agent",
        "ask",
        "say hi",
        "--provider",
        "ollama",
        "--model",
        "translategemma:4b",
        "--approve-all",
    ])
    .unwrap();

    assert_eq!(cli.provider.as_deref(), Some("ollama"));
    assert_eq!(cli.model.as_deref(), Some("translategemma:4b"));
    assert!(cli.approve_all);
    assert!(matches!(cli.command, Command::Ask { prompt } if prompt == "say hi"));
}

#[test]
fn chat_subcommand_is_available() {
    let cli =
        Cli::try_parse_from(["agent", "chat", "--endpoint", "http://localhost:11434"]).unwrap();

    assert_eq!(cli.endpoint.as_deref(), Some("http://localhost:11434"));
    assert!(matches!(cli.command, Command::Chat));
}

#[test]
fn dot_is_workspace_entrypoint_and_profile_can_be_overridden() {
    let cli = Cli::try_parse_from(["agent", ".", "--profile", "careful"]).unwrap();
    assert_eq!(cli.profile.as_deref(), Some("careful"));
    assert!(
        matches!(cli.command, Command::Tui { workspace } if workspace == std::path::Path::new("."))
    );
}

#[test]
fn command_is_required() {
    assert!(Cli::try_parse_from(["agent"]).is_err());
}

#[test]
fn approve_all_and_deny_all_are_mutually_exclusive() {
    assert!(Cli::try_parse_from(["agent", "ask", "hi", "--approve-all", "--deny-all"]).is_err());
    let cli = Cli::try_parse_from(["agent", "ask", "hi", "--deny-all"]).unwrap();
    assert!(cli.deny_all);
}
