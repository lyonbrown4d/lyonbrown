/// A deliberately simple provider-neutral token estimate.
#[derive(Debug, Clone, Copy)]
pub struct TokenEstimator {
    characters_per_token: u32,
}

impl TokenEstimator {
    pub fn new(characters_per_token: u32) -> Self {
        Self {
            characters_per_token: characters_per_token.max(1),
        }
    }

    pub fn estimate(&self, text: &str) -> u32 {
        let characters = u32::try_from(text.chars().count()).unwrap_or(u32::MAX);
        characters.div_ceil(self.characters_per_token)
    }
}

impl Default for TokenEstimator {
    fn default() -> Self {
        Self::new(4)
    }
}
