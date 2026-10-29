//! Finding agent commands the way a terminal would.
//!
//! Apps started from the Dock, Finder or a login item get a minimal PATH, so
//! `npx` from Homebrew, Nix or a version manager isn't found. The user's
//! login shell knows the real PATH, so ask it once.

use std::path::PathBuf;
use std::sync::OnceLock;

pub fn login_path() -> Option<String> {
    static PATH: OnceLock<Option<String>> = OnceLock::new();
    PATH.get_or_init(read_login_path).clone()
}

#[cfg(unix)]
fn read_login_path() -> Option<String> {
    use std::process::{Command, Stdio};

    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into());
    let output = Command::new(shell)
        .args(["-l", "-c", "printf %s \"$PATH\""])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .inspect_err(|err| log::warn!("couldn't read the login shell's PATH: {err}"))
        .ok()?;
    let path = String::from_utf8(output.stdout).ok()?;
    (!path.trim().is_empty()).then(|| path.trim().to_owned())
}

#[cfg(not(unix))]
fn read_login_path() -> Option<String> {
    // Windows apps inherit the full user PATH.
    std::env::var("PATH").ok()
}

/// Resolves a bare command name against `path`; anything else is returned
/// unchanged.
pub fn resolve(command: &str, path: Option<&str>) -> PathBuf {
    let bare = !command.contains(std::path::MAIN_SEPARATOR) && !command.contains('/');
    if let (true, Some(path)) = (bare, path) {
        for dir in std::env::split_paths(path) {
            let candidate = dir.join(command);
            if candidate.is_file() {
                return candidate;
            }
        }
    }
    PathBuf::from(command)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_bare_commands_on_path() {
        let resolved = resolve("sh", Some("/nonexistent:/bin"));
        assert_eq!(resolved, PathBuf::from("/bin/sh"));
    }

    #[test]
    fn leaves_paths_and_unknown_commands_alone() {
        assert_eq!(resolve("./agent", Some("/bin")), PathBuf::from("./agent"));
        assert_eq!(resolve("no-such-cmd-42", Some("/bin")), PathBuf::from("no-such-cmd-42"));
    }
}
