use agent_decision::Tool;

#[test]
fn tool_names_preserve_model_tool_contract() {
  for (tool, expected) in [
    (Tool::ReadFile, "read_file"),
    (Tool::WriteFile, "write_file"),
    (Tool::ExecuteCommand, "execute_command"),
    (Tool::BrowserObserve, "browser_observe"),
    (Tool::BrowserAct, "browser_act"),
    (Tool::ComputerObserve, "computer_observe"),
    (Tool::ComputerAct, "computer_act"),
    (Tool::McpCall, "mcp_call"),
    (Tool::DelegateTask, "delegate_task"),
  ] {
    let name: &'static str = tool.name();
    assert_eq!(name, expected);
    assert_eq!(
      serde_json::to_value(tool).expect("serialize tool"),
      expected
    );
    assert_eq!(
      serde_json::from_value::<Tool>(expected.into()).expect("deserialize tool"),
      tool
    );
  }
  assert!(serde_json::from_str::<Tool>("\"unknown_tool\"").is_err());
}
