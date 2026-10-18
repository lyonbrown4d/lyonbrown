mod support;

use agent_computer::*;
use agent_computer_runtime::*;
use support::*;
use uuid::Uuid;

fn renewal(lease: &LeaseToken) -> Operation {
  Operation::RenewLease {
    lease: lease.clone(),
    ttl_ms: 300_000,
  }
}

#[test]
fn renewal_keeps_same_run_token_and_delayed_preparation_live_beyond_five_minutes() {
  let f = Fixture::new();
  let lease = f.lease();
  let prepared = f.prepare(&lease);
  for _ in 0..4 {
    f.clock.advance(100_000);
    match f.call(renewal(&lease)).expect("renew") {
      Reply::Lease(renewed) => assert_eq!(renewed, lease),
      _ => panic!("lease reply"),
    }
  }
  assert_eq!(f.input(), 0);
  assert_eq!(f.effects.lock().expect("effects").limits.len(), 1);
  let result = match f.call(execution(&prepared)).expect("delayed execution") {
    Reply::Executed(result) => result,
    _ => panic!("execution reply"),
  };
  assert_eq!(result.preparation_id, prepared.preparation_id);
  assert_eq!(result.original_request.lease, lease);
  assert_eq!(result.receipt.outcome, ActionOutcome::Completed);
  assert_eq!(f.input(), 1);
}

#[test]
fn wrong_run_epoch_and_stale_tokens_cannot_renew_or_extend_replacement() {
  let f = Fixture::new();
  let lease = f.lease();
  let mut wrong_run = f.binding.clone();
  wrong_run.run_id = Uuid::new_v4().to_string();
  let mut wrong_epoch = f.binding.clone();
  wrong_epoch.owner_epoch += 1;
  for binding in [wrong_run, wrong_epoch] {
    assert!(matches!(
      f.as_binding(binding, renewal(&lease)),
      Err(RuntimeError::BindingMismatch)
    ));
  }
  f.clock.advance(300_000);
  assert!(matches!(
    f.call(renewal(&lease)),
    Err(RuntimeError::Desktop(DesktopError::LeaseInvalid))
  ));
  let next = f.lease();
  assert!(matches!(
    f.call(renewal(&lease)),
    Err(RuntimeError::Desktop(DesktopError::LeaseInvalid))
  ));
  f.call(Operation::Revoke {
    lease: next.clone(),
  })
  .expect("revoke");
  assert!(matches!(
    f.call(renewal(&next)),
    Err(RuntimeError::Desktop(DesktopError::LeaseInvalid))
  ));
  let replacement = f.lease();
  f.clock.advance(300_000);
  assert!(matches!(
    f.call(renewal(&replacement)),
    Err(RuntimeError::Desktop(DesktopError::LeaseInvalid))
  ));
  assert_eq!(f.input(), 0);
  assert!(f.effects.lock().expect("effects").limits.is_empty());
}
