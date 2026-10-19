use gateway_validation::{Credentials, MAX_POLICY_DOCUMENT_BYTES, PolicyDraft};

#[test]
fn credentials_keep_ascii_username_and_utf8_byte_boundaries() {
  for username in ["a", "Admin_1.user@local-test", &"a".repeat(64)] {
    assert!(Credentials::validate_username(username).is_ok());
  }
  for username in ["", "汉", "admin\n", " a", "a/b", &"a".repeat(65)] {
    assert!(Credentials::validate_username(username).is_err());
  }
  for password in [
    "五个汉字哦",
    &"x".repeat(15),
    &"x".repeat(128),
    "               ",
  ] {
    assert!(Credentials::validate_password(password).is_ok());
  }
  for password in ["", "四个汉字", &"x".repeat(14), &"x".repeat(129)] {
    assert!(Credentials::validate_password(password).is_err());
  }
}

#[test]
fn credential_errors_only_expose_fixed_field_messages() {
  let errors = Credentials {
    username: "secret-user/marker",
    password: "secret-marker",
  }
  .validate()
  .expect_err("invalid fixture");
  assert!(errors.username.is_some());
  assert!(errors.password.is_some());
  let display = format!("{errors:?}");
  assert!(!display.contains("secret-user"));
  assert!(!display.contains("secret-marker"));
  assert!(
    Credentials {
      username: "super_admin",
      password: "synthetic-password"
    }
    .validate()
    .is_ok()
  );
}

#[test]
fn policy_revision_rejects_blank_control_unchanged_and_oversized_names() {
  for revision in ["new", "新版本", &"r".repeat(128)] {
    assert!(draft("old", revision, "{}").validate().is_ok());
  }
  for revision in [
    "",
    "  \u{2003}",
    "old",
    "new\n",
    "new\u{85}",
    &"r".repeat(129),
  ] {
    let error = draft("old", revision, "{}")
      .validate()
      .expect_err("invalid revision");
    assert!(error.revision.is_some());
    assert!(error.document.is_none());
  }
  // The expected revision is a CAS identifier: preserve its existing byte-only contract.
  for expected in [" ", "\n"] {
    assert!(draft(expected, "new", "{}").validate().is_ok());
  }
  assert!(draft("", "new", "{}").validate().is_err());
  assert!(draft(&"e".repeat(129), "new", "{}").validate().is_err());
}

#[test]
fn policy_document_accepts_only_bounded_json_objects_without_echoing_content() {
  let exact = format!(
    "{{\"value\":\"{}\"}}",
    "x".repeat(MAX_POLICY_DOCUMENT_BYTES - 12)
  );
  assert_eq!(exact.len(), MAX_POLICY_DOCUMENT_BYTES);
  assert!(draft("old", "new", &exact).validate().is_ok());
  let oversized = format!("{exact} ");
  for document in ["[]", "null", "\"text\"", "{secret-marker}", &oversized] {
    let error = draft("old", "new", document)
      .validate()
      .expect_err("invalid document");
    assert!(error.document.is_some());
    assert!(error.revision.is_none());
    assert!(!format!("{error:?}").contains("secret-marker"));
  }
  // Input validation does not strip markup from otherwise valid JSON; output encoding owns XSS.
  assert!(
    draft(
      "old",
      "new",
      r#"{"label":"</textarea><script>synthetic</script>"}"#
    )
    .validate()
    .is_ok()
  );
}

fn draft<'a>(expected_revision: &'a str, revision: &'a str, document: &'a str) -> PolicyDraft<'a> {
  PolicyDraft {
    expected_revision,
    revision,
    document,
  }
}
