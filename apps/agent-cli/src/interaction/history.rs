use serde::{Deserialize, Deserializer, Serialize};

pub const MAX_HISTORY_ENTRIES: usize = 100;
pub const MAX_PROMPT_BYTES: usize = 1024 * 1024;
pub const MAX_HISTORY_BYTES: usize = MAX_PROMPT_BYTES;
pub const MAX_HISTORY_SESSIONS: usize = 32;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
struct SessionHistory {
  session: String,
  prompts: Vec<String>,
}

/// A bounded cache of acknowledged prompts, never pending drafts or replay instructions.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct InputHistory {
  sessions: Vec<SessionHistory>,
}

impl<'de> Deserialize<'de> for InputHistory {
  fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
    #[derive(Deserialize)]
    struct StoredHistory {
      sessions: Vec<SessionHistory>,
    }
    let stored = StoredHistory::deserialize(deserializer)?;
    let mut history = Self::default();
    for session in stored.sessions {
      for prompt in session.prompts {
        history.record_acknowledged(&session.session, &prompt);
      }
    }
    Ok(history)
  }
}

impl InputHistory {
  pub fn entries(&self, session: &str) -> &[String] {
    self
      .sessions
      .iter()
      .find(|entry| entry.session == session)
      .map(|entry| entry.prompts.as_slice())
      .unwrap_or_default()
  }

  pub fn total_bytes(&self) -> usize {
    self
      .sessions
      .iter()
      .flat_map(|session| &session.prompts)
      .map(String::len)
      .sum()
  }

  /// Call only once the daemon acknowledges the submitted prompt. Oversize text is not truncated.
  pub fn record_acknowledged(&mut self, session: &str, prompt: &str) -> bool {
    if session.is_empty()
      || session.len() > 256
      || prompt.trim().is_empty()
      || prompt.len() > MAX_PROMPT_BYTES
    {
      return false;
    }
    let mut entry = self
      .sessions
      .iter()
      .position(|entry| entry.session == session)
      .map(|index| self.sessions.remove(index))
      .unwrap_or_else(|| SessionHistory {
        session: session.into(),
        prompts: Vec::new(),
      });
    entry.prompts.retain(|existing| existing != prompt);
    entry.prompts.push(prompt.into());
    if entry.prompts.len() > MAX_HISTORY_ENTRIES {
      entry.prompts.remove(0);
    }
    self.sessions.push(entry);
    while self.sessions.len() > MAX_HISTORY_SESSIONS {
      self.sessions.remove(0);
    }
    while self.total_bytes() > MAX_HISTORY_BYTES {
      if let Some(oldest) = self.sessions.first_mut() {
        oldest.prompts.remove(0);
        if oldest.prompts.is_empty() {
          self.sessions.remove(0);
        }
      }
    }
    true
  }

  /// Newest matches first, with original per-session indices for explicit selection.
  pub fn search(&self, session: &str, query: &str) -> Vec<(usize, &str)> {
    let query = query.to_lowercase();
    self
      .entries(session)
      .iter()
      .enumerate()
      .rev()
      .filter(|(_, prompt)| prompt.to_lowercase().contains(&query))
      .map(|(index, prompt)| (index, prompt.as_str()))
      .collect()
  }
}
