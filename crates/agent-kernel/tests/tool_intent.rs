#[path = "../src/kernel/intent.rs"]
mod intent;

#[test]
fn action_with_a_concrete_target_needs_tools() {
  for prompt in [
    "list files",
    "read the config file",
    "write a file",
    "create src/main.rs",
    "delete the old file",
    "search files for TODO",
    "edit Cargo.toml",
    "run a shell command",
    "execute command cargo fmt",
    "build the project",
    "test this code",
    "inspect the repository",
    "fix this bug",
    "implement feature",
    "repair failing test",
    "add a feature",
    "update code",
    "refactor module",
    "fix this bug without changing the API",
    "read the file; do not edit it",
    "列出文件",
    "读取配置文件",
    "写入文件",
    "创建文件",
    "删除文件",
    "搜索文件",
    "编辑文件",
    "执行命令",
    "运行 shell 命令",
    "构建项目",
    "测试代码",
    "检查仓库",
    "修复这个错误",
    "实现这个功能",
    "实现 M4",
    "修复登录问题",
    "修复登录问题，不要修改数据库",
    "实现功能，无需重构模块",
  ] {
    assert!(
      intent::step_needs_tools(prompt),
      "expected tool intent: {prompt}"
    );
  }
}

#[test]
fn explanation_and_negation_do_not_need_tools() {
  for prompt in [
    "explain unit testing",
    "what is a build",
    "describe the command pattern",
    "explain the directory concept",
    "how do filesystems work",
    "解释单元测试",
    "什么是构建",
    "解释命令模式",
    "目录是什么概念",
    "explain how bug fixes work",
    "what does feature implementation mean",
    "describe a failing test",
    "解释如何修复错误",
    "什么是功能实现",
    "how should I fix this bug?",
    "why should I fix this bug?",
    "don't fix this bug",
    "do not implement this feature",
    "如何修复登录问题",
    "不要实现这个功能",
  ] {
    assert!(
      !intent::step_needs_tools(prompt),
      "unexpected tool intent: {prompt}"
    );
  }
}
