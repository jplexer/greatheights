//! Inline math and unit conversion, via fend.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use greatheights_core::{Action, Icon, Item, Matcher, Provider, Run};

/// Ranked above everything except an explicit bang.
const SCORE: u32 = 10_000;
/// Long enough for real calculations; short enough to never stall typing.
const TIMEOUT: Duration = Duration::from_millis(50);

pub struct CalcProvider {
    context: Mutex<fend_core::Context>,
}

impl CalcProvider {
    pub fn new() -> Self {
        Self {
            context: Mutex::new(fend_core::Context::new()),
        }
    }

    pub fn evaluate(&self, query: &str) -> Option<String> {
        // Without a digit, fend reads plain words as units ("m" is metres),
        // which would put a calculator result on top of every app search.
        if !query.chars().any(|c| c.is_ascii_digit()) {
            return None;
        }
        let context = self.context.lock().unwrap();
        let deadline = Deadline(Instant::now() + TIMEOUT);
        let result = fend_core::evaluate_preview_with_interrupt(query, &context, &deadline);
        let answer = result.get_main_result().trim();
        // fend marks rounded results; the digits already say as much.
        let answer = answer.strip_prefix("approx. ").unwrap_or(answer);
        (!answer.is_empty()).then(|| answer.to_owned())
    }
}

impl Default for CalcProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl Provider for CalcProvider {
    fn search(&self, query: &str, _matcher: &mut Matcher) -> Vec<Item> {
        let Some(answer) = self.evaluate(query) else {
            return Vec::new();
        };
        vec![Item {
            id: String::new(),
            title: answer.clone(),
            subtitle: Some("Copy result".into()),
            icon: Icon::Glyph("="),
            actions: vec![Action::new("Copy", Run::CopyText(answer))],
            score: SCORE,
        }]
    }
}

struct Deadline(Instant);

impl fend_core::Interrupt for Deadline {
    fn should_interrupt(&self) -> bool {
        Instant::now() >= self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evaluates_math_and_units() {
        let calc = CalcProvider::new();
        assert_eq!(calc.evaluate("12*7").as_deref(), Some("84"));
        assert_eq!(calc.evaluate("2 km to m").as_deref(), Some("2000 m"));
        assert_eq!(
            calc.evaluate("5 km in mi").as_deref(),
            Some("3.1068559612 mi")
        );
    }

    #[test]
    fn ignores_plain_words_and_bare_numbers() {
        let calc = CalcProvider::new();
        assert_eq!(calc.evaluate("m"), None);
        assert_eq!(calc.evaluate("safari"), None);
        assert_eq!(calc.evaluate("42"), None);
        assert_eq!(calc.evaluate("1password"), None);
    }
}
