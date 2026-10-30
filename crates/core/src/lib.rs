//! Search model shared by the UI and every provider.
//!
//! A [`Provider`] turns a query into scored [`Item`]s. Each item carries the
//! [`Action`]s it supports; the first one is what Enter does.

mod engine;
mod frecency;
mod matcher;
pub mod os;
mod settings;

use std::path::PathBuf;

pub use engine::Engine;
pub use frecency::Frecency;
pub use matcher::Matcher;
pub use settings::{
    AGENT_PRESETS, AgentConfig, AgentPreset, SearchEngine, Settings, settings_path,
};

#[derive(Clone, Debug)]
pub struct Item {
    /// Stable identifier, used to remember how often the item is picked.
    /// Leave empty for results that shouldn't be remembered, such as anything
    /// echoing the query back.
    pub id: String,
    pub title: String,
    pub subtitle: Option<String>,
    pub icon: Icon,
    /// The first action is the primary one.
    pub actions: Vec<Action>,
    pub score: u32,
}

#[derive(Clone, Debug)]
pub enum Icon {
    None,
    /// A PNG/SVG file on disk.
    Image(PathBuf),
    /// A short symbol drawn in a tile, for results that aren't files.
    Glyph(&'static str),
}

#[derive(Clone, Debug)]
pub struct Action {
    pub title: String,
    pub run: Run,
    /// Hard-to-undo actions (restart, shut down) run on a second Enter.
    pub confirm: bool,
}

impl Action {
    pub fn new(title: impl Into<String>, run: Run) -> Self {
        Self {
            title: title.into(),
            run,
            confirm: false,
        }
    }

    pub fn confirmed(mut self) -> Self {
        self.confirm = true;
        self
    }
}

#[derive(Clone, Debug)]
pub enum Run {
    /// Open a file, folder or app bundle with the OS default handler.
    OpenPath(PathBuf),
    /// Show the path in the platform file manager.
    RevealPath(PathBuf),
    OpenUrl(String),
    Spawn { program: String, args: Vec<String> },
    /// Handled by the UI, which owns the clipboard.
    CopyText(String),
    /// Handled by the UI.
    Quit,
    /// Ask the configured agent; handled by the UI.
    Ask(String),
    /// Open the settings window; handled by the UI.
    OpenSettings,
}

pub trait Provider: Send + Sync {
    /// Returns matches for a non-empty query.
    fn search(&self, query: &str, matcher: &mut Matcher) -> Vec<Item>;

    /// Rebuilds any cached index. Called off the UI thread.
    fn refresh(&self) {}

    /// Image files this provider's results may show, so the UI can load them
    /// ahead of time instead of popping them in.
    fn icons(&self) -> Vec<PathBuf> {
        Vec::new()
    }
}
