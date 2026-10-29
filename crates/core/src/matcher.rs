use nucleo_matcher::pattern::{AtomKind, CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Utf32Str};

/// Fuzzy matcher for the current query, reused across providers.
pub struct Matcher {
    inner: nucleo_matcher::Matcher,
    pattern: Pattern,
    buf: Vec<char>,
}

impl Matcher {
    pub fn new(query: &str) -> Self {
        Self {
            inner: nucleo_matcher::Matcher::new(Config::DEFAULT),
            // Plain fuzzy atoms: no fzf syntax, so `!` and `'` match literally.
            pattern: Pattern::new(
                query,
                CaseMatching::Ignore,
                Normalization::Smart,
                AtomKind::Fuzzy,
            ),
            buf: Vec::new(),
        }
    }

    pub fn score(&mut self, haystack: &str) -> Option<u32> {
        self.pattern
            .score(Utf32Str::new(haystack, &mut self.buf), &mut self.inner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_word_initials() {
        let mut m = Matcher::new("vsc");
        assert!(m.score("Visual Studio Code").is_some());
        assert!(m.score("Safari").is_none());
    }

    #[test]
    fn prefix_beats_scattered_match() {
        let mut m = Matcher::new("term");
        let prefix = m.score("Terminal").unwrap();
        let scattered = m.score("The Remote Manager").unwrap_or(0);
        assert!(prefix > scattered);
    }
}

