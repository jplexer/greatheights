//! Installed applications.
//!
//! Each platform module scans its usual install locations into [`AppEntry`]s.
//! Icons are rendered to PNGs in a cache directory on a background thread,
//! so results show up immediately and gain icons as they become available.

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
mod desktop_entry;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "linux")]
use linux as platform;
#[cfg(target_os = "macos")]
use macos as platform;
#[cfg(target_os = "windows")]
use windows as platform;

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::UNIX_EPOCH;

use greatheights_core::{Action, Icon, Item, Matcher, Provider, Run};

#[derive(Clone, Debug)]
pub struct AppEntry {
    pub name: String,
    /// The bundle, desktop file or shortcut the app was found at.
    pub path: PathBuf,
    pub launch: Run,
}

/// An app as shown in results.
struct IndexedApp {
    entry: AppEntry,
    subtitle: String,
}

pub struct AppProvider {
    apps: RwLock<Vec<IndexedApp>>,
    icon_dir: PathBuf,
    rendering_icons: Arc<AtomicBool>,
}

impl AppProvider {
    pub fn new(icon_dir: PathBuf) -> Self {
        let provider = Self {
            apps: RwLock::new(Vec::new()),
            icon_dir,
            rendering_icons: Arc::default(),
        };
        provider.refresh();
        provider
    }

    fn icon_path(&self, app: &AppEntry) -> PathBuf {
        // Keyed on path and modification time, so an updated app gets a new icon.
        let mut hasher = DefaultHasher::new();
        app.path.hash(&mut hasher);
        modified_secs(&app.path).hash(&mut hasher);
        self.icon_dir.join(format!("{:016x}.png", hasher.finish()))
    }

    fn render_missing_icons(&self, apps: &[AppEntry]) {
        if !platform::CAN_RENDER_ICONS || self.rendering_icons.swap(true, Ordering::AcqRel) {
            return;
        }
        let missing: Vec<(PathBuf, PathBuf)> = apps
            .iter()
            .map(|app| (app.path.clone(), self.icon_path(app)))
            .filter(|(_, icon)| !icon.exists())
            .collect();
        if missing.is_empty() {
            self.rendering_icons.store(false, Ordering::Release);
            return;
        }
        if let Err(err) = std::fs::create_dir_all(&self.icon_dir) {
            log::warn!("can't create icon cache {}: {err}", self.icon_dir.display());
            self.rendering_icons.store(false, Ordering::Release);
            return;
        }
        let flag = self.rendering_icons.clone();
        std::thread::spawn(move || {
            for (app, icon) in missing {
                platform::render_icon(&app, &icon);
            }
            flag.store(false, Ordering::Release);
        });
    }
}

impl Provider for AppProvider {
    fn search(&self, _query: &str, matcher: &mut Matcher) -> Vec<Item> {
        let apps = self.apps.read().unwrap();
        apps.iter()
            .filter_map(|IndexedApp { entry: app, subtitle }| {
                let score = matcher.score(&app.name)?;
                let icon = self.icon_path(app);
                Some(Item {
                    id: format!("app:{}", app.path.display()),
                    title: app.name.clone(),
                    subtitle: Some(subtitle.clone()),
                    icon: if icon.exists() {
                        Icon::Image(icon)
                    } else {
                        Icon::None
                    },
                    actions: vec![
                        Action::new("Open", app.launch.clone()),
                        Action::new(platform::REVEAL_LABEL, Run::RevealPath(app.path.clone())),
                    ],
                    score,
                })
            })
            .collect()
    }

    fn icons(&self) -> Vec<PathBuf> {
        let apps = self.apps.read().unwrap();
        apps.iter()
            .map(|app| self.icon_path(&app.entry))
            .filter(|icon| icon.exists())
            .collect()
    }

    fn refresh(&self) {
        let mut apps = platform::scan();
        apps.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.path.cmp(&b.path)));
        apps.dedup_by(|a, b| a.path == b.path);
        self.render_missing_icons(&apps);

        // Same-named apps (say, a native and a CrossOver "Steam") are told
        // apart by the folder they live in.
        let indexed = apps
            .iter()
            .enumerate()
            .map(|(i, app)| {
                let shares_name = [i.checked_sub(1), Some(i + 1)]
                    .into_iter()
                    .flatten()
                    .filter_map(|j| apps.get(j))
                    .any(|other| other.name == app.name);
                IndexedApp {
                    entry: app.clone(),
                    subtitle: if shares_name {
                        display_folder(&app.path)
                    } else {
                        "Application".into()
                    },
                }
            })
            .collect();
        *self.apps.write().unwrap() = indexed;
    }
}

/// The app's folder, with the home directory shortened to `~`.
fn display_folder(path: &Path) -> String {
    let folder = path.parent().unwrap_or(path);
    match dirs::home_dir().and_then(|home| folder.strip_prefix(home).ok().map(Path::to_owned)) {
        Some(relative) => format!("~/{}", relative.display()),
        None => folder.display().to_string(),
    }
}

fn modified_secs(path: &Path) -> u64 {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
