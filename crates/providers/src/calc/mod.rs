//! Inline math, unit and currency conversion, via fend.

mod rates;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use greatheights_core::{Action, Icon, Item, Matcher, Provider, Run};

use self::rates::Rates;

/// Ranked above everything except an explicit bang.
const SCORE: u32 = 10_000;
/// Long enough for real calculations; short enough to never stall typing.
const TIMEOUT: Duration = Duration::from_millis(50);

pub struct CalcProvider {
    context: Mutex<fend_core::Context>,
    rates: Rates,
    /// Set when the last evaluation looked up an exchange rate.
    used_rates: Arc<AtomicBool>,
}

/// An evaluated query.
struct Answer {
    text: String,
    used_rates: bool,
}

impl CalcProvider {
    /// `rates_cache` is where exchange rates are kept between runs.
    pub fn new(rates_cache: PathBuf) -> Self {
        let provider = Self::with_rates(Rates::load(rates_cache));
        provider.rates.refresh_if_stale();
        provider
    }

    fn with_rates(rates: Rates) -> Self {
        let used_rates = Arc::new(AtomicBool::new(false));
        let mut context = fend_core::Context::new();
        context.set_exchange_rate_handler_v2(RateLookup {
            rates: rates.clone(),
            used: used_rates.clone(),
        });
        Self {
            context: Mutex::new(context),
            rates,
            used_rates,
        }
    }

    fn evaluate(&self, query: &str) -> Option<Answer> {
        // Without a digit, fend reads plain words as units ("m" is metres),
        // which would put a calculator result on top of every app search.
        if !query.chars().any(|c| c.is_ascii_digit()) {
            return None;
        }
        let context = self.context.lock().unwrap();
        self.used_rates.store(false, Ordering::Relaxed);
        let deadline = Deadline(Instant::now() + TIMEOUT);
        let result = fend_core::evaluate_preview_with_interrupt(query, &context, &deadline);
        let answer = result.get_main_result().trim();
        // fend marks rounded results; the digits already say as much.
        let answer = answer.strip_prefix("approx. ").unwrap_or(answer);
        let used_rates = self.used_rates.load(Ordering::Relaxed);
        let text = if used_rates {
            round_money(answer)
        } else {
            answer.to_owned()
        };
        (!text.is_empty()).then_some(Answer { text, used_rates })
    }
}

/// Rounds "89.8472596586 EUR" to "89.85 EUR". Anything else, such as a
/// result in another unit, is left as fend wrote it.
fn round_money(answer: &str) -> String {
    let Some((amount, currency)) = answer.split_once(' ') else {
        return answer.to_owned();
    };
    match amount.parse::<f64>() {
        Ok(amount) if currency.len() == 3 && currency.chars().all(|c| c.is_ascii_uppercase()) => {
            format!("{amount:.2} {currency}")
        }
        _ => answer.to_owned(),
    }
}

impl Provider for CalcProvider {
    fn search(&self, query: &str, _matcher: &mut Matcher) -> Vec<Item> {
        let Some(answer) = self.evaluate(query) else {
            return Vec::new();
        };
        let subtitle = match self.rates.date().filter(|_| answer.used_rates) {
            Some(date) => format!("ECB rates of {date}"),
            None => "Copy result".into(),
        };
        vec![Item {
            id: String::new(),
            title: answer.text.clone(),
            subtitle: Some(subtitle),
            icon: Icon::Glyph("="),
            actions: vec![Action::new("Copy", Run::CopyText(answer.text))],
            score: SCORE,
        }]
    }

    fn refresh(&self) {
        self.rates.refresh_if_stale();
    }
}

/// Answers fend's exchange rate questions from the cached rates, with the
/// euro as the base currency. fend wants how many units of the currency one
/// base unit buys, which is exactly what the ECB publishes.
struct RateLookup {
    rates: Rates,
    used: Arc<AtomicBool>,
}

impl fend_core::ExchangeRateFnV2 for RateLookup {
    fn relative_to_base_currency(
        &self,
        currency: &str,
        _options: &fend_core::ExchangeRateFnV2Options,
    ) -> Result<f64, Box<dyn std::error::Error + Send + Sync + 'static>> {
        self.used.store(true, Ordering::Relaxed);
        self.rates
            .per_euro(currency)
            .ok_or_else(|| format!("no exchange rate for {currency}").into())
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

    fn calc() -> CalcProvider {
        CalcProvider::with_rates(rates::fixture(&[("USD", 1.25), ("JPY", 160.)]))
    }

    fn eval(query: &str) -> Option<String> {
        calc().evaluate(query).map(|answer| answer.text)
    }

    #[test]
    fn evaluates_math_and_units() {
        assert_eq!(eval("12*7").as_deref(), Some("84"));
        assert_eq!(eval("2 km to m").as_deref(), Some("2000 m"));
        assert_eq!(eval("5 km in mi").as_deref(), Some("3.1068559612 mi"));
    }

    #[test]
    fn converts_currencies() {
        assert_eq!(eval("10 EUR to USD").as_deref(), Some("12.50 USD"));
        assert_eq!(eval("25 usd in eur").as_deref(), Some("20.00 EUR"));
        assert_eq!(eval("1.25 USD to JPY").as_deref(), Some("160.00 JPY"));
        assert_eq!(eval("1 USD to EUR").as_deref(), Some("0.80 EUR"));
        let calc = calc();
        assert!(calc.evaluate("10 EUR to USD").unwrap().used_rates);
        assert!(!calc.evaluate("12*7").unwrap().used_rates);
    }

    #[test]
    fn unknown_currency_gives_no_result() {
        assert_eq!(eval("10 EUR to CHF"), None);
    }

    #[test]
    fn ignores_plain_words_and_bare_numbers() {
        assert_eq!(eval("m"), None);
        assert_eq!(eval("safari"), None);
        assert_eq!(eval("42"), None);
        assert_eq!(eval("1password"), None);
    }
}
