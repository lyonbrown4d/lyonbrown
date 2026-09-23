//! 领域 ID 类型。全部是 `Uuid` 的 newtype，serde 透明（序列化为裸 UUID 字符串）。
//!
//! 用 newtype 而非裸 `Uuid` 是为了在签名里区分 session/turn/step 等，避免误传。

use serde::{Deserialize, Serialize};
use uuid::Uuid;

macro_rules! id_type {
  ($name:ident, $doc:expr) => {
    #[doc = $doc]
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
    #[serde(transparent)]
    pub struct $name(pub Uuid);

    impl $name {
      /// 生成新的随机 ID。
      pub fn new() -> Self {
        Self(Uuid::new_v4())
      }

      pub fn as_uuid(&self) -> Uuid {
        self.0
      }
    }

    impl std::fmt::Display for $name {
      fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
      }
    }

    impl Default for $name {
      fn default() -> Self {
        Self::new()
      }
    }
  };
}

id_type!(
  SessionId,
  "一次会话（runtime 拥有的长生命周期上下文）的 ID。"
);
id_type!(TurnId, "一轮用户交互（一条用户消息 → 最终回答）的 ID。");
id_type!(
  StepId,
  "一轮内的一个 step（一次模型调用 + 可能的工具执行）的 ID。"
);
id_type!(
  ToolCallId,
  "一次工具调用的 ID（provider 返回的 tool call id）。"
);
id_type!(
  ObservationId,
  "一次工具观察（回喂模型的 ToolObservation）的 ID。"
);
