use agent_app::{ModelRoute, RouteSummary, RuntimePhase, RuntimeSnapshot};
use agent_cli::tui::{Locale, RenderContext, TuiState, apply_remote_state, render_with_context};
use agent_client::{ConnectionState, RemoteState, VersionedRemoteSnapshot};
use ratatui::{Terminal, backend::TestBackend};

#[test]
fn a_snapshot_without_a_route_clears_the_previous_epoch_model() {
  let mut state = TuiState::default();
  let mut epoch_a = RuntimeSnapshot {
    phase: RuntimePhase::Ready,
    ..RuntimeSnapshot::default()
  };
  epoch_a.route = Some(RouteSummary {
    selected: ModelRoute {
      provider: "provider-a".into(),
      model: "model-a".into(),
    },
    reason: "selected on daemon A".into(),
    fallbacks: vec![],
  });
  apply_remote_state(&mut state, &connected("daemon-a", epoch_a));
  assert!(render_text(&state).contains("provider-a/model-a"));

  apply_remote_state(
    &mut state,
    &connected(
      "daemon-b",
      RuntimeSnapshot {
        phase: RuntimePhase::Ready,
        ..RuntimeSnapshot::default()
      },
    ),
  );

  let rendered = render_text(&state);
  assert!(!rendered.contains("provider-a/model-a"), "{rendered}");
  assert!(rendered.contains('—'), "{rendered}");
}

fn connected(server_instance_id: &str, snapshot: RuntimeSnapshot) -> RemoteState {
  RemoteState {
    connection: ConnectionState::Connected {
      server_instance_id: server_instance_id.into(),
    },
    snapshot: Some(VersionedRemoteSnapshot {
      server_instance_id: server_instance_id.into(),
      revision: 1,
      snapshot,
    }),
  }
}

fn render_text(state: &TuiState) -> String {
  let backend = TestBackend::new(96, 18);
  let mut terminal = Terminal::new(backend).unwrap();
  terminal
    .draw(|frame| {
      render_with_context(
        frame,
        state,
        RenderContext {
          locale: Locale::EnUs,
          ..RenderContext::default()
        },
      )
    })
    .unwrap();
  let buffer = terminal.backend().buffer();
  (0..buffer.area.height)
    .map(|y| {
      (0..buffer.area.width)
        .map(|x| buffer[(x, y)].symbol())
        .collect::<String>()
    })
    .collect::<Vec<_>>()
    .join("\n")
}
