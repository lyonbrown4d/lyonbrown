use garde::Validate;

pub const MAX_POLICY_DOCUMENT_BYTES: usize = 1_048_576;
const REVISION_ERROR: &str =
  "新版本名称须为 1–128 字节，不能为空白或包含控制字符，且须与当前版本不同。";
const DOCUMENT_ERROR: &str = "策略内容须为不超过 1 MiB 的有效 JSON 对象。";

/// Untrusted policy form. Semantic policy validation and compare-and-swap are caller concerns.
pub struct PolicyDraft<'a> {
  pub expected_revision: &'a str,
  pub revision: &'a str,
  pub document: &'a str,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PolicyDraftErrors {
  pub revision: Option<&'static str>,
  pub document: Option<&'static str>,
}
impl PolicyDraftErrors {
  pub fn invalid(self) -> bool {
    self.revision.is_some() || self.document.is_some()
  }
}

#[derive(Validate)]
#[garde(transparent)]
struct ExpectedRevision<'a>(#[garde(length(bytes, min = 1, max = 128))] &'a str);

#[derive(Validate)]
struct Revision<'a> {
  #[garde(skip)]
  expected: &'a str,
  #[garde(length(bytes, min = 1, max = 128), custom(clean_revision), custom(different(self.expected)))]
  value: &'a str,
}

#[derive(Validate)]
#[garde(transparent)]
struct Document<'a>(
  #[garde(length(bytes, max = MAX_POLICY_DOCUMENT_BYTES), custom(json_object))] &'a str,
);

impl PolicyDraft<'_> {
  pub fn validate(&self) -> Result<(), PolicyDraftErrors> {
    let revision = Revision {
      expected: self.expected_revision,
      value: self.revision,
    };
    let errors = PolicyDraftErrors {
      revision: if ExpectedRevision(self.expected_revision).validate().is_err()
        || revision.validate().is_err()
      {
        Some(REVISION_ERROR)
      } else {
        None
      },
      document: Document(self.document)
        .validate()
        .err()
        .map(|_| DOCUMENT_ERROR),
    };
    if errors.invalid() {
      Err(errors)
    } else {
      Ok(())
    }
  }
}

fn clean_revision(value: &str, _: &()) -> garde::Result {
  if value.trim().is_empty() || value.chars().any(char::is_control) {
    Err(garde::Error::new("invalid revision"))
  } else {
    Ok(())
  }
}
fn different(expected: &str) -> impl FnOnce(&str, &()) -> garde::Result + '_ {
  move |value, _| {
    if value == expected {
      Err(garde::Error::new("revision unchanged"))
    } else {
      Ok(())
    }
  }
}
fn json_object(value: &str, _: &()) -> garde::Result {
  // Garde evaluates every rule. Bound parsing independently of the length rule's result.
  if value.len() > MAX_POLICY_DOCUMENT_BYTES {
    return Err(garde::Error::new("document too large"));
  }
  match serde_json::from_str::<serde_json::Value>(value) {
    Ok(serde_json::Value::Object(_)) => Ok(()),
    _ => Err(garde::Error::new("invalid JSON object")),
  }
}
