use std::collections::HashMap;

use aho_corasick::AhoCorasick;
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};

use crate::{
  Detection, Detector, Evidence, Kind, PrivacyError, RuleDetector,
  detector::{MAX_INPUT, MAX_MATCHES},
  known_secrets::KnownSecrets,
  placeholder::{inside_placeholder, is_placeholder},
};

pub(crate) struct Mapping {
  pub(crate) placeholder: String,
  original: SecretString,
}

#[derive(Serialize)]
struct ScopeExport<'a> {
  scope: &'a str,
  mappings: Vec<MappingExport<'a>>,
}
#[derive(Serialize)]
struct MappingExport<'a> {
  placeholder: &'a str,
  original: &'a str,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ScopeImport {
  scope: String,
  mappings: Vec<MappingImport>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MappingImport {
  placeholder: String,
  original: SecretString,
}

/// This service holds sensitive mappings in memory. It neither grants nor checks authority.
pub struct PrivacyService {
  detector: Box<dyn Detector>,
  known: KnownSecrets,
  scopes: HashMap<String, Vec<Mapping>>,
}

impl PrivacyService {
  pub fn new(known_secrets: &[SecretString]) -> Result<Self, PrivacyError> {
    Ok(Self::with_detector(Box::new(RuleDetector::new(
      known_secrets,
    )?)))
  }

  pub fn with_detector(detector: Box<dyn Detector>) -> Self {
    Self {
      detector,
      known: KnownSecrets::default(),
      scopes: HashMap::new(),
    }
  }

  /// Add an exact host-known secret to all scopes, without creating mappings.
  /// Duplicate values are ignored; rejected updates leave existing detection intact.
  /// Registrations are memory-only and the host must restore them after restart.
  pub fn register_known_secret(&mut self, secret: &SecretString) -> Result<(), PrivacyError> {
    self.known.register(secret)
  }

