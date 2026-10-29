//! Currency exchange rates for the calculator.
//!
//! Uses the European Central Bank's daily reference rates (about 30 major
//! currencies, no API key). Rates are cached on disk so conversions work
//! offline and right after launch, and refreshed in the background once the
//! cache is older than [`MAX_AGE`]. Lookups never touch the network, so
//! typing never waits on it.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

const ECB_URL: &str = "https://www.ecb.europa.eu/stats/eurofxref/eurofxref-daily.xml";
/// The ECB publishes once per working day.
const MAX_AGE: Duration = Duration::from_secs(12 * 60 * 60);
const TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Clone, Serialize, Deserialize)]
struct Snapshot {
    /// When we downloaded it, in seconds since the epoch.
    fetched_at: u64,
    /// The ECB's publication date, e.g. "2026-10-29".
    date: String,
    /// Units of each currency per euro.
    per_euro: HashMap<String, f64>,
}

#[derive(Clone)]
pub struct Rates {
    snapshot: Arc<RwLock<Option<Snapshot>>>,
    cache: PathBuf,
    fetching: Arc<AtomicBool>,
}

impl Rates {
    pub fn load(cache: PathBuf) -> Self {
        let snapshot = std::fs::read(&cache)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok());
        Self {
            snapshot: Arc::new(RwLock::new(snapshot)),
            cache,
            fetching: Arc::default(),
        }
    }

    /// Units of `currency` one euro buys, from whatever is cached.
    pub fn per_euro(&self, currency: &str) -> Option<f64> {
        let snapshot = self.snapshot.read().unwrap();
        snapshot.as_ref()?.per_euro.get(currency).copied()
    }

    /// The ECB publication date of the cached rates.
    pub fn date(&self) -> Option<String> {
        self.snapshot.read().unwrap().as_ref().map(|s| s.date.clone())
    }

    /// Downloads fresh rates on a background thread if the cache is stale.
    pub fn refresh_if_stale(&self) {
        let fresh = self
            .snapshot
            .read()
            .unwrap()
            .as_ref()
            .is_some_and(|s| now().saturating_sub(s.fetched_at) < MAX_AGE.as_secs());
        if fresh || self.fetching.swap(true, Ordering::AcqRel) {
            return;
        }
        let rates = self.clone();
        std::thread::spawn(move || {
            match fetch() {
                Ok(snapshot) => rates.store(snapshot),
                Err(err) => log::warn!("couldn't fetch exchange rates: {err}"),
            }
            rates.fetching.store(false, Ordering::Release);
        });
    }

    fn store(&self, snapshot: Snapshot) {
        if let Some(dir) = self.cache.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        match serde_json::to_vec(&snapshot) {
            Ok(bytes) => {
                if let Err(err) = std::fs::write(&self.cache, bytes) {
                    log::warn!("couldn't cache exchange rates: {err}");
                }
            }
            Err(err) => log::warn!("couldn't serialize exchange rates: {err}"),
        }
        *self.snapshot.write().unwrap() = Some(snapshot);
    }
}

fn fetch() -> Result<Snapshot, Box<dyn std::error::Error>> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(TIMEOUT))
        .build()
        .into();
    let xml = agent.get(ECB_URL).call()?.body_mut().read_to_string()?;
    let snapshot = parse_ecb(&xml).ok_or("unexpected response format")?;
    Ok(snapshot)
}

/// Pulls `time='…'` and `currency='…' rate='…'` attributes out of the ECB
/// feed. The format has been stable for two decades; a full XML parser
/// would be overkill.
fn parse_ecb(xml: &str) -> Option<Snapshot> {
    let date = attribute(xml, "time")?.to_owned();
    let mut per_euro = HashMap::from([("EUR".to_owned(), 1.)]);
    for line in xml.lines() {
        if let (Some(currency), Some(rate)) = (attribute(line, "currency"), attribute(line, "rate"))
        {
            if let Ok(rate) = rate.parse() {
                per_euro.insert(currency.to_owned(), rate);
            }
        }
    }
    (per_euro.len() > 1).then(|| Snapshot {
        fetched_at: now(),
        date,
        per_euro,
    })
}

fn attribute<'a>(text: &'a str, name: &str) -> Option<&'a str> {
    let start = text.find(&format!("{name}='"))? + name.len() + 2;
    let len = text[start..].find('\'')?;
    Some(&text[start..start + len])
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
pub(super) fn fixture(per_euro: &[(&str, f64)]) -> Rates {
    let mut map: HashMap<String, f64> = per_euro.iter().map(|(c, r)| (c.to_string(), *r)).collect();
    map.insert("EUR".into(), 1.);
    Rates {
        snapshot: Arc::new(RwLock::new(Some(Snapshot {
            fetched_at: now(),
            date: "2026-10-29".into(),
            per_euro: map,
        }))),
        cache: std::env::temp_dir().join("greatheights-test-rates.json"),
        fetching: Arc::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ecb_feed() {
        let xml = "<Cube>\n\t<Cube time='2026-10-29'>\n\
                   \t\t<Cube currency='USD' rate='1.1130'/>\n\
                   \t\t<Cube currency='JPY' rate='175.92'/>\n\t</Cube>\n</Cube>";
        let snapshot = parse_ecb(xml).unwrap();
        assert_eq!(snapshot.date, "2026-10-29");
        assert_eq!(snapshot.per_euro["USD"], 1.1130);
        assert_eq!(snapshot.per_euro["EUR"], 1.);
    }

    #[test]
    fn rejects_unexpected_responses() {
        assert!(parse_ecb("<html>maintenance</html>").is_none());
    }
}
