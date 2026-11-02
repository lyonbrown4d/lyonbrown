use crate::model::{App, ContextPanel};
pub(crate) fn run(app: &mut App, prompt: &str) {
  let words = match shell_words::split(prompt) {
    Ok(words) => words,
    Err(error) => {
      app.draft_detail = format!("Invalid draft command quoting: {error}");
      app.router.navigate(ContextPanel::Drafts);
      return;
    }
  };
  match words.as_slice() {
    [_, command] if command == "list" => {
      app.draft_detail = if app.drafts.is_empty() && app.unbound_draft.is_empty() {
        "没有保存的草稿。".into()
      } else {
        app
          .drafts
          .iter()
          .map(|(session, draft)| {
            let known = app.sessions.iter().any(|value| value.id == *session);
            format!(
              "{} {session} · {} 字符 · /draft recover {session}",
              if known {
                "会话草稿"
              } else {
                "原会话当前不可用"
              },
              draft.chars().count()
            )
          })
          .chain((!app.unbound_draft.is_empty()).then(|| {
            format!(
              "未绑定草稿 · {} 字符 · /draft recover unbound",
              app.unbound_draft.chars().count()
            )
          }))
          .collect::<Vec<_>>()
          .join("\n")
      };
      app.input.clear();
      app.recovered_draft_from = None;
    }
    [_, command, session] if command == "recover" => {
      let draft = if session == "unbound" {
        (!app.unbound_draft.is_empty()).then(|| app.unbound_draft.clone())
      } else {
        app.drafts.get(session).cloned()
      };
      if let Some(draft) = draft {
        app.input = crate::composer::Composer::from(draft.as_str());
        app.recovered_draft_from = Some(session.clone());
        app.draft_detail =
          format!("已从会话 {session} 恢复草稿到编辑器。检查内容和当前会话后按 Enter 才会提交。");
      } else {
        app.draft_detail = format!("找不到会话 {session} 的保存草稿。没有发送任何内容。");
      }
    }
    _ => {
      app.draft_detail = "用法：/draft list · /draft recover SESSION_ID".into();
    }
  }
  app.router.navigate(ContextPanel::Drafts);
}