  /// Transform only content explicitly selected by the host. Never restores response text.
  pub fn protect_text(&mut self, scope: &str, text: &str) -> Result<String, PrivacyError> {
    if scope.is_empty() || scope.len() > 1024 || text.len() > MAX_INPUT {
      return Err(PrivacyError::Limit);
    }
    let mut spans = self.detector.detect(text)?;
    self
      .known
      .append_matches(text, self.scopes.get(scope).map(Vec::as_slice), &mut spans)?;
    if let Some(mappings) = self.scopes.get(scope)
      && !mappings.is_empty()
    {
      let matcher = AhoCorasick::new(mappings.iter().map(|m| m.original.expose_secret()))
        .map_err(|_| PrivacyError::DetectorInitialization)?;
      for hit in matcher.find_overlapping_iter(text) {
        if !inside_placeholder(text, hit.start(), hit.end(), Some(mappings.as_slice())) {
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
    if spans.len() > MAX_MATCHES {
      return Err(PrivacyError::Limit);
    }
    for Detection { start, end, .. } in &spans {
      if start >= end
        || *end > text.len()
        || !text.is_char_boundary(*start)
        || !text.is_char_boundary(*end)
      {
        return Err(PrivacyError::InvalidSpan);
      }
    }
    spans.retain(|span| {
      !inside_placeholder(
        text,
        span.start,
        span.end,
        self.scopes.get(scope).map(Vec::as_slice),
      )
    });
    spans.sort_unstable_by_key(|span| (span.start, span.end));
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for span in spans {
      if let Some(last) = merged.last_mut()
        && span.start < last.1
      {
        last.1 = last.1.max(span.end);
        continue;
      }
      merged.push((span.start, span.end));
    }
    let mut result = String::with_capacity(text.len());
    let mut cursor = 0;
    for (start, end) in merged {
      result.push_str(&text[cursor..start]);
      result.push_str(self.placeholder(scope, &text[start..end])?);
      cursor = end;
    }
    result.push_str(&text[cursor..]);
    Ok(result)
  }

  fn placeholder(&mut self, scope: &str, original: &str) -> Result<&str, PrivacyError> {
    if !self.scopes.contains_key(scope) && self.scopes.len() >= MAX_MATCHES {
      return Err(PrivacyError::Limit);
    }
    let mappings = self.scopes.entry(scope.to_owned()).or_default();
    if let Some(index) = mappings
      .iter()
      .position(|m| m.original.expose_secret() == original)
    {
      return Ok(&mappings[index].placeholder);
    }
    let total = mappings.iter().try_fold(original.len(), |sum, m| {
      sum.checked_add(m.original.expose_secret().len())
    });
    if mappings.len() >= MAX_MATCHES || total.is_none_or(|size| size > MAX_INPUT) {
      return Err(PrivacyError::Limit);
    }
    let placeholder = loop {
      let mut bytes = [0u8; 16];
      getrandom::fill(&mut bytes).map_err(|_| PrivacyError::Random)?;
      let token = bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
      let candidate = format!("[PRIVATE:{token}]");
      if !mappings.iter().any(|m| m.placeholder == candidate) {
        break candidate;
      }
    };
    mappings.push(Mapping {
      placeholder,
      original: SecretString::from(original.to_owned()),
    });
    Ok(
      &mappings
        .last()
        .ok_or(PrivacyError::MissingMapping)?
        .placeholder,
    )
  }

  /// The host must authorize the sink independently before calling this method.
  /// Only an exact placeholder in the same scope is accepted.
  pub fn resolve(&self, scope: &str, placeholder: &str) -> Result<&str, PrivacyError> {
    self
      .scopes
      .get(scope)
      .and_then(|mappings| mappings.iter().find(|m| m.placeholder == placeholder))
      .map(|m| m.original.expose_secret())
      .ok_or(PrivacyError::MissingMapping)
  }

  /// Register a host-supplied secret for subsequent exact matching in this scope.
  pub fn register_secret(&mut self, scope: &str, secret: &str) -> Result<(), PrivacyError> {
    if scope.is_empty() || scope.len() > 1024 || secret.is_empty() || secret.len() > MAX_INPUT {
      return Err(PrivacyError::Limit);
    }
    self.placeholder(scope, secret)?;
    Ok(())
  }

  /// Export only for a host-controlled encrypted persistence boundary.
  pub fn export_scope(&self, scope: &str) -> Result<SecretString, PrivacyError> {
    if scope.is_empty() || scope.len() > 1024 {
      return Err(PrivacyError::Limit);
    }
    let mappings = self
      .scopes
      .get(scope)
      .into_iter()
      .flatten()
      .map(|m| MappingExport {
        placeholder: &m.placeholder,
        original: m.original.expose_secret(),
      })
      .collect();
    let value = serde_json::to_string(&ScopeExport { scope, mappings })
      .map_err(|_| PrivacyError::InvalidMapping)?;
    Ok(SecretString::from(value))
  }

  /// Import only after authenticating and decrypting the host's persisted scope.
  pub fn import_scope(
    &mut self,
    scope: &str,
    serialized: &SecretString,
  ) -> Result<(), PrivacyError> {
    if scope.is_empty() || scope.len() > 1024 || serialized.expose_secret().len() > 8 * MAX_INPUT {
      return Err(PrivacyError::Limit);
    }
    let data: ScopeImport =
      serde_json::from_str(serialized.expose_secret()).map_err(|_| PrivacyError::InvalidMapping)?;
    if data.scope != scope || data.mappings.len() > MAX_MATCHES {
      return Err(PrivacyError::InvalidMapping);
    }
    let existing = self.scopes.get(scope);
    let mut additions = Vec::new();
    for item in data.mappings {
      if !is_placeholder(&item.placeholder) || item.original.expose_secret().is_empty() {
        return Err(PrivacyError::InvalidMapping);
      }
      if let Some(found) = existing
        .into_iter()
        .flatten()
        .chain(additions.iter())
        .find(|m: &&Mapping| m.placeholder == item.placeholder)
      {
        if found.original.expose_secret() != item.original.expose_secret() {
          return Err(PrivacyError::InvalidMapping);
        }
        continue;
      }
      additions.push(Mapping {
        placeholder: item.placeholder,
        original: item.original,
      });
    }
    let count = existing.map_or(0, Vec::len).saturating_add(additions.len());
    let total = existing
      .into_iter()
      .flatten()
      .chain(additions.iter())
      .try_fold(0usize, |sum, m| {
        sum.checked_add(m.original.expose_secret().len())
      });
    if count > MAX_MATCHES
      || total.is_none_or(|size| size > MAX_INPUT)
      || (existing.is_none() && !additions.is_empty() && self.scopes.len() >= MAX_MATCHES)
    {
      return Err(PrivacyError::Limit);
    }
    if !additions.is_empty() {
      self
        .scopes
        .entry(scope.to_owned())
        .or_default()
        .extend(additions);
    }
    Ok(())
  }

  /// Transfer host-owned mappings into a shared service without
  /// changing existing references. Source and target must be separately locked.
  pub fn transfer_scope(&mut self, target: &mut Self, scope: &str) -> Result<(), PrivacyError> {
    let saved = self.export_scope(scope)?;
    target.import_scope(scope, &saved)?;
    self.scopes.remove(scope);
    Ok(())
  }

  /// Explicitly discard a session/caller's mappings.
  pub fn forget_scope(&mut self, scope: &str) {
    self.scopes.remove(scope);
  }
}
