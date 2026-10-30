use agent_cli::{
  config::ThemeChoice,
  model::{App, Connection},
  theme::Theme,
  view,
};
use ratatui::{Terminal, backend::TestBackend};

fn painted(app: &App, choice: ThemeChoice, width: u16, height: u16) -> ratatui::buffer::Buffer {
  let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("test backend");
  terminal
    .draw(|frame| view::render(frame, app, Theme::resolve(choice, None)))
    .expect("render");
  terminal.backend().buffer().clone()
}

fn visible(buffer: &ratatui::buffer::Buffer) -> String {
  let mut screen = String::new();
  for y in buffer.area.top()..buffer.area.bottom() {
    let mut x = buffer.area.left();
    while x < buffer.area.right() {
      let symbol = buffer[(x, y)].symbol();
      screen.push_str(symbol);
      x += ratatui::text::Span::raw(symbol).width().max(1) as u16;
    }
    screen.push('\n');
  }
  screen
}

fn visible_cells(buffer: &ratatui::buffer::Buffer) -> Vec<(u16, u16, &ratatui::buffer::Cell)> {
  let mut cells = Vec::new();
  for y in buffer.area.top()..buffer.area.bottom() {
    let mut x = buffer.area.left();
    while x < buffer.area.right() {
      let cell = &buffer[(x, y)];
      cells.push((x, y, cell));
      // Ratatui's diff skips cells covered by a wide glyph. They are not
      // independent terminal cells and TestBackend never receives their style.
      x += ratatui::text::Span::raw(cell.symbol()).width().max(1) as u16;
    }
  }
  cells
}

fn assert_surface(buffer: &ratatui::buffer::Buffer, theme: Theme) {
  let mismatches: Vec<_> = visible_cells(buffer)
    .into_iter()
    .filter(|(_, _, cell)| cell.bg != theme.background)
    .take(10)
    .map(|(x, y, cell)| (x, y, cell.symbol(), cell.fg, cell.bg, cell.modifier))
    .collect();
  assert!(
    mismatches.is_empty(),
    "expected background {:?}; first (x,y,text,fg,bg,modifier) mismatches: {mismatches:?}",
    theme.background
  );
}

#[test]
fn selected_task_remains_visible_in_a_short_narrow_terminal() {
  let app = App {
    panel: agent_cli::model::ContextPanel::Tasks,
    task_rows: (0..50)
      .map(|index| {
        serde_json::json!({
          "envelope": { "task_id": format!("task-{index:02}") },
          "status": "awaiting_acceptance", "attempt_status": "succeeded"
        })
      })
      .collect(),
    task_selected: 49,
    ..App::default()
  };
  let buffer = painted(&app, ThemeChoice::Dark, 48, 14);
  let screen = visible(&buffer);
  assert!(screen.contains("task-49"), "{screen}");
  assert!(!screen.contains("task-00"), "list must scroll to selection");
}

#[test]
fn warm_conversation_has_readable_cell_colors_and_no_empty_panels() {
  for choice in [ThemeChoice::Dark, ThemeChoice::Light] {
    let theme = Theme::resolve(choice, None);
    let buffer = painted(&App::default(), choice, 120, 32);
    let screen = visible(&buffer);
    assert!(screen.contains("从一个具体问题开始"));
    assert!(screen.contains("F1 帮助"));
    for obsolete in [
      "No tasks",
      "No pending approvals",
      "Sessions",
      "Conversation",
    ] {
      assert!(!screen.contains(obsolete));
    }
    assert_surface(&buffer, theme);
    assert!(visible_cells(&buffer).iter().filter(|(_, _, cell)| !cell.symbol().trim().is_empty()).all(|(_, _, cell)| {
      matches!(cell.fg, color if color == theme.text || color == theme.muted || color == theme.accent || color == theme.warning)
    }));
    assert!(
      visible_cells(&buffer).iter().any(|(_, _, cell)| {
        cell.modifier.contains(ratatui::style::Modifier::REVERSED)
          && cell.fg == theme.text
          && cell.bg == theme.background
      }),
      "the composer cursor must visibly reverse the active theme colors"
    );
    assert!(
      buffer
        .content
        .iter()
        .any(|cell| cell.symbol() == "L" && cell.fg == theme.accent)
    );
  }
}

#[test]
fn markdown_and_inline_approval_remain_visible_without_leaking_identifiers_or_controls() {
  use agent_cli::model::{Approval, Message};
  use ratatui::style::Modifier;
  let app = App {
    connection: Connection::Connected,
    messages: vec![Message { role: "Agent".into(), content: "## Result\n\n**important** and `Cargo.toml`\n\n- item\n\n```rust\nlet x = 1;\n```\n\n\u{1b}[31munsafe\u{7}".into() }],
    approvals: vec![Approval { id: "private-approval-id".into(), description: "write_file\nREADME.md".into() }],
    ..App::default()
  };
  let buffer = painted(&app, ThemeChoice::Dark, 120, 32);
  let screen = visible(&buffer);
  assert!(screen.contains("important"));
  assert!(!screen.contains("**important**"));
  assert!(screen.contains("Cargo.toml"));
  assert!(screen.contains("let x = 1;"));
  assert!(screen.contains("write_file"));
  assert!(screen.contains("Ctrl+Y 批准一次"));
  assert!(screen.contains("Ctrl+E 拒绝"));
  assert!(!screen.contains("private-approval-id"));
  assert!(!screen.contains('\u{1b}'));
  assert!(!screen.contains('\u{7}'));
  assert!(
    buffer
      .content
      .iter()
      .any(|cell| cell.symbol() == "i" && cell.modifier.contains(Modifier::BOLD))
  );
  for choice in [ThemeChoice::Light, ThemeChoice::Dark] {
    let theme = Theme::resolve(choice, None);
    let themed = painted(&app, choice, 120, 32);
    assert_surface(&themed, theme);
    assert!(
      visible_cells(&themed).iter().all(|(_, _, cell)| {
        [theme.text, theme.accent, theme.muted, theme.warning].contains(&cell.fg)
      }),
      "Markdown must use the active theme, including code and heading cells"
    );
  }
  for (width, height) in [(40, 12), (70, 16)] {
    let screen = visible(&painted(&app, ThemeChoice::Dark, width, height));
    assert!(
      screen.contains("Ctrl+Y"),
      "approval must not scroll away at {width}x{height}"
    );
    assert!(screen.contains("Ctrl+E"));
  }
}

#[test]
fn computer_context_names_the_binding_owner_without_identifiers() {
  use agent_cli::{
    computer::ComputerStatus,
    model::{ContextPanel, Session},
  };
  use agent_computer_runtime::DesktopScope;
  let mut app = App {
    panel: ContextPanel::Computer,
    computer: ComputerStatus::Attached {
      session: "private-owner".into(),
      scope: DesktopScope::Window(7),
    },
    sessions: vec![
      Session {
        id: "private-owner".into(),
        title: "Original".into(),
      },
      Session {
        id: "other".into(),
        title: "Other".into(),
      },
    ],
    ..App::default()
  };
  assert!(visible(&painted(&app, ThemeChoice::Dark, 80, 24)).contains("属于当前会话"));
  app.next_session();
  let screen = visible(&painted(&app, ThemeChoice::Dark, 80, 24));
  assert!(screen.contains("属于另一会话"));
  assert!(screen.contains("切换不会转移控制权"));
  assert!(!screen.contains("private-owner"));
}
