#[test]
fn tool_protocol_accepts_only_declared_schema_valid_calls() {
  let tools = [lookup_tool()];
  let parsed = parse_model_output(
    r#"<tool_call>{"name":"lookup","arguments":{"query":"rust"}}</tool_call>"#,
    &tools,
  )
  .expect("valid call");

  let ParsedModelOutput::ToolCall(call) = parsed else {
    panic!("expected tool call");
  };
  assert_eq!(call.name, "lookup");
  assert_eq!(call.arguments, serde_json::json!({"query": "rust"}));

  assert!(parse_model_output(
    r#"<tool_call>{"name":"missing","arguments":{}}</tool_call>"#,
    &tools,
  )
  .is_err());
  assert!(parse_model_output(
    r#"<tool_call>{"name":"lookup","arguments":{}}</tool_call>"#,
    &tools,
  )
  .is_err());
}

#[test]
fn tool_protocol_rejects_malformed_and_oversized_envelopes() {
  let tools = [lookup_tool()];
  assert!(parse_model_output("before <tool_call>{}</tool_call>", &tools).is_err());
  assert!(parse_model_output("<tool_call>{bad}</tool_call>", &tools).is_err());
  let oversized = format!("<tool_call>{}</tool_call>", "x".repeat(MAX_TOOL_CALL_BYTES + 1));
  assert!(parse_model_output(&oversized, &tools).is_err());
}

#[test]
fn buffered_tool_output_is_bounded_while_tokens_arrive() {
  let mut output = "x".repeat(MAX_BUFFERED_OUTPUT_BYTES);
  append_buffered_output(&mut output, "").unwrap();

  let error = append_buffered_output(&mut output, "x").unwrap_err();
  assert!(error.to_string().contains("buffered tool output exceeds"));
}

#[test]
fn ordinary_model_output_remains_text() {
  assert_eq!(
    parse_model_output("ordinary answer", &[lookup_tool()]).expect("text"),
    ParsedModelOutput::Text("ordinary answer".into())
  );
}

#[test]
fn tool_guidance_is_bounded_and_describes_the_exact_envelope() {
  let guidance = tool_protocol_guidance(&[lookup_tool()]).expect("guidance");
  assert!(guidance.contains("<tool_call>"));
  assert!(guidance.contains(r#""name":"lookup""#));
  assert!(guidance.contains(r#""required":["query"]"#));
  assert!(guidance.len() <= MAX_TOOL_GUIDANCE_BYTES);
}

#[test]
fn tool_requests_add_protocol_guidance_to_the_system_message() {
  let mut model_request = request("local-test");
  model_request.tools.push(lookup_tool());
  let messages = prompt_messages(&model_request).expect("prompt messages");
  assert_eq!(messages[0].role, "system");
  assert!(messages[0].content.contains("Available tools:"));
  assert!(messages[0].content.contains("<tool_call>"));
}

#[test]
fn prior_assistant_tool_call_is_preserved_in_prompt_history() {
  let mut model_request = request("local-test");
  model_request.messages[1].content.clear();
  model_request.messages[1].tool_calls.push(agent_core::ToolInvocation {
    id: ToolCallId::new(),
    name: "lookup".into(),
    arguments: serde_json::json!({"query": "rust"}),
  });
  let messages = prompt_messages(&model_request).expect("prompt messages");
  let ParsedModelOutput::ToolCall(call) =
    parse_model_output(&messages[2].content, &[lookup_tool()]).expect("historical tool call")
  else {
    panic!("expected tool call");
  };
  assert_eq!(call.name, "lookup");
  assert_eq!(call.arguments, serde_json::json!({"query": "rust"}));
}

#[test]
fn qwen3_no_think_directive_is_added_before_template_rendering() {
  let mut messages = vec![PromptMessage::new("user", "fix the bug")];
  apply_qwen3_thinking_control(
    "{% if enable_thinking %}<think>{% endif %}",
    &mut messages,
    Some(false),
  );
  assert_eq!(messages[0].content, "fix the bug /no_think");
}

#[test]
fn thinking_control_does_not_change_other_templates_or_default_requests() {
  let mut other = vec![PromptMessage::new("user", "hello")];
  apply_qwen3_thinking_control("chatml", &mut other, Some(false));
  assert_eq!(other[0].content, "hello");

  let mut default_qwen = vec![PromptMessage::new("user", "hello")];
  apply_qwen3_thinking_control("enable_thinking <think>", &mut default_qwen, None);
  assert_eq!(default_qwen[0].content, "hello");
}

fn lookup_tool() -> ToolDefinition {
  ToolDefinition {
    name: "lookup".into(),
    description: "look up code".into(),
    input_schema: serde_json::json!({
      "type": "object",
      "properties": {"query": {"type": "string"}},
      "required": ["query"],
      "additionalProperties": false
    }),
    source: ToolSource::BuiltIn,
    risk: RiskLevel::Read,
    requires_secrets: Vec::new(),
  }
}
