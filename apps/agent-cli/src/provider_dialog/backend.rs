use super::CredentialChange;
use crate::backend::Update;
use agent_client::AgentClient;
use agent_secrets::{ExposeSecret, SecretString};
use tokio::sync::mpsc;

pub(crate) async fn execute(
  client: &AgentClient,
  provider: String,
  change: CredentialChange,
  generation: u64,
  dialog: String,
  updates: &mpsc::Sender<Update>,
) {
  let result = change_credential(client, &provider, change).await;
  let _ = updates
    .send(Update::ProviderCredential {
      generation,
      dialog,
      result,
    })
    .await;
}
async fn change_credential(
  client: &AgentClient,
  provider: &str,
  change: CredentialChange,
) -> Result<agent_protocol::v1::Provider, String> {
  let definition = agent_config::provider_definition(provider)
    .filter(|entry| entry.runtime_supported && !entry.credential_env_names.is_empty())
    .ok_or("此 Provider 不支持凭据配置。")?;
  let credential = match change {
    CredentialChange::Remove => {
      return client
        .remove_provider_credential(definition.id)
        .await
        .map_err(|_| "移除结果未确认；关闭后重新打开 /providers 核对，不会自动重试。".into());
    }
    CredentialChange::Typed(value) => value,
    CredentialChange::Environment(name) => {
      if !definition.credential_env_names.contains(&name.as_str()) {
        return Err("环境变量不在此 Provider 的允许列表内。".into());
      }
      SecretString::from(std::env::var(name).map_err(|_| "未读取到所选环境变量，请检查启动环境。")?)
    }
  };
  let value = credential.expose_secret();
  if value.trim().is_empty()
    || value.len() > agent_secrets::MAX_SECRET_BYTES
    || value.chars().any(char::is_control)
  {
    return Err("密钥不能为空、包含控制字符或超过 16 KiB。".into());
  }
  client
    .put_provider_credential(definition.id, value.as_bytes().to_vec())
    .await
    .map_err(|_| "保存结果未确认；草稿保留。关闭后重新打开 /providers 核对，不会自动重试。".into())
}
