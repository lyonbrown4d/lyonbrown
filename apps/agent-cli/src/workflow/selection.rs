#[derive(Debug, Default)]
pub(super) struct Selection {
  pub id: Option<String>,
}

impl Selection {
  pub fn replace(&mut self, old_empty: bool, ids: &[&str]) {
    if let Some(id) = &self.id {
      if !ids.contains(&id.as_str()) {
        // Never silently move an approval decision to another request after a refresh.
        self.id = None;
      }
    } else if old_empty {
      self.id = ids.first().map(|id| (*id).to_owned());
    }
  }

  pub fn step(&mut self, ids: &[&str], forward: bool) {
    let selected = self
      .id
      .as_ref()
      .and_then(|id| ids.iter().position(|item| *item == id));
    let index = match selected {
      Some(index) if forward => (index + 1).min(ids.len().saturating_sub(1)),
      Some(index) => index.saturating_sub(1),
      None if forward => 0,
      None => ids.len().saturating_sub(1),
    };
    self.id = ids.get(index).map(|id| (*id).to_owned());
  }
}
