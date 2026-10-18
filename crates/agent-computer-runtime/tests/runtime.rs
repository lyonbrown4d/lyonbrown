mod support;
use agent_computer::*;
use agent_computer_runtime::*;
use support::*;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

#[test]
fn unchanged_delayed_approval_refreshes_and_preserves_original_binding() {
  let fixture = Fixture::new();
  let lease = fixture.lease();
  let prepared = fixture.prepare(&lease);
  fixture.clock.advance(10_000);
  let result = match fixture.call(execution(&prepared)).expect("execute") {
    Reply::Executed(result) => result,
    _ => panic!("execution reply"),
  };
  assert_eq!(fixture.input(), 1);
  assert_eq!(result.binding, fixture.binding);
  assert_eq!(result.preparation_id, prepared.preparation_id);
  assert!(result.original_request == prepared.request);
  assert_eq!(result.content_digest, prepared.content_digest);
  assert_ne!(result.receipt.snapshot_id, prepared.request.snapshot_id);
  assert_eq!(result.receipt.outcome, ActionOutcome::Completed);
  assert!(matches!(
    fixture.call(execution(&prepared)),
    Err(RuntimeError::PreparationUnavailable)
  ));
  let receipt = match fixture
    .call(Operation::Receipt {
      action_id: prepared.request.action_id,
    })
    .expect("receipt")
  {
    Reply::Receipt(Some(receipt)) => receipt,
    _ => panic!("receipt"),
  };
  assert_eq!(receipt.receipt, result.receipt);
  assert_eq!(receipt.binding, fixture.binding);
  assert_eq!(fixture.input(), 1);
  assert!(
    fixture
      .effects
      .lock()
      .expect("effects")
      .limits
      .iter()
      .all(|limits| limits.max_pixels == 4 && limits.max_bytes == 16)
  );
}

#[test]
fn changed_content_consumes_preparation_without_input_or_replay() {
  let fixture = Fixture::new();
  let lease = fixture.lease();
  let prepared = fixture.prepare(&lease);
  fixture.clock.advance(10_000);
  fixture.effects.lock().expect("effects").pixel = 1;
  assert!(matches!(
    fixture.call(execution(&prepared)),
    Err(RuntimeError::Desktop(DesktopError::StaleSnapshot))
  ));
  fixture.effects.lock().expect("effects").pixel = 0;
  assert!(matches!(
    fixture.call(execution(&prepared)),
    Err(RuntimeError::PreparationUnavailable)
  ));
  let mut request = fixture.request(&lease, &fixture.snapshot(&lease));
  request.action_id = prepared.request.action_id;
  assert!(matches!(
    fixture.call(Operation::Prepare { request }),
    Err(RuntimeError::Desktop(DesktopError::DuplicateAction))
  ));
  assert_eq!(fixture.input(), 0);
}

#[test]
fn wrong_run_or_epoch_cannot_observe_prepare_execute_revoke_or_read_receipt() {
  let fixture = Fixture::new();
  let lease = fixture.lease();
  let prepared = fixture.prepare(&lease);
  let mut wrong = fixture.binding.clone();
  wrong.owner_epoch += 1;
  for operation in [
    Operation::Observe {
      lease: lease.clone(),
      limits: ObservationLimits::default(),
      timeout_ms: 1000,
    },
    Operation::Prepare {
      request: prepared.request.clone(),
    },
    execution(&prepared),
    Operation::Revoke {
      lease: lease.clone(),
    },
    Operation::Receipt {
      action_id: prepared.request.action_id,
    },
  ] {
    assert!(matches!(
      fixture.as_binding(wrong.clone(), operation),
      Err(RuntimeError::BindingMismatch)
    ));
  }
  wrong = fixture.binding.clone();
  wrong.run_id = Uuid::new_v4().to_string();
  assert!(matches!(
    fixture.as_binding(wrong, execution(&prepared)),
    Err(RuntimeError::BindingMismatch)
  ));
  assert_eq!(fixture.input(), 0);
  fixture
    .call(execution(&prepared))
    .expect("owner remains authorized");
  assert_eq!(fixture.input(), 1);
  let mut other_session = fixture.binding.clone();
  other_session.daemon_session_id = Uuid::new_v4().to_string();
  assert!(matches!(
    fixture.as_binding(
      other_session,
      Operation::Receipt {
        action_id: prepared.request.action_id
      }
    ),
    Err(RuntimeError::BindingMismatch)
  ));
}

#[test]
fn mutated_approval_identity_and_digest_do_not_consume_original_preparation() {
  let fixture = Fixture::new();
  let prepared = fixture.prepare(&fixture.lease());
  let mut altered = prepared.clone();
  altered.content_digest[0] ^= 1;
  assert!(matches!(
    fixture.call(execution(&altered)),
    Err(RuntimeError::PreparationMismatch)
  ));
  altered = prepared.clone();
  altered.request.action_id = Uuid::new_v4();
  assert!(matches!(
    fixture.call(execution(&altered)),
    Err(RuntimeError::PreparationMismatch)
  ));
  let mut nil_approval = execution(&prepared);
  if let Operation::Execute { approval_id, .. } = &mut nil_approval {
    *approval_id = Uuid::nil();
  }
  assert!(matches!(
    fixture.call(nil_approval),
    Err(RuntimeError::InvalidControl)
  ));
  assert_eq!(fixture.input(), 0);
  fixture
    .call(execution(&prepared))
    .expect("original preparation");
  assert_eq!(fixture.input(), 1);
}

