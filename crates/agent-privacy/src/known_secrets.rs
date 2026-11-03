use aho_corasick::AhoCorasick;
use secrecy::{ExposeSecret, SecretString};

use crate::{
  Detection, Evidence, Kind, PrivacyError,
  detector::{MAX_INPUT, MAX_MATCHES},
  placeholder::inside_placeholder,
  service::Mapping,
};

/// Dynamic host credentials. No scope mappings are created by registration.
#[derive(Default)]
pub(crate) struct KnownSecrets {
  values: Vec<SecretString>,
  bytes: usize,
  matcher: Option<AhoCorasick>,
}

impl KnownSecrets {
  pub fn register(&mut self, secret: &SecretString) -> Result<(), PrivacyError> {
    let value = secret.expose_secret();
    if value.is_empty() {
      return Err(PrivacyError::Limit);
    }
    if self.values.iter().any(|item| item.expose_secret() == value) {
      return Ok(());
    }
    let total = self
      .bytes
      .checked_add(value.len())
      .ok_or(PrivacyError::Limit)?;
    if self.values.len() >= MAX_MATCHES || total > MAX_INPUT {
      return Err(PrivacyError::Limit);
    }
    // Build before mutation so a failure preserves both the old matcher and values.
    let matcher = AhoCorasick::new(
      self
        .values
        .iter()
        .map(|item| item.expose_secret())
        .chain(std::iter::once(value)),
    )
    .map_err(|_| PrivacyError::DetectorInitialization)?;
    self.values.push(secret.clone());
    self.bytes = total;
    self.matcher = Some(matcher);
    Ok(())
  }

  pub fn append_matches(
    &self,
    text: &str,
    mappings: Option<&[Mapping]>,
    spans: &mut Vec<Detection>,
  ) -> Result<(), PrivacyError> {
    if let Some(matcher) = &self.matcher {
      for hit in matcher.find_overlapping_iter(text) {
        if !inside_placeholder(text, hit.start(), hit.end(), mappings) {
          spans.push(Detection {
            start: hit.start(),
            end: hit.end(),
            kind: Kind::KnownSecret,
            evidence: Evidence::ExactKnownValue,
          });
          if spans.len() > MAX_MATCHES {
            return Err(PrivacyError::Limit);
          }
        }
      }
    }
    Ok(())
  }
}
