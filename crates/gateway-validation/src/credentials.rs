use garde::Validate;

const USERNAME_ERROR: &str = "用户名须为 1–64 个 ASCII 字符，仅支持字母、数字及 _ . @ -。";
const PASSWORD_ERROR: &str = "密码须为 15–128 字节。";

/// Untrusted credentials. Never format or serialize this input.
pub struct Credentials<'a> {
  pub username: &'a str,
  pub password: &'a str,
}

/// Safe field messages; never contains the submitted username or password.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CredentialErrors {
  pub username: Option<&'static str>,
  pub password: Option<&'static str>,
}

#[derive(Validate)]
#[garde(transparent)]
struct Username<'a>(
  #[garde(
    ascii,
    length(bytes, min = 1, max = 64),
    pattern(r"\A[A-Za-z0-9_.@-]+\z")
  )]
  &'a str,
);

#[derive(Validate)]
#[garde(transparent)]
struct Password<'a>(#[garde(length(bytes, min = 15, max = 128))] &'a str);

impl Credentials<'_> {
  pub fn validate(&self) -> Result<(), CredentialErrors> {
    let errors = CredentialErrors {
      username: Self::validate_username(self.username)
        .err()
        .and_then(|e| e.username),
      password: Self::validate_password(self.password)
        .err()
        .and_then(|e| e.password),
    };
    if errors.username.is_some() || errors.password.is_some() {
      Err(errors)
    } else {
      Ok(())
    }
  }

  pub fn validate_username(username: &str) -> Result<(), CredentialErrors> {
    Username(username).validate().map_err(|_| CredentialErrors {
      username: Some(USERNAME_ERROR),
      password: None,
    })
  }

  pub fn validate_password(password: &str) -> Result<(), CredentialErrors> {
    Password(password).validate().map_err(|_| CredentialErrors {
      username: None,
      password: Some(PASSWORD_ERROR),
    })
  }
}
