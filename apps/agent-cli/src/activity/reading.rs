use super::values::safe;

/// A logical text position; unaffected by terminal width and line wrapping.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageAnchor {
  pub message_id: String,
  pub line: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchHit {
  pub anchor: MessageAnchor,
  /// Character offsets within a logical line, rather than UTF-8 byte offsets.
  pub start: usize,
  pub end: usize,
}

/// Presentation-only search. Callers supply stable message identities from
/// their history adapter; no server mutation or model request is performed.
#[derive(Debug, Default, Clone)]
pub struct ReadingState {
  messages: Vec<(String, String)>,
  query: String,
  hits: Vec<SearchHit>,
  selected: usize,
}

impl ReadingState {
  pub fn sync<'a>(&mut self, messages: impl IntoIterator<Item = (&'a str, &'a str)>) {
    self.messages = messages
      .into_iter()
      .map(|(id, content)| (id.into(), safe(content)))
      .collect();
    self.search();
  }

  pub fn set_query(&mut self, query: &str) {
    self.query = safe(query);
    self.selected = 0;
    self.search();
  }

  pub fn query(&self) -> &str {
    &self.query
  }

  pub fn anchors(&self) -> impl Iterator<Item = MessageAnchor> + '_ {
    self.messages.iter().map(|(id, _)| MessageAnchor {
      message_id: id.clone(),
      line: 0,
    })
  }

  pub fn message_index(&self, anchor: &MessageAnchor) -> Option<usize> {
    self
      .messages
      .iter()
      .position(|(id, _)| id == &anchor.message_id)
  }

  pub fn hits(&self) -> &[SearchHit] {
    &self.hits
  }

  pub fn current(&self) -> Option<&SearchHit> {
    self.hits.get(self.selected)
  }

  pub fn next_hit(&mut self) -> Option<&SearchHit> {
    if !self.hits.is_empty() {
      self.selected = (self.selected + 1) % self.hits.len();
    }
    self.current()
  }

  pub fn previous_hit(&mut self) -> Option<&SearchHit> {
    if !self.hits.is_empty() {
      self.selected = (self.selected + self.hits.len() - 1) % self.hits.len();
    }
    self.current()
  }

  pub fn position(&self) -> Option<(usize, usize)> {
    self.current().map(|_| (self.selected + 1, self.hits.len()))
  }

  fn search(&mut self) {
    let previous = self.current().cloned();
    self.hits.clear();
    if !self.query.is_empty() {
      // Exact Unicode matching avoids changing character offsets during case
      // folding (for example İ expands to multiple Unicode scalars).
      for (id, content) in &self.messages {
        for (line, text) in content.lines().enumerate() {
          for (byte, matched) in text.match_indices(self.query.as_str()) {
            let start = text[..byte].chars().count();
            self.hits.push(SearchHit {
              anchor: MessageAnchor {
                message_id: id.clone(),
                line,
              },
              start,
              end: start + matched.chars().count(),
            });
          }
        }
      }
    }
    self.selected = previous
      .and_then(|previous| self.hits.iter().position(|hit| hit == &previous))
      .unwrap_or(0);
  }
}
