use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

const DAY: f64 = 86_400.0;

#[derive(Clone, Copy, Default, Serialize, Deserialize)]
struct Entry {
    count: u32,
    last_used: u64,
}

/// Remembers how often and how recently each item was picked.
pub struct Frecency {
    path: Option<PathBuf>,
    entries: HashMap<String, Entry>,
}

impl Frecency {
    pub fn in_memory() -> Self {
        Self {
            path: None,
            entries: HashMap::new(),
        }
    }

    pub fn load(path: PathBuf) -> Self {
        let entries = std::fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        Self {
            path: Some(path),
            entries,
        }
    }

    /// Score added on top of the fuzzy score. Roughly comparable to a few
    /// matched characters, so usage reorders close matches without letting a
    /// weak match jump over a strong one.
    pub fn bonus(&self, id: &str) -> u32 {
        let Some(entry) = self.entries.get(id) else {
            return 0;
        };
        let age_days = now().saturating_sub(entry.last_used) as f64 / DAY;
        let recency = 1.0 / (1.0 + age_days / 14.0);
        ((entry.count as f64).ln_1p() * 24.0 * recency) as u32
    }

    pub fn record(&mut self, id: &str) {
        let entry = self.entries.entry(id.to_owned()).or_default();
        entry.count = entry.count.saturating_add(1);
        entry.last_used = now();
        self.save();
    }

    fn save(&self) {
        let Some(path) = &self.path else { return };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        match serde_json::to_vec(&self.entries) {
            Ok(bytes) => {
                if let Err(err) = std::fs::write(path, bytes) {
                    log::warn!("failed to save frecency to {}: {err}", path.display());
                }
            }
            Err(err) => log::warn!("failed to serialize frecency: {err}"),
        }
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_raise_bonus() {
        let mut f = Frecency::in_memory();
        assert_eq!(f.bonus("a"), 0);
        f.record("a");
        let once = f.bonus("a");
        f.record("a");
        assert!(f.bonus("a") > once);
    }
}
