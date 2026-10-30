//! System commands: lock, sleep, restart and friends.

use std::sync::atomic::{AtomicBool, Ordering};

use greatheights_core::{Action, Icon, Item, Matcher, Provider, Run, Settings};

struct Command {
    title: &'static str,
    keywords: &'static [&'static str],
    /// Disruptive commands need a longer prefix, so a short query meant for
    /// an app ("re" for Reminders) can't put Restart on top, and a second
    /// Enter to run.
    disruptive: bool,
    /// Greatheights' own commands stay available when system commands are
    /// turned off, so Settings can always be reached.
    own: bool,
    run: Run,
}

pub struct SystemProvider {
    commands: Vec<Command>,
    enabled: AtomicBool,
}

impl SystemProvider {
    pub fn new() -> Self {
        let mut commands = platform_commands();
        commands.push(Command {
            title: "Greatheights Settings",
            keywords: &["settings", "preferences", "config"],
            disruptive: false,
            own: true,
            run: Run::OpenSettings,
        });
        commands.push(Command {
            title: "Quit Greatheights",
            keywords: &["exit", "close"],
            disruptive: false,
            own: true,
            run: Run::Quit,
        });
        Self {
            commands,
            enabled: AtomicBool::new(true),
        }
    }
}

impl Default for SystemProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl Provider for SystemProvider {
    fn search(&self, query: &str, matcher: &mut Matcher) -> Vec<Item> {
        let query = query.to_lowercase();
        let enabled = self.enabled.load(Ordering::Relaxed);
        self.commands
            .iter()
            .filter(|command| enabled || command.own)
            .filter(|command| {
                let min_len = if command.disruptive { 3 } else { 2 };
                query.chars().count() >= min_len && matches_prefix(command, &query)
            })
            .map(|command| Item {
                id: format!("system:{}", command.title),
                title: command.title.to_owned(),
                subtitle: Some("System".into()),
                icon: Icon::Glyph("⏻"),
                actions: vec![{
                    let action = Action::new("Run", command.run.clone());
                    if command.disruptive {
                        action.confirmed()
                    } else {
                        action
                    }
                }],
                score: matcher.score(command.title).unwrap_or(0),
            })
            .collect()
    }

    fn configure(&self, settings: &Settings) {
        self.enabled.store(settings.system_commands, Ordering::Relaxed);
    }
}

/// True if the query starts any word of the title or a keyword.
fn matches_prefix(command: &Command, query: &str) -> bool {
    let title = command.title.to_lowercase();
    title.starts_with(query)
        || title.split_whitespace().any(|word| word.starts_with(query))
        || command.keywords.iter().any(|k| k.starts_with(query))
}

fn spawn(program: &str, args: &[&str]) -> Run {
    Run::Spawn {
        program: program.into(),
        args: args.iter().map(|a| a.to_string()).collect(),
    }
}

#[cfg(target_os = "macos")]
fn platform_commands() -> Vec<Command> {
    let script = |source: &str| spawn("osascript", &["-e", source]);
    vec![
        Command {
            title: "Lock Screen",
            keywords: &["lock"],
            disruptive: false,
            own: false,
            // Locks when "require password after sleep" is immediate, the default.
            run: spawn("pmset", &["displaysleepnow"]),
        },
        Command {
            title: "Sleep",
            keywords: &["suspend"],
            disruptive: false,
            own: false,
            run: spawn("pmset", &["sleepnow"]),
        },
        Command {
            title: "Restart",
            keywords: &["reboot"],
            disruptive: true,
            own: false,
            run: script("tell application \"System Events\" to restart"),
        },
        Command {
            title: "Shut Down",
            keywords: &["shutdown", "power off"],
            disruptive: true,
            own: false,
            run: script("tell application \"System Events\" to shut down"),
        },
        Command {
            title: "Log Out",
            keywords: &["logout", "sign out"],
            disruptive: true,
            own: false,
            run: script("tell application \"System Events\" to log out"),
        },
        Command {
            title: "Empty Trash",
            keywords: &["trash", "bin"],
            disruptive: false,
            own: false,
            run: script("tell application \"Finder\" to empty trash"),
        },
        Command {
            title: "Toggle Dark Mode",
            keywords: &["dark", "light", "appearance", "theme"],
            disruptive: false,
            own: false,
            run: script(
                "tell application \"System Events\" to tell appearance preferences \
                 to set dark mode to not dark mode",
            ),
        },
    ]
}

#[cfg(target_os = "windows")]
fn platform_commands() -> Vec<Command> {
    vec![
        Command {
            title: "Lock Screen",
            keywords: &["lock"],
            disruptive: false,
            own: false,
            run: spawn("rundll32.exe", &["user32.dll,LockWorkStation"]),
        },
        Command {
            title: "Sleep",
            keywords: &["suspend"],
            disruptive: false,
            own: false,
            run: spawn("rundll32.exe", &["powrprof.dll,SetSuspendState", "0,1,0"]),
        },
        Command {
            title: "Restart",
            keywords: &["reboot"],
            disruptive: true,
            own: false,
            run: spawn("shutdown", &["/r", "/t", "0"]),
        },
        Command {
            title: "Shut Down",
            keywords: &["shutdown", "power off"],
            disruptive: true,
            own: false,
            run: spawn("shutdown", &["/s", "/t", "0"]),
        },
        Command {
            title: "Log Out",
            keywords: &["logout", "sign out"],
            disruptive: true,
            own: false,
            run: spawn("shutdown", &["/l"]),
        },
    ]
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn platform_commands() -> Vec<Command> {
    let mut commands = vec![
        Command {
            title: "Lock Screen",
            keywords: &["lock"],
            disruptive: false,
            own: false,
            run: spawn("loginctl", &["lock-session"]),
        },
        Command {
            title: "Sleep",
            keywords: &["suspend"],
            disruptive: false,
            own: false,
            run: spawn("systemctl", &["suspend"]),
        },
        Command {
            title: "Restart",
            keywords: &["reboot"],
            disruptive: true,
            own: false,
            run: spawn("systemctl", &["reboot"]),
        },
        Command {
            title: "Shut Down",
            keywords: &["shutdown", "power off"],
            disruptive: true,
            own: false,
            run: spawn("systemctl", &["poweroff"]),
        },
    ];
    if let Ok(session) = std::env::var("XDG_SESSION_ID") {
        commands.push(Command {
            title: "Log Out",
            keywords: &["logout", "sign out"],
            disruptive: true,
            own: false,
            run: spawn("loginctl", &["terminate-session", &session]),
        });
    }
    commands
}

#[cfg(test)]
mod tests {
    use super::*;

    fn titles(query: &str) -> Vec<String> {
        SystemProvider::new()
            .search(query, &mut Matcher::new(query))
            .into_iter()
            .map(|item| item.title)
            .collect()
    }

    #[test]
    fn short_queries_never_match_disruptive_commands() {
        assert!(!titles("re").contains(&"Restart".to_string()));
        assert!(titles("res").contains(&"Restart".to_string()));
    }

    #[test]
    fn matches_keywords_and_later_words() {
        assert!(titles("reboot").contains(&"Restart".to_string()));
        assert!(titles("lock").contains(&"Lock Screen".to_string()));
        assert!(titles("quit").contains(&"Quit Greatheights".to_string()));
    }

    #[test]
    fn needs_a_word_prefix() {
        // "ep" is inside "Sleep" but starts no word.
        assert!(!titles("ep").contains(&"Sleep".to_string()));
    }
}
