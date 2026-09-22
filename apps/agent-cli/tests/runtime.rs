use std::sync::Arc;

use agent_cli::cli::run_kernel_turn;
use agent_kernel::Kernel;
use agent_model::ModelRegistry;
use agent_tool::{AllowAllApproval, ShellTool, ToolRegistry};
use provider_mock::{MockProvider, ScriptedStep};
use serde_json::json;

#[tokio::test]
async fn cli_kernel_path_executes_a_real_shell_tool_and_streams_the_answer() {
    let command = if cfg!(windows) {
        "echo shell-ok"
    } else {
        "printf shell-ok"
    };
    let provider = Arc::new(MockProvider::scripted(vec![
        ScriptedStep::tool_call("shell", json!({"command": command})),
        ScriptedStep::text("finished"),
    ]));
    let registry = ModelRegistry::new();
    registry.register(provider);
    let tools = ToolRegistry::new();
    tools.register(Arc::new(ShellTool::new()));
    let mut kernel = Kernel::builder()
        .providers(registry)
        .tools(tools)
        .approval(AllowAllApproval)
        .workspace_root(std::env::current_dir().unwrap())
        .build();
    let mut output = Vec::new();

    let events = run_kernel_turn(&mut kernel, "run echo", &mut output)
        .await
        .unwrap();

    assert_eq!(String::from_utf8(output).unwrap(), "finished");
    let shell_result = events.iter().find_map(|event| event.tool_result()).unwrap();
    assert!(shell_result.ok);
    assert!(shell_result.output.contains("shell-ok"));
}

#[tokio::test]
async fn headless_stream_neutralizes_terminal_control_sequences() {
    let provider = Arc::new(MockProvider::scripted(vec![ScriptedStep::text(
        "safe\n\t\u{1b}[31mred\u{7}",
    )]));
    let registry = ModelRegistry::new();
    registry.register(provider);
    let mut kernel = Kernel::builder().providers(registry).build();
    let mut output = Vec::new();

    run_kernel_turn(&mut kernel, "show text", &mut output)
        .await
        .unwrap();

    let output = String::from_utf8(output).unwrap();
    assert!(output.contains("safe\n\t"));
    assert!(output.contains("red"));
    assert!(!output.contains('\u{1b}'));
    assert!(!output.contains('\u{7}'));
}
