use super::BrowserTool;
use agent_browser::{Action, Command};
use anyhow::{Result, ensure};
use serde_json::Value;
use sha2::{Digest, Sha256};

pub(super) enum Operation {
  Act(Action),
  OpenSpace {
    id: String,
    host: String,
    profile: String,
    headless: bool,
  },
  OpenTab {
    id: String,
    space: String,
    url: String,
  },
  CloseSpace {
    id: String,
    space: String,
  },
}
impl Operation {
  pub fn parse(args: &Value, run: &str, call: &str) -> Result<Self> {
    keys(args, &["call"])?;
    ensure!(
      !call.is_empty() && call.len() <= 4096,
      "invalid browser tool call identifier"
    );
    let id = action_id(run, call);
    let input = args
      .get("call")
      .ok_or_else(|| anyhow::anyhow!("call is required"))?;
    let operation = text(input, "op")?;
    match operation.as_str() {
      "click" | "fill" | "press" | "navigate" => {
        keys(
          input,
          &[
            "op",
            "tab_id",
            "snapshot_id",
            "revision",
            "element_ref",
            "text",
            "timeout_ms",
          ],
        )?;
        let revision = input
          .get("revision")
          .and_then(Value::as_u64)
          .ok_or_else(|| anyhow::anyhow!("revision must be an unsigned integer"))?;
        let timeout = input
          .get("timeout_ms")
          .map_or(Some(15_000), Value::as_u64)
          .and_then(|value| u32::try_from(value).ok())
          .ok_or_else(|| anyhow::anyhow!("invalid action timeout"))?;
        let element_ref = optional_text(input, "element_ref")?.unwrap_or_default();
        ensure!(
          operation == "navigate" || !element_ref.is_empty(),
          "element_ref is required"
        );
        let action = Action {
          id,
          tab_id: text(input, "tab_id")?,
          snapshot_id: text(input, "snapshot_id")?,
          revision,
          kind: operation,
          element_ref,
          text: optional_text(input, "text")?.unwrap_or_default(),
          timeout_ms: timeout,
        };
        agent_browser::validate_action(&action)?;
        Ok(Self::Act(action))
      }
      "open_space" => {
        keys(input, &["op", "host_id", "profile_id", "headless"])?;
        let profile = text(input, "profile_id")?;
        ensure!(
          profile.len() <= 96
            && profile
              .bytes()
              .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_')),
          "invalid profile identifier"
        );
        Ok(Self::OpenSpace {
          id,
          host: optional_text(input, "host_id")?.unwrap_or_default(),
          profile,
          headless: boolean(input, "headless", false)?,
        })
      }
      "open_tab" => {
        keys(input, &["op", "space_id", "url"])?;
        let url = text(input, "url")?;
        agent_browser::validate_url(&url)?;
        Ok(Self::OpenTab {
          id,
          space: text(input, "space_id")?,
          url,
        })
      }
      "close_space" => {
        keys(input, &["op", "space_id"])?;
        Ok(Self::CloseSpace {
          id,
          space: text(input, "space_id")?,
        })
      }
      _ => anyhow::bail!("unsupported browser operation"),
    }
  }
  pub fn id(&self) -> &str {
    match self {
      Self::Act(action) => &action.id,
      Self::OpenSpace { id, .. } | Self::OpenTab { id, .. } | Self::CloseSpace { id, .. } => id,
    }
  }
  pub async fn host(&self, tool: &BrowserTool) -> Result<String> {
    match self {
      Self::Act(action) => tool.tab(&action.tab_id, true).await,
      Self::OpenSpace { host, .. } => Ok(tool.daemon.browser.host(host).await?),
      Self::OpenTab { space, .. } | Self::CloseSpace { space, .. } => tool.space(space).await,
    }
  }
  pub fn command(&self, session: &str) -> Command {
    match self {
      Self::Act(action) => Command::Act(action.clone()),
      Self::OpenSpace {
        profile, headless, ..
      } => Command::OpenSpace {
        session_id: session.into(),
        profile_id: profile.clone(),
        headless: *headless,
      },
      Self::OpenTab { space, url, .. } => Command::OpenTab {
        space_id: space.clone(),
        url: url.clone(),
      },
      Self::CloseSpace { space, .. } => Command::CloseSpace {
        space_id: space.clone(),
      },
    }
  }
}
pub(super) fn action_id(run: &str, call: &str) -> String {
  let mut digest = Sha256::new();
  digest.update(run.as_bytes());
  digest.update([0]);
  digest.update(call.as_bytes());
  format!("agent-browser-{}", hex::encode(digest.finalize()))
}
pub(super) fn keys(value: &Value, allowed: &[&str]) -> Result<()> {
  let object = value
    .as_object()
    .ok_or_else(|| anyhow::anyhow!("arguments must be an object"))?;
  ensure!(
    object.keys().all(|key| allowed.contains(&key.as_str())),
    "unexpected browser argument"
  );
  Ok(())
}
pub(super) fn text(value: &Value, name: &str) -> Result<String> {
  optional_text(value, name)?
    .filter(|text| !text.is_empty())
    .ok_or_else(|| anyhow::anyhow!("{name} is required"))
}
pub(super) fn optional_text(value: &Value, name: &str) -> Result<Option<String>> {
  value
    .get(name)
    .map(|value| {
      let text = value
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("{name} must be a string"))?;
      ensure!(
        text.len() <= 64 * 1024,
        "{name} exceeds browser argument limit"
      );
      Ok(text.into())
    })
    .transpose()
}
pub(super) fn boolean(value: &Value, name: &str, default: bool) -> Result<bool> {
  value.get(name).map_or(Ok(default), |value| {
    value
      .as_bool()
      .ok_or_else(|| anyhow::anyhow!("{name} must be a boolean"))
  })
}
