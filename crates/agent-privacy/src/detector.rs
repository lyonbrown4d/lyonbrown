use aho_corasick::AhoCorasick;
use regex::Regex;
use secrecy::{ExposeSecret, SecretString};

use crate::PrivacyError;

pub(crate) const MAX_INPUT: usize = 1024 * 1024;
pub(crate) const MAX_MATCHES: usize = 4096;

/// Byte offsets are relative to the original UTF-8 input.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Detection {
  pub start: usize,
  pub end: usize,
  pub kind: Kind,
  pub evidence: Evidence,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Kind {
  KnownSecret,
  PrivateKey,
  Credential,
  Email,
  Phone,
}

/// Classification source only; never contains matched text.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Evidence {
  ExactKnownValue,
  Pattern,
}

/// Detectors may be supplied by the host, including future local adapters.
pub trait Detector: Send + Sync {
  fn detect(&self, text: &str) -> Result<Vec<Detection>, PrivacyError>;
}

pub struct RuleDetector {
  known: Option<AhoCorasick>,
  rules: Vec<(Regex, Kind, usize)>,
}

impl RuleDetector {
  pub fn new(known_secrets: &[SecretString]) -> Result<Self, PrivacyError> {
    let total = known_secrets.iter().try_fold(0usize, |sum, secret| {
      sum.checked_add(secret.expose_secret().len())
    });
    if total.is_none_or(|size| size > MAX_INPUT)
      || known_secrets.len() > 4096
      || known_secrets
        .iter()
        .any(|s| s.expose_secret().is_empty() || s.expose_secret().len() > MAX_INPUT)
    {
      return Err(PrivacyError::Limit);
    }
    let known = if known_secrets.is_empty() {
      None
    } else {
      Some(
        AhoCorasick::new(known_secrets.iter().map(|s| s.expose_secret()))
          .map_err(|_| PrivacyError::DetectorInitialization)?,
      )
    };
    let patterns = [
      (
        r"(?s)-----BEGIN (?:RSA |EC |OPENSSH |DSA |ENCRYPTED )?PRIVATE KEY-----.*?-----END (?:RSA |EC |OPENSSH |DSA |ENCRYPTED )?PRIVATE KEY-----",
        Kind::PrivateKey,
        0,
      ),
      (
        r"\b(?:sk|pk|api|token)[-_][A-Za-z0-9_-]{16,}\b",
        Kind::Credential,
        0,
      ),
      (
        r#"(?i)\b(?:[a-z][a-z0-9]*[_-])*(?:password|passwd|pwd|api[_-]?key|access[_-]?token|token|secret)[\"']?[\t ]*[:=][\t ]*\"((?:\\.|[^\"\\\r\n])+)\""#,
        Kind::Credential,
        1,
      ),
      (
        r#"(?i)\b(?:[a-z][a-z0-9]*[_-])*(?:password|passwd|pwd|api[_-]?key|access[_-]?token|token|secret)[\"']?[\t ]*[:=][\t ]*'((?:\\.|[^'\\\r\n])+)'"#,
        Kind::Credential,
        1,
      ),
      (
        r#"(?i)\b(?:[a-z][a-z0-9]*[_-])*(?:password|passwd|pwd|api[_-]?key|access[_-]?token|token|secret)[\"']?[\t ]*[:=][\t ]*([^\s\"',;}{]+)"#,
        Kind::Credential,
        1,
      ),
      (
        r"\b(?:ghp_|github_pat_)[A-Za-z0-9_]{20,}",
        Kind::Credential,
        0,
      ),
      (r"\bAIza[A-Za-z0-9_-]{35}", Kind::Credential, 0),
      (r"\bxox[a-z]-[A-Za-z0-9-]{10,}", Kind::Credential, 0),
      (r"\b(?:AKIA|ASIA)[A-Z0-9]{16}\b", Kind::Credential, 0),
      (
        r"\beyJ[A-Za-z0-9_-]{5,}\.[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}",
        Kind::Credential,
        0,
      ),
      (
        r"\b[A-Za-z0-9.!#$%&'*+/=?^_`{|}~-]+@[A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)+\b",
        Kind::Email,
        0,
      ),
      (
        r"(?i)\b(?:phone|mobile|tel|电话|手机)\s*[:=：]?\s*(\+?[0-9][0-9 ()-]{6,}[0-9])",
        Kind::Phone,
        1,
      ),
    ];
    let rules = patterns
      .into_iter()
      .map(|(p, k, g)| {
        Regex::new(p)
          .map(|r| (r, k, g))
          .map_err(|_| PrivacyError::DetectorInitialization)
      })
      .collect::<Result<Vec<_>, _>>()?;
    Ok(Self { known, rules })
  }
}

impl Detector for RuleDetector {
  fn detect(&self, text: &str) -> Result<Vec<Detection>, PrivacyError> {
    if text.len() > MAX_INPUT {
      return Err(PrivacyError::Limit);
    }
    let mut result = Vec::new();
    if let Some(known) = &self.known {
      for m in known.find_overlapping_iter(text) {
        result.push(Detection {
          start: m.start(),
          end: m.end(),
          kind: Kind::KnownSecret,
          evidence: Evidence::ExactKnownValue,
        });
        if result.len() > MAX_MATCHES {
          return Err(PrivacyError::Limit);
        }
      }
    }
    for (rule, kind, group) in &self.rules {
      for capture in rule.captures_iter(text) {
        if let Some(m) = capture.get(*group) {
          if is_placeholder(m.as_str()) {
            continue;
          }
          result.push(Detection {
            start: m.start(),
            end: m.end(),
            kind: *kind,
            evidence: Evidence::Pattern,
          });
          if result.len() > MAX_MATCHES {
            return Err(PrivacyError::Limit);
          }
        }
      }
    }
    Ok(result)
  }
}

fn is_placeholder(text: &str) -> bool {
  text
    .strip_prefix("[PRIVATE:")
    .and_then(|value| value.strip_suffix(']'))
    .is_some_and(|id| id.len() == 32 && id.bytes().all(|byte| byte.is_ascii_hexdigit()))
}
