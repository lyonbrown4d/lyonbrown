use agent_cli::tui::Theme;

#[test]
fn all_builtin_themes_have_exact_design_tokens() {
  let expected = [
    (
      "carbon-mint",
      [
        0x090c10, 0x121820, 0x18212b, 0xe8edf2, 0x9aa5b1, 0x647080, 0x5eead4, 0xa5b4fc, 0x4ade80,
        0xfbbf24, 0xfb7185, 0x273240,
      ],
    ),
    (
      "ink-violet",
      [
        0x0c0b10, 0x16141d, 0x1d1928, 0xf0edf5, 0xa39cac, 0x6f687a, 0xa78bfa, 0x7dd3fc, 0x5eead4,
        0xf6c453, 0xfb7185, 0x332d40,
      ],
    ),
    (
      "slate-blue",
      [
        0x0a0f16, 0x111a24, 0x182433, 0xe6edf5, 0x98a6b8, 0x64748b, 0x60a5fa, 0x67e8f9, 0x34d399,
        0xf5b942, 0xf87171, 0x2b3a4d,
      ],
    ),
    (
      "hearth-teal",
      [
        0x0b1113, 0x122024, 0x182a2f, 0xeae7df, 0xa8a39a, 0x716d66, 0xe07a5f, 0x74c7b8, 0x6fcf97,
        0xe9b949, 0xe76f76, 0x2a3a3d,
      ],
    ),
    (
      "sandstone-espresso",
      [
        0x100e0c, 0x1a1713, 0x241f19, 0xeee9e0, 0xa9a093, 0x70685e, 0xd4a373, 0x8fb996, 0x7bc8a4,
        0xe8b44f, 0xe27d75, 0x3a3229,
      ],
    ),
    (
      "graphite-coral",
      [
        0x101114, 0x191a1f, 0x22242b, 0xeceef2, 0x9da3ae, 0x666c78, 0xf28c70, 0x82a7c9, 0x74c69d,
        0xf0b95b, 0xef6f78, 0x30333d,
      ],
    ),
  ];

  assert_eq!(Theme::built_ins().len(), expected.len());
  for (theme, (id, tokens)) in Theme::built_ins().iter().zip(expected) {
    assert_eq!(theme.id, id);
    assert_eq!(theme.rgb_tokens(), tokens);
    assert_eq!(theme.focus_ring, theme.accent_primary);
  }
  assert_eq!(Theme::default().id, "hearth-teal");
  assert_eq!(Theme::from_id("unknown").id, "hearth-teal");
}

#[test]
fn terminal_session_uses_inline_viewport_without_alternate_screen() {
  let source = include_str!("../src/tui/terminal.rs");
  assert!(source.contains("Viewport::Inline"));
  assert!(!source.contains("EnterAlternateScreen"));
  assert!(!source.contains("LeaveAlternateScreen"));
}
