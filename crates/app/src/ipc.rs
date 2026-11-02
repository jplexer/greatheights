//! Commands from other launches of the app, over a Unix socket.
//!
//! `greatheights --toggle` toggles a running launcher, for desktops where
//! apps can't grab global shortcuts (Wayland): bind a compositor shortcut
//! to it instead. Launching the app again shows the running launcher rather
//! than starting a second one.

use std::path::PathBuf;

/// A command for the running launcher.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    Toggle,
    Show,
}

#[cfg_attr(not(unix), allow(dead_code))]
impl Command {
    fn as_str(self) -> &'static str {
        match self {
            Command::Toggle => "toggle",
            Command::Show => "show",
        }
    }

    fn parse(line: &str) -> Option<Self> {
        match line.trim() {
            "toggle" => Some(Command::Toggle),
            "show" => Some(Command::Show),
            _ => None,
        }
    }
}

/// Where the running launcher listens.
#[cfg_attr(not(unix), allow(dead_code))]
fn socket_path() -> PathBuf {
    // Per user and per session where there is one, as on Linux.
    let dir = dirs::runtime_dir()
        .or_else(dirs::cache_dir)
        .unwrap_or_else(std::env::temp_dir);
    let name = if cfg!(debug_assertions) {
        "greatheights-debug.sock"
    } else {
        "greatheights.sock"
    };
    dir.join(name)
}

#[cfg(unix)]
mod imp {
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::time::Duration;

    use gpui::App;

    use super::{Command, socket_path};
    use crate::window;

    /// Sends `command` to a running launcher. False if none is running.
    pub fn send(command: Command) -> bool {
        let Ok(mut stream) = UnixStream::connect(socket_path()) else {
            return false;
        };
        let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
        writeln!(stream, "{}", command.as_str()).is_ok()
    }

    /// Whether a launcher is running, without disturbing it.
    pub fn is_running() -> bool {
        UnixStream::connect(socket_path()).is_ok()
    }

    /// Listens for commands from later launches. Call once nothing answers
    /// [`send`], so a leftover socket file is stale.
    pub fn listen(cx: &mut App) {
        let path = socket_path();
        let _ = std::fs::remove_file(&path);
        let listener = match UnixListener::bind(&path) {
            Ok(listener) => listener,
            Err(err) => {
                log::warn!("can't listen on {}: {err}", path.display());
                return;
            }
        };
        let (tx, rx) = async_channel::unbounded();
        let spawned = std::thread::Builder::new()
            .name("ipc".into())
            .spawn(move || {
                for stream in listener.incoming().flatten() {
                    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                    let mut line = String::new();
                    if BufReader::new(stream).read_line(&mut line).is_err() {
                        continue;
                    }
                    match Command::parse(&line) {
                        Some(command) => {
                            if tx.send_blocking(command).is_err() {
                                return;
                            }
                        }
                        // An empty line is just `is_running` checking in.
                        None if line.trim().is_empty() => {}
                        None => log::warn!("unknown command: {}", line.trim()),
                    }
                }
            });
        if let Err(err) = spawned {
            log::warn!("can't start the command listener: {err}");
            return;
        }
        cx.spawn(async move |cx| {
            while let Ok(command) = rx.recv().await {
                let run = match command {
                    Command::Toggle => window::toggle,
                    Command::Show => window::show,
                };
                if cx.update(run).is_err() {
                    break;
                }
            }
        })
        .detach();
    }
}

/// Windows has global shortcuts and Explorer reopens apps itself, so there
/// is no socket there; every launch runs on its own.
#[cfg(not(unix))]
mod imp {
    use super::Command;

    pub fn send(_: Command) -> bool {
        false
    }

    pub fn is_running() -> bool {
        false
    }

    pub fn listen(_: &mut gpui::App) {}
}

pub use imp::{is_running, listen, send};

#[cfg(test)]
mod tests {
    use super::Command;

    #[test]
    fn commands_round_trip() {
        for command in [Command::Toggle, Command::Show] {
            assert_eq!(Command::parse(&format!("{}\n", command.as_str())), Some(command));
        }
        assert_eq!(Command::parse("quit"), None);
    }
}
