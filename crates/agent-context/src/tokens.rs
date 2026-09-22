/// A deliberately simple provider-neutral token estimate.
#[derive(Debug, Clone, Copy)]
pub struct TokenEstimator {
    characters_per_token: u64,
}

impl TokenEstimator {
    pub fn new(characters_per_token: u64) -> Self {
        Self {
            characters_per_token: characters_per_token.max(1),
        }
    }

    pub fn estimate(&self, text: &str) -> u64 {
        let characters = u64::try_from(text.chars().count()).unwrap_or(u64::MAX);
        estimate_character_count(characters, self.characters_per_token)
    }
}

fn estimate_character_count(characters: u64, characters_per_token: u64) -> u64 {
    characters.div_ceil(characters_per_token.max(1))
}

impl Default for TokenEstimator {
    fn default() -> Self {
        Self::new(4)
    }
}

#[cfg(test)]
mod tests {
    use super::estimate_character_count;

    #[test]
    fn estimates_character_counts_beyond_u32_without_truncation() {
        let characters = u64::from(u32::MAX) + 9;
        assert_eq!(
            estimate_character_count(characters, 4),
            characters.div_ceil(4)
        );
    }
}
