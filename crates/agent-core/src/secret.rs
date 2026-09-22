//! SecretRef：secret 的逻辑引用（§18.1）。
//!
//! 模型只能看见引用（名字），永远拿不到值。值在 ToolRunning 阶段由 broker 解析并
//! 只注入子进程 env（late binding，§18.2）。

use serde::{Deserialize, Serialize};

/// 逻辑引用。形式：`"env:ANTHROPIC_API_KEY"` | `"file:github"`。
/// keychain 属 Phase 2。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SecretRef(pub String);

impl SecretRef {
    /// 解析出 `(kind, name)`，如 `("env", "ANTHROPIC_API_KEY")`。
    pub fn parts(&self) -> (&str, &str) {
        match self.0.split_once(':') {
            Some((k, v)) => (k, v),
            None => ("", &self.0),
        }
    }

    pub fn env(name: impl Into<String>) -> Self {
        Self(format!("env:{}", name.into()))
    }

    pub fn file(name: impl Into<String>) -> Self {
        Self(format!("file:{}", name.into()))
    }
}

impl std::fmt::Display for SecretRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
