use std::collections::HashSet;
use std::path::{Path, PathBuf};

use greatheights_core::Run;

use super::{AppEntry, desktop_entry};

// Icon themes need a lookup across theme directories and formats; not yet.
pub const CAN_RENDER_ICONS: bool = false;
pub const REVEAL_LABEL: &str = "Show in Files";

pub fn render_icon(_app: &Path, _out: &Path) {}

pub fn scan() -> Vec<AppEntry> {
    let mut apps = Vec::new();
    // Earlier directories override later ones with the same desktop file id.
    let mut seen = HashSet::new();
    for dir in data_dirs() {
        collect(&dir.join("applications"), &dir.join("applications"), &mut seen, &mut apps);
    }
    apps
}

fn data_dirs() -> Vec<PathBuf> {
    let mut out = Vec::new();
    match std::env::var_os("XDG_DATA_HOME") {
        Some(home) => out.push(PathBuf::from(home)),
        None => out.extend(dirs::home_dir().map(|h| h.join(".local/share"))),
    }
    let system = std::env::var("XDG_DATA_DIRS")
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".into());
    out.extend(system.split(':').map(PathBuf::from));
    out.push(PathBuf::from("/var/lib/flatpak/exports/share"));
    out.extend(dirs::home_dir().map(|h| h.join(".local/share/flatpak/exports/share")));
    out
}

fn collect(root: &Path, dir: &Path, seen: &mut HashSet<String>, apps: &mut Vec<AppEntry>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if entry.file_type().is_ok_and(|t| t.is_dir()) {
            collect(root, &path, seen, apps);
            continue;
        }
        if path.extension().is_none_or(|ext| ext != "desktop") {
            continue;
        }
        // Desktop file ids use '-' for subdirectories.
        let id = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('/', "-");
        if !seen.insert(id) {
            continue;
        }
        let Some(parsed) = std::fs::read_to_string(&path)
            .ok()
            .and_then(|source| desktop_entry::parse(&source))
        else {
            continue;
        };
        let mut exec = parsed.exec.into_iter();
        let program = exec.next().unwrap_or_default();
        apps.push(AppEntry {
            name: parsed.name,
            path,
            launch: Run::Spawn {
                program,
                args: exec.collect(),
            },
        });
    }
}
