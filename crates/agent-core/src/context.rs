//! 上下文条目：working memory 的基本单元（§12/§13）。
//!
//! `ContextItem` 是 Session working memory 里的一条内容。MVP 不做压缩/检索（Phase 2），
//! 只做优先级排序 + 预算裁剪（见 agent-context）。

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// 条目类别。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ContextItemKind {
    Message,
    ToolObservation,
    Document,
    Memory,
    Summary,
    Artifact,
    Reference,
    WorkspaceState,
    Plan,
    Finding,
}

/// 数据分级（§24）。MVP 主要用于 scheduler 的 privacy 过滤。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum DataClassification {
    #[default]
    Public,
    Internal,
    Confidential,
    Secret,
}

/// working memory 的一条内容。
///
/// `priority` 越小越先保留（0 = 最先保留）。预算裁剪按 `(priority asc, timestamp asc)` 排序后
/// 从低优先级端丢弃。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContextItem {
    pub id: Uuid,
    pub kind: ContextItemKind,
    pub content: String,
    /// 0 = 最先保留。
    pub priority: u8,
    pub token_estimate: u32,
    /// 来源标识：`"user"` | `"tool://shell#<call-id>"` | `"file://AGENTS.md"` …
    pub source: String,
    pub timestamp: DateTime<Utc>,
    pub classification: DataClassification,
    /// MVP 恒 false（压缩属 Phase 2）。
    pub expandable: bool,
}

impl ContextItem {
    pub fn new(kind: ContextItemKind, content: impl Into<String>, priority: u8) -> Self {
        let content = content.into();
        let token_estimate = (content.len() as u32) / 4; // 粗估，agent-context 有更准的估算器
        Self {
            id: Uuid::new_v4(),
            kind,
            content,
            priority,
            token_estimate,
            source: String::new(),
            timestamp: Utc::now(),
            classification: DataClassification::default(),
            expandable: false,
        }
    }
}
