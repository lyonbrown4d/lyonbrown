use crate::{
  model::App,
  picker::{Picker, PickerItem, PickerValue},
};

pub(crate) fn open(app: &mut App) {
  let Some(session) = app.selected_session().map(|session| session.id.clone()) else {
    return;
  };
  let mut picker = Picker::loading("输入历史 · 键入搜索 · Enter 回填 · Esc 取消");
  picker.loading = false;
  picker.items = app
    .interaction
    .history
    .entries(&session)
    .iter()
    .rev()
    .map(|prompt| PickerItem {
      label: prompt
        .lines()
        .next()
        .unwrap_or_default()
        .chars()
        .take(100)
        .collect(),
      detail: format!("{} 字符", prompt.chars().count()),
      value: PickerValue::History {
        session: session.clone(),
        prompt: prompt.clone(),
      },
    })
    .collect();
  app.open_picker(picker);
}
