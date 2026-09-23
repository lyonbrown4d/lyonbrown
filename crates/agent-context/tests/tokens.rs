include!("../src/tokens.rs");

#[test]
fn estimates_character_counts_beyond_u32_without_truncation() {
  let characters = u64::from(u32::MAX) + 9;
  assert_eq!(
    estimate_character_count(characters, 4),
    characters.div_ceil(4)
  );
}
