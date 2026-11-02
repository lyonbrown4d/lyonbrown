//! Why the desktop currently refuses to switch chats or replace the daemon
//! connection, and the exact user-facing text for each reason.
//!
//! [`Desktop::block_reason`] is the only decision point: the connection command,
//! every chat-tab guard, the error line and `DesktopSnapshot` read this enum
//! instead of re-checking workbench flags or comparing banner strings.

use crate::shell::Desktop;
use gpui_kit::*;

/// One stable reason why switching chats or reconnecting is not allowed yet.
///
/// The variant order is the evaluation order of [`Desktop::block_reason`]:
/// connection and local restore first, then the active chat's own recovery
/// inspection, then the workbench operations that would be abandoned by a
/// switch.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum BlockReason {
  /// A daemon connection attempt is already in flight.
  Connecting,
  /// Local desktop state is still being restored.
  RestoringState,
  /// The active chat's abnormal-run inspection or recovery request is pending.
  RecoveryPending,
  /// A computer-operation request is in flight.
  ComputerPending,
  /// A workbench file save is in flight.
  SavePending,
  /// A browser-operation request is in flight.
  BrowserPending,
}

impl BlockReason {
  /// User-facing text; `None` means the guard stays silent (no error banner).
  pub(crate) fn message(self) -> Option<&'static str> {
    match self {
      Self::ComputerPending => {
        Some("Wait for the computer operation to finish before changing chats or the connection.")
      }
      Self::SavePending => Some("Wait for the file save to finish before switching chats."),
      Self::BrowserPending => {
        Some("Wait for the browser operation to finish before changing chats or the connection.")
      }
      Self::RecoveryPending => Some("等待异常运行核对或恢复请求结束后再切换聊天。"),
      Self::Connecting | Self::RestoringState => None,
    }
  }

  /// Stable machine-readable name for diagnostics; never shown to users.
  pub(crate) fn label(self) -> &'static str {
    match self {
      Self::Connecting => "connecting",
      Self::RestoringState => "restoring_state",
      Self::RecoveryPending => "recovery_pending",
      Self::ComputerPending => "computer_pending",
      Self::SavePending => "save_pending",
      Self::BrowserPending => "browser_pending",
    }
  }
}

impl Desktop {
  /// The single ordered decision point for "may the user switch chats or the
  /// connection now?". Every guard calls this instead of re-checking flags.
  pub(super) fn block_reason(&self, cx: &App) -> Option<BlockReason> {
    if self.navigation.connecting {
      return Some(BlockReason::Connecting);
    }
    if self.persistence_loading {
      return Some(BlockReason::RestoringState);
    }
    if self.vm.recovery.as_ref().is_some_and(|state| state.pending) {
      return Some(BlockReason::RecoveryPending);
    }
    let tasks = self.tasks.read(cx);
    if tasks.computer.read(cx).pending {
      return Some(BlockReason::ComputerPending);
    }
    if tasks.pending_save {
      return Some(BlockReason::SavePending);
    }
    if tasks.pending_browser_operation {
      return Some(BlockReason::BrowserPending);
    }
    None
  }

  /// Whether one recorded reason's own condition is still true. Completion is
  /// decided per reason, never by comparing banner text.
  pub(super) fn block_reason_holds(&self, reason: BlockReason, cx: &App) -> bool {
    match reason {
      BlockReason::Connecting => self.navigation.connecting,
      BlockReason::RestoringState => self.persistence_loading,
      BlockReason::RecoveryPending => self.vm.recovery.as_ref().is_some_and(|state| state.pending),
      BlockReason::ComputerPending => self.tasks.read(cx).computer.read(cx).pending,
      BlockReason::SavePending => self.tasks.read(cx).pending_save,
      BlockReason::BrowserPending => self.tasks.read(cx).pending_browser_operation,
    }
  }

  /// The line shown under the composer. A real error wins over the wait banner,
  /// so `vm.error` never has to carry guard text.
  pub(super) fn error_line(&self) -> Option<String> {
    let wait = self
      .blocked
      .and_then(BlockReason::message)
      .map(str::to_string);
    self.vm.error.clone().or(wait)
  }

  /// Shared guard for activating, creating and closing chat tabs, selecting a
  /// session, and reconnecting. Records the visible wait reason so the view and
  /// the snapshot can show it without writing into `vm.error`.
  pub(super) fn can_change_chat(&mut self, cx: &mut Context<Self>) -> bool {
    match self.block_reason(cx) {
      Some(reason) => {
        if reason.message().is_some() {
          self.blocked = Some(reason);
          cx.notify();
        } else if self.blocked.take().is_some() {
          // Silent guards never show a banner; drop a stale one instead.
          cx.notify();
        }
        false
      }
      None => {
        if self.blocked.take().is_some() {
          cx.notify();
        }
        true
      }
    }
  }
}
