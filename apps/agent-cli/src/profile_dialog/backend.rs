use super::{ProfileOperation, ProfileResult};
use agent_client::AgentClient;
use agent_protocol::v1::{ProbeProviderProfileRequest, PutProviderProfileRequest};
use agent_secrets::ExposeSecret;

fn failure(error: anyhow::Error) -> String {
  let status = error
    .downcast_ref::<tonic::Status>()
    .map(tonic::Status::code);
  match status {
    Some(tonic::Code::InvalidArgument) => "配置校验失败，请检查名称、协议、地址、模型与凭据。",
    Some(tonic::Code::Aborted | tonic::Code::AlreadyExists) => {
      "配置版本冲突，请重新打开后核对并编辑。"
    }
    Some(tonic::Code::NotFound) => "配置已不存在，请关闭并重新打开列表。",
    Some(tonic::Code::ResourceExhausted) => "已达到配置容量或请求资源限制。",
    Some(tonic::Code::PermissionDenied | tonic::Code::Unauthenticated) => {
      "当前连接未获授权，请检查连接身份。"
    }
    Some(tonic::Code::DeadlineExceeded) => "请求超时，结果未确认；请重新打开核对。",
    _ => "操作结果未确认，请重新打开配置核对。",
  }
  .into()
}

pub(crate) async fn execute(
  client: &AgentClient,
  operation: ProfileOperation,
) -> Result<ProfileResult, String> {
  match operation {
    ProfileOperation::Save {
      profile,
      credential,
    } => client
      .put_provider_profile(PutProviderProfileRequest {
        profile: Some(profile),
        credential: credential.expose_secret().as_bytes().to_vec(),
      })
      .await
      .map(ProfileResult::Saved)
      .map_err(failure),
    ProfileOperation::Probe {
      profile,
      credential,
    } => client
      .probe_provider_profile(ProbeProviderProfileRequest {
        profile: Some(profile),
        credential: credential.expose_secret().as_bytes().to_vec(),
      })
      .await
      .map(ProfileResult::Probed)
      .map_err(failure),
    ProfileOperation::Remove { id, revision } => client
      .remove_provider_profile(&id, revision)
      .await
      .map(|()| ProfileResult::Removed)
      .map_err(failure),
  }
}
