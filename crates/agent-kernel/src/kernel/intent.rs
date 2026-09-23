use std::collections::HashSet;

pub(super) fn step_needs_tools(prompt: &str) -> bool {
  let lower = prompt.to_lowercase();
  let word_sequence: Vec<_> = lower
    .split(|character: char| {
      !character.is_ascii_alphanumeric() && character != '_' && character != '\''
    })
    .filter(|word| !word.is_empty())
    .collect();
  let words: HashSet<_> = word_sequence.iter().copied().collect();
  let file_action = has_unnegated_english_action(
    &word_sequence,
    &[
      "list", "listing", "read", "reading", "write", "writing", "create", "delete", "search",
      "edit", "open", "show",
    ],
  ) && ["file", "files", "directory", "folder", "filesystem"]
    .iter()
    .any(|word| words.contains(word));
  let path_action = has_unnegated_english_action(
    &word_sequence,
    &[
      "read", "write", "create", "delete", "search", "edit", "open",
    ],
  ) && (lower.contains('/')
    || lower.contains(".rs")
    || lower.contains(".toml")
    || lower.contains(".json")
    || lower.contains(".yaml")
    || lower.contains(".yml"));
  let command_action = has_unnegated_english_action(&word_sequence, &["run", "execute"])
    && ["shell", "command", "script", "tests"]
      .iter()
      .any(|word| words.contains(word));
  let project_action = has_unnegated_english_action(&word_sequence, &["build", "test"])
    && ["project", "code", "workspace", "repository"]
      .iter()
      .any(|word| words.contains(word));
  let repository_action = has_unnegated_english_action(&word_sequence, &["inspect", "search"])
    && ["repository", "repo", "workspace"]
      .iter()
      .any(|word| words.contains(word));
  let explanatory = [
    "explain", "describe", "what", "meaning", "concept", "how", "why",
  ]
  .iter()
  .any(|word| words.contains(word));
  let coding_action = has_unnegated_english_action(
    &word_sequence,
    &["fix", "repair", "implement", "add", "update", "refactor"],
  ) && ["bug", "error", "test", "tests", "feature", "code", "module"]
    .iter()
    .any(|word| words.contains(word))
    && !explanatory;
  let chinese_file_action = has_unnegated_chinese_action(
    &lower,
    &[
      "列出", "查看", "读取", "读", "写入", "写", "创建", "删除", "搜索", "编辑",
    ],
  ) && lower.contains("文件");
  let chinese_command_action = has_unnegated_chinese_action(&lower, &["执行", "运行"])
    && (lower.contains("命令") || lower.contains("shell"));
  let chinese_project_action = has_unnegated_chinese_action(&lower, &["构建", "测试"])
    && (lower.contains("项目") || lower.contains("代码"));
  let chinese_repository_action = has_unnegated_chinese_action(&lower, &["检查", "搜索"])
    && (lower.contains("仓库") || lower.contains("代码库"));
  let chinese_explanatory = ["解释", "什么", "如何", "怎么", "为什么"]
    .iter()
    .any(|marker| lower.contains(marker));
  let chinese_coding_action = (has_unnegated_chinese_action(&lower, &["修复", "实现"])
    || (has_unnegated_chinese_action(&lower, &["添加", "更新", "重构"])
      && ["错误", "缺陷", "测试", "功能", "代码", "模块"]
        .iter()
        .any(|object| lower.contains(object))))
    && !chinese_explanatory;
  let actionable = file_action
    || path_action
    || command_action
    || project_action
    || repository_action
    || coding_action
    || chinese_file_action
    || chinese_command_action
    || chinese_project_action
    || chinese_repository_action
    || chinese_coding_action;
  actionable && !explanatory && !chinese_explanatory
}

fn has_unnegated_english_action(words: &[&str], actions: &[&str]) -> bool {
  words.iter().enumerate().any(|(index, word)| {
    let preceding = index
      .checked_sub(1)
      .and_then(|preceding| words.get(preceding))
      .copied();
    actions.contains(word)
      && !matches!(
        preceding,
        Some("don't" | "not" | "never" | "avoid" | "without")
      )
  })
}

fn has_unnegated_chinese_action(text: &str, actions: &[&str]) -> bool {
  actions.iter().any(|action| {
    text.match_indices(action).any(|(index, _)| {
      let prefix = text[..index].trim_end();
      !["不要", "别", "无需", "不必"]
        .iter()
        .any(|negation| prefix.ends_with(negation))
    })
  })
}
