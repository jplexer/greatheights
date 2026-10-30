use std::path::{Path, PathBuf};

use greatheights_core::Run;
use objc2::rc::autoreleasepool;
use objc2::AnyThread;
use objc2_app_kit::{NSBitmapImageFileType, NSBitmapImageRep, NSWorkspace};
use objc2_foundation::{NSDictionary, NSPoint, NSRect, NSSize, NSString};

use super::AppEntry;

pub const CAN_RENDER_ICONS: bool = true;
pub const REVEAL_LABEL: &str = "Show in Finder";

/// Rendered at 2x for the 32pt result rows.
const ICON_PX: f64 = 64.0;

/// `extra` folders are searched the same way as the standard ones.
pub fn scan(extra: &[PathBuf]) -> Vec<AppEntry> {
    let mut roots = vec![
        PathBuf::from("/Applications"),
        PathBuf::from("/System/Applications"),
        PathBuf::from("/System/Library/CoreServices/Applications"),
    ];
    if let Some(home) = dirs::home_dir() {
        roots.push(home.join("Applications"));
    }
    roots.extend(extra.iter().cloned());

    let mut apps = Vec::new();
    for root in &roots {
        collect(root, 0, &mut apps);
    }
    // Finder lives on its own, outside every applications folder.
    collect_bundle(Path::new("/System/Library/CoreServices/Finder.app"), &mut apps);
    apps
}

/// Walks into plain folders (Utilities, vendor folders like "Adobe Photoshop")
/// but never into bundles themselves.
fn collect(dir: &Path, depth: usize, apps: &mut Vec<AppEntry>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "app") {
            collect_bundle(&path, apps);
        } else if depth < 2 && entry.file_type().is_ok_and(|t| t.is_dir()) {
            collect(&path, depth + 1, apps);
        }
    }
}

fn collect_bundle(path: &Path, apps: &mut Vec<AppEntry>) {
    let Some(name) = path.file_stem().and_then(|s| s.to_str()) else {
        return;
    };
    apps.push(AppEntry {
        name: name.to_owned(),
        path: path.to_owned(),
        launch: Run::OpenPath(path.to_owned()),
    });
}

pub fn render_icon(app: &Path, out: &Path) {
    let Some(app) = app.to_str() else { return };
    autoreleasepool(|_| {
        let image = NSWorkspace::sharedWorkspace().iconForFile(&NSString::from_str(app));
        let mut rect = NSRect::new(NSPoint::ZERO, NSSize::new(ICON_PX, ICON_PX));
        let Some(cg_image) =
            (unsafe { image.CGImageForProposedRect_context_hints(&mut rect, None, None) })
        else {
            return;
        };
        let rep = NSBitmapImageRep::initWithCGImage(NSBitmapImageRep::alloc(), &cg_image);
        let Some(png) = (unsafe {
            rep.representationUsingType_properties(NSBitmapImageFileType::PNG, &NSDictionary::new())
        }) else {
            return;
        };
        if let Err(err) = std::fs::write(out, png.to_vec()) {
            log::warn!("can't write icon {}: {err}", out.display());
        }
    });
}
