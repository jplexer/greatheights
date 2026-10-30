//! Launch at login. The login item itself is the source of truth (a
//! LaunchAgent on macOS, an autostart entry on Linux, a Run key on
//! Windows), so it can't drift from a setting.

use std::path::PathBuf;

/// Passed by the login item so the app starts without showing the bar.
pub const BACKGROUND_ARG: &str = "--background";

#[cfg(any(target_os = "macos", all(unix, not(target_os = "macos"))))]
const ID: &str = "dev.greatheights.launcher";

/// The executable to start at login, if this build can be one.
pub fn executable() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|err| err.to_string())?;
    if cfg!(target_os = "macos") && !exe.to_string_lossy().contains(".app/Contents/MacOS/") {
        return Err("Available once Greatheights is installed as an app.".into());
    }
    Ok(exe)
}

#[cfg(target_os = "macos")]
fn agent_path() -> Option<PathBuf> {
    Some(dirs::home_dir()?.join(format!("Library/LaunchAgents/{ID}.plist")))
}

#[cfg(target_os = "macos")]
pub fn is_enabled() -> bool {
    agent_path().is_some_and(|path| path.exists())
}

#[cfg(target_os = "macos")]
pub fn set(enabled: bool) -> Result<(), String> {
    let path = agent_path().ok_or("No home directory.")?;
    if !enabled {
        return match std::fs::remove_file(&path) {
            Err(err) if err.kind() != std::io::ErrorKind::NotFound => Err(err.to_string()),
            _ => Ok(()),
        };
    }
    let exe = executable()?;
    let plist = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{ID}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{}</string>
        <string>{BACKGROUND_ARG}</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>ProcessType</key>
    <string>Interactive</string>
</dict>
</plist>
"#,
        xml_escape(&exe.to_string_lossy())
    );
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|err| err.to_string())?;
    }
    std::fs::write(&path, plist).map_err(|err| err.to_string())
}

#[cfg(target_os = "macos")]
fn xml_escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

#[cfg(all(unix, not(target_os = "macos")))]
fn entry_path() -> Option<PathBuf> {
    Some(dirs::config_dir()?.join(format!("autostart/{ID}.desktop")))
}

#[cfg(all(unix, not(target_os = "macos")))]
pub fn is_enabled() -> bool {
    entry_path().is_some_and(|path| path.exists())
}

#[cfg(all(unix, not(target_os = "macos")))]
pub fn set(enabled: bool) -> Result<(), String> {
    let path = entry_path().ok_or("No config directory.")?;
    if !enabled {
        return match std::fs::remove_file(&path) {
            Err(err) if err.kind() != std::io::ErrorKind::NotFound => Err(err.to_string()),
            _ => Ok(()),
        };
    }
    let exe = executable()?;
    let entry = format!(
        "[Desktop Entry]\nType=Application\nName=Greatheights\nExec=\"{}\" {BACKGROUND_ARG}\n\
         X-GNOME-Autostart-enabled=true\n",
        exe.display()
    );
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|err| err.to_string())?;
    }
    std::fs::write(&path, entry).map_err(|err| err.to_string())
}

#[cfg(windows)]
const RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";

#[cfg(windows)]
pub fn is_enabled() -> bool {
    std::process::Command::new("reg")
        .args(["query", RUN_KEY, "/v", "Greatheights"])
        .output()
        .is_ok_and(|output| output.status.success())
}

#[cfg(windows)]
pub fn set(enabled: bool) -> Result<(), String> {
    let mut command = std::process::Command::new("reg");
    if enabled {
        let exe = executable()?;
        let value = format!("\"{}\" {BACKGROUND_ARG}", exe.display());
        command.args(["add", RUN_KEY, "/v", "Greatheights", "/d", &value, "/f"]);
    } else if is_enabled() {
        command.args(["delete", RUN_KEY, "/v", "Greatheights", "/f"]);
    } else {
        return Ok(());
    }
    let output = command.output().map_err(|err| err.to_string())?;
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_owned())
    }
}
