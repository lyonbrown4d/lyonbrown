use std::collections::HashSet;
use std::sync::{Arc, RwLock};

#[derive(Clone, Default)]
pub(crate) struct ResolvedSecrets {
    values: Arc<RwLock<HashSet<String>>>,
}

impl ResolvedSecrets {
    pub(crate) fn register(&self, value: &str) {
        if value.is_empty() {
            return;
        }
        self.values
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(value.to_owned());
    }

    pub(crate) fn redact(&self, text: &str) -> String {
        let mut values = self
            .values
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .cloned()
            .collect::<Vec<_>>();
        values.sort_by(|left, right| right.len().cmp(&left.len()).then_with(|| left.cmp(right)));
        let mut redacted = String::with_capacity(text.len());
        let mut offset = 0;
        while offset < text.len() {
            if let Some(value) = values
                .iter()
                .find(|value| text[offset..].starts_with(value.as_str()))
            {
                redacted.push_str("[REDACTED]");
                offset += value.len();
            } else {
                let character = text[offset..]
                    .chars()
                    .next()
                    .expect("offset is before the end of text");
                redacted.push(character);
                offset += character.len_utf8();
            }
        }
        redacted
    }
}
