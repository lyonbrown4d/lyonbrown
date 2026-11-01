use agent_runtime::ToolDefinition;
use serde_json::json;

pub(super) fn definition(name: &str) -> ToolDefinition {
  let path = json!({"type":"string","description":"Workspace-relative path; empty means root for discovery."});
  let max_depth = json!({"type":"integer","minimum":1,"maximum":8,"default":4});
  let max_results = json!({"type":"integer","minimum":1,"maximum":200,"default":200});
  let (description, properties, required) = match name {
    "read_file" => (
      "Read a UTF-8 workspace file (at most 1 MiB). Returns JSON content and SHA-256 revision. Retain revision for write_file.",
      json!({"path":path}),
      json!(["path"]),
    ),
    "write_file" => (
      "Write a UTF-8 workspace file after exact-action human approval. First read relevant repository guidance using read_file or list_files on the parent directory. Requires the revision returned by read_file; empty expected_revision is only for creating an absent file. A revision conflict leaves current content unchanged; read again before proposing another write.",
      json!({"path":path,"content":{"type":"string","maxLength":1048576},"expected_revision":{"type":"string","pattern":"^([0-9a-f]{64})?$"}}),
      json!(["path", "content", "expected_revision"]),
    ),
    "list_files" => (
      "Discover workspace files and directories without following symlinks. Root is depth zero. Reports omissions/truncation explicitly; fixed limits: 2000 visited entries and 256 KiB result. Skips .git, target and node_modules directories.",
      json!({"path":path,"max_depth":max_depth,"max_results":max_results}),
      json!(["path"]),
    ),
    _ => (
      "Find case-sensitive literal text in UTF-8 workspace files, with 1-based line numbers. Reports omissions/truncation; fixed limits: 2000 visited entries, 1 MiB per file, 8 MiB total content, 256 KiB result. Skips symlinks, binary/large files, .git, target and node_modules directories.",
      json!({"path":path,"query":{"type":"string","minLength":1,"maxLength":4096},"max_depth":max_depth,"max_results":max_results}),
      json!(["path", "query"]),
    ),
  };
  ToolDefinition {
    name: name.into(),
    description: description.into(),
    parameters: json!({"type":"object","properties":properties,"required":required,"additionalProperties":false}),
  }
}
