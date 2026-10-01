use std::collections::{HashMap, VecDeque};

use agent_app::RuntimeCommandError;
use sha2::{Digest, Sha256};
use tokio::sync::watch;

pub(crate) type CommandKey = (String, String);
pub(crate) type Fingerprint = [u8; 32];
type ReplayResult = Result<(), RuntimeCommandError>;

pub(crate) enum ReplayDecision {
  Execute(ReplayWaiter),
  Wait(ReplayWaiter),
  Replay(ReplayResult),
  Conflict,
}

pub(crate) struct ReplayWaiter {
  result: watch::Receiver<Option<ReplayResult>>,
}

impl ReplayWaiter {
  pub(crate) async fn wait(mut self) -> ReplayResult {
    loop {
      if let Some(result) = self.result.borrow_and_update().clone() {
        return result;
      }
      if self.result.changed().await.is_err() {
        return Err(RuntimeCommandError::RuntimeStopped);
      }
    }
  }
}

enum ReplayEntry {
  Pending {
    fingerprint: Fingerprint,
    result: watch::Sender<Option<ReplayResult>>,
  },
  Completed {
    fingerprint: Fingerprint,
    result: ReplayResult,
  },
}

pub(crate) struct ReplayCache {
  capacity: usize,
  entries: HashMap<CommandKey, ReplayEntry>,
  completed_order: VecDeque<CommandKey>,
  completed_count: usize,
}

impl ReplayCache {
  pub(crate) fn new(capacity: usize) -> Self {
    Self {
      capacity: capacity.max(1),
      entries: HashMap::new(),
      completed_order: VecDeque::new(),
      completed_count: 0,
    }
  }

  pub(crate) fn reserve(&mut self, key: CommandKey, fingerprint: Fingerprint) -> ReplayDecision {
    match self.entries.get(&key) {
      Some(ReplayEntry::Pending {
        fingerprint: existing,
        result,
      }) if *existing == fingerprint => ReplayDecision::Wait(ReplayWaiter {
        result: result.subscribe(),
      }),
      Some(ReplayEntry::Completed {
        fingerprint: existing,
        result,
      }) if *existing == fingerprint => ReplayDecision::Replay(result.clone()),
      Some(_) => ReplayDecision::Conflict,
      None => {
        let (result, waiter) = watch::channel(None);
        self.entries.insert(
          key,
          ReplayEntry::Pending {
            fingerprint,
            result,
          },
        );
        ReplayDecision::Execute(ReplayWaiter { result: waiter })
      }
    }
  }

  pub(crate) fn complete(&mut self, key: CommandKey, result: ReplayResult) {
    let Some(ReplayEntry::Pending {
      fingerprint,
      result: sender,
    }) = self.entries.remove(&key)
    else {
      return;
    };
    sender.send_replace(Some(result.clone()));
    self.evict_completed_for_insert();
    self.entries.insert(
      key.clone(),
      ReplayEntry::Completed {
        fingerprint,
        result,
      },
    );
    self.completed_order.push_back(key);
    self.completed_count += 1;
  }

  fn evict_completed_for_insert(&mut self) {
    while self.completed_count >= self.capacity {
      let Some(oldest) = self.completed_order.pop_front() else {
        break;
      };
      if matches!(
        self.entries.get(&oldest),
        Some(ReplayEntry::Completed { .. })
      ) {
        self.entries.remove(&oldest);
        self.completed_count -= 1;
      }
    }
  }
}

pub(crate) fn fingerprint(kind: &str, fields: &[&[u8]]) -> Fingerprint {
  let mut digest = Sha256::new();
  update_field(&mut digest, kind.as_bytes());
  for field in fields {
    update_field(&mut digest, field);
  }
  digest.finalize().into()
}

fn update_field(digest: &mut Sha256, value: &[u8]) {
  digest.update((value.len() as u64).to_be_bytes());
  digest.update(value);
}