#[test]
fn expiry_and_revocation_never_reacquire_and_old_token_cannot_revoke_new_lease() {
  let fixture = Fixture::new();
  let lease = fixture.lease();
  let prepared = fixture.prepare(&lease);
  fixture
    .call(Operation::Revoke {
      lease: lease.clone(),
    })
    .expect("revoke");
  assert!(matches!(
    fixture.call(execution(&prepared)),
    Err(RuntimeError::PreparationUnavailable)
  ));
  let new_lease = fixture.lease();
  assert_ne!(lease.id, new_lease.id);
  assert!(matches!(
    fixture.call(Operation::Revoke { lease }),
    Err(RuntimeError::Desktop(DesktopError::LeaseInvalid))
  ));
  let prepared = fixture.prepare(&new_lease);
  fixture.clock.advance(300_000);
  assert!(matches!(
    fixture.call(execution(&prepared)),
    Err(RuntimeError::Desktop(DesktopError::LeaseInvalid))
  ));
  assert_eq!(fixture.input(), 0);
}

#[test]
fn cancellation_after_input_retains_uncertain_receipt_and_forbids_replay() {
  let fixture = Fixture::new();
  let prepared = fixture.prepare(&fixture.lease());
  let token = CancellationToken::new();
  fixture.effects.lock().expect("effects").cancel_input = Some(token.clone());
  let reply = fixture
    .engine
    .handle(
      Command {
        binding: fixture.binding.clone(),
        operation: execution(&prepared),
      },
      token,
    )
    .expect("uncertain receipt");
  assert!(matches!(
    reply,
    Reply::Executed(ExecutionResult {
      receipt: DesktopActionReceipt {
        outcome: ActionOutcome::Uncertain(DesktopError::Cancelled),
        ..
      },
      ..
    })
  ));
  assert!(matches!(
    fixture.call(execution(&prepared)),
    Err(RuntimeError::PreparationUnavailable)
  ));
  assert_eq!(fixture.input(), 1);
}

#[test]
fn invalid_binding_controls_and_pending_capacity_fail_without_input() {
  let fixture = Fixture::new();
  let mut invalid = fixture.binding.clone();
  invalid.owner_epoch = 0;
  assert!(matches!(
    fixture.as_binding(invalid, Operation::AcquireLease { ttl_ms: 1000 }),
    Err(RuntimeError::InvalidBinding)
  ));
  assert!(matches!(
    fixture.call(Operation::AcquireLease { ttl_ms: 300_001 }),
    Err(RuntimeError::Desktop(DesktopError::InvalidRequest(_)))
  ));
  let lease = fixture.lease();
  assert!(matches!(
    fixture.call(Operation::Observe {
      lease: lease.clone(),
      limits: ObservationLimits::default(),
      timeout_ms: 30_001
    }),
    Err(RuntimeError::InvalidControl)
  ));
  let original = fixture.prepare(&lease);
  assert!(matches!(
    fixture.call(Operation::Prepare {
      request: original.request.clone()
    }),
    Err(RuntimeError::Desktop(DesktopError::DuplicateAction))
  ));
  for _ in 1..64 {
    fixture.prepare(&lease);
  }
  let request = fixture.request(&lease, &fixture.snapshot(&lease));
  assert!(matches!(
    fixture.call(Operation::Prepare { request }),
    Err(RuntimeError::CapacityExceeded)
  ));
  fixture
    .call(execution(&original))
    .expect("capacity never evicted original");
  assert_eq!(fixture.input(), 1);
}

#[test]
fn terminal_identity_capacity_never_evicts_replay_guards() {
  let fixture = Fixture::new();
  let lease = fixture.lease();
  let first = fixture.prepare(&lease);
  fixture.call(execution(&first)).expect("first execute");
  for _ in 1..4096 {
    let prepared = fixture.prepare(&lease);
    fixture.call(execution(&prepared)).expect("execute");
  }
  let request = fixture.request(&lease, &fixture.snapshot(&lease));
  assert!(matches!(
    fixture.call(Operation::Prepare { request }),
    Err(RuntimeError::CapacityExceeded)
  ));
  assert!(matches!(
    fixture.call(execution(&first)),
    Err(RuntimeError::PreparationUnavailable)
  ));
  assert!(matches!(
    fixture.call(Operation::Receipt {
      action_id: first.request.action_id
    }),
    Ok(Reply::Receipt(Some(_)))
  ));
  assert_eq!(fixture.input(), 4096);
}
