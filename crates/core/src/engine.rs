use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::{Frecency, Item, Matcher, Provider, Settings, os};

const MAX_RESULTS: usize = 50;

/// Runs a query through every provider and ranks the combined results.
pub struct Engine {
    providers: Vec<Arc<dyn Provider>>,
    frecency: Mutex<Frecency>,
}

impl Engine {
    pub fn new(providers: Vec<Arc<dyn Provider>>, frecency: Frecency) -> Self {
        Self {
            providers,
            frecency: Mutex::new(frecency),
        }
    }

    pub fn search(&self, query: &str) -> Vec<Item> {
        let query = query.trim();
        if query.is_empty() {
            return Vec::new();
        }

        let mut matcher = Matcher::new(query);
        let mut items: Vec<Item> = self
            .providers
            .iter()
            .flat_map(|p| p.search(query, &mut matcher))
            .collect();

        let frecency = self.frecency.lock().unwrap();
        for item in items.iter_mut().filter(|item| !item.id.is_empty()) {
            item.score += frecency.bonus(&item.id);
        }
        drop(frecency);

        items.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.title.cmp(&b.title)));
        items.truncate(MAX_RESULTS);
        items
    }

    pub fn configure(&self, settings: &Settings) {
        for provider in &self.providers {
            provider.configure(settings);
        }
    }

    pub fn refresh(&self) {
        for provider in &self.providers {
            provider.refresh();
        }
    }

    pub fn icons(&self) -> Vec<PathBuf> {
        self.providers.iter().flat_map(|p| p.icons()).collect()
    }

    /// Runs the item's action at `index` and records the pick.
    pub fn execute(&self, item: &Item, index: usize) {
        let Some(action) = item.actions.get(index) else {
            return;
        };
        if let Err(err) = os::run(&action.run) {
            log::error!("failed to run {:?}: {err}", action.run);
            return;
        }
        self.record(item);
    }

    /// Remembers that the item was picked, for items the UI runs itself.
    pub fn record(&self, item: &Item) {
        if !item.id.is_empty() {
            self.frecency.lock().unwrap().record(&item.id);
        }
    }
}
