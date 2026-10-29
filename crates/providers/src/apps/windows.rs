use std::path::{Path, PathBuf};

use greatheights_core::Run;

use super::AppEntry;

// Shell icon extraction needs the Win32 shell APIs; not yet.
pub const CAN_RENDER_ICONS: bool = false;
pub const REVEAL_LABEL: &str = "Show in Explorer";

pub fn render_icon(_app: &Path, _out: &Path) {}

/// Start Menu shortcuts are what Windows itself lists as installed apps.
pub fn scan() -> Vec<AppEntry> {
    let roots = ["ProgramData", "APPDATA"]
        .into_iter()
        .filter_map(std::env::var_os)
        .map(|base| PathBuf::from(base).join(r"Microsoft\Windows\Start Menu\Programs"));

    let mut apps = Vec::new();
    for root in roots {
        collect(&root, &mut apps);
    }
    apps
}

fn collect(dir: &Path, apps: &mut Vec<AppEntry>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if entry.file_type().is_ok_and(|t| t.is_dir()) {
            collect(&path, apps);
        } else if path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("lnk")) {
            let Some(name) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            // Skip the uninstaller and readme shortcuts many installers add.
            let lower = name.to_lowercase();
            if lower.starts_with("uninstall") || lower.contains("readme") {
                continue;
            }
            apps.push(AppEntry {
                name: name.to_owned(),
                path: path.clone(),
                launch: Run::OpenPath(path),
            });
        }
    }
}
