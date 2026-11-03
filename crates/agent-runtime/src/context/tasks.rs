//! User-supplied task text remains authoritative data outside model summaries.
use crate::Result;
use rig_core::{
  conversation::{Conversation, Entry, MessageId},
  message::{Message, UserContent},
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct TaskEntry {
  pub id: Option<MessageId>,
  pub message: Message,
}
pub(super) fn text(message: &Message) -> Option<Message> {
  let Message::User { content } = message else {
    return None;
  };
  if content
    .iter()
    .any(|item| matches!(item, UserContent::ToolResult(_)))
  {
    return None;
  }
  let parts = content
    .iter()
    .filter_map(|part| match part {
      UserContent::Text(text) => Some(UserContent::text(text.text.clone())),
      _ => None,
    })
    .collect::<Vec<_>>();
  (!parts.is_empty()).then_some(Message::User { content: parts })
}
pub(super) fn projection(conversation: &Conversation, tasks: &[TaskEntry]) -> Vec<Message> {
  let mut entries = (0..conversation.messages().len())
    .filter_map(|index| conversation.entry(index))
    .collect::<Vec<_>>();
  let prefix = rig_memory::summarized_prefix(conversation);
  for task in tasks {
    let Some(id) = task.id else {
      continue;
    };
    if entries.iter().any(|entry| entry.id == Some(id)) {
      continue;
    }
    let at = entries
      .iter()
      .enumerate()
      .skip(prefix)
      .find_map(|(at, entry)| entry.id.filter(|entry_id| *entry_id > id).map(|_| at))
      .unwrap_or(entries.len());
    let mut entry = Entry::new(task.message.clone());
    entry.id = Some(id);
    entries.insert(at, entry);
  }
  entries
    .into_iter()
    .map(|entry| (*entry.message).clone())
    .collect()
}
pub(super) fn summarizable(
  conversation: &Conversation,
  tasks: &[TaskEntry],
) -> Result<Conversation> {
  let mut candidate = conversation.clone();
  let last = conversation.id(conversation.messages().len().saturating_sub(1));
  let entries = (0..conversation.messages().len())
    .filter_map(|index| conversation.entry(index))
    .filter(|entry| entry.id == last || !tasks.iter().any(|task| task.id == entry.id))
    .collect();
  candidate.set_entries(entries, rig_memory::summary_author());
  candidate
    .validate()
    .map_err(crate::persistence::rig_error)?;
  Ok(candidate)
}
