//! User settings, stored as JSON in the platform config directory and
//! edited through the settings window.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    /// The ACP agent Quick Ask talks to. There is deliberately no default:
    /// asking only works once the user picks an agent.
    #[serde(default)]
    pub agent: Option<AgentConfig>,
}

/// How to launch an Agent Client Protocol agent.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentConfig {
    /// Shown in the UI; defaults to the command name.
    #[serde(default)]
    pub name: Option<String>,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
}

impl AgentConfig {
    pub fn display_name(&self) -> String {
        self.name.clone().unwrap_or_else(|| {
            Path::new(&self.command)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| self.command.clone())
        })
    }

    /// Whether this launches the same agent as `other`, ignoring the name.
    pub fn same_launch(&self, other: &AgentConfig) -> bool {
        self.command == other.command && self.args == other.args && self.env == other.env
    }
}

/// An agent offered in the settings window, launched the way the ACP
/// registry describes it.
pub struct AgentPreset {
    pub name: &'static str,
    /// Which account the agent signs in with.
    pub sign_in: &'static str,
    command: &'static str,
    args: &'static [&'static str],
}

impl AgentPreset {
    pub fn config(&self) -> AgentConfig {
        AgentConfig {
            name: Some(self.name.to_owned()),
            command: self.command.to_owned(),
            args: self.args.iter().map(|a| a.to_string()).collect(),
            env: BTreeMap::new(),
        }
    }
}

pub const AGENT_PRESETS: &[AgentPreset] = &[
    AgentPreset {
        name: "Claude",
        sign_in: "Uses your Claude Code login",
        command: "npx",
        args: &["-y", "@agentclientprotocol/claude-agent-acp@latest"],
    },
    AgentPreset {
        name: "Codex",
        sign_in: "Uses your Codex / ChatGPT login",
        command: "npx",
        args: &["-y", "@agentclientprotocol/codex-acp@latest"],
    },
    AgentPreset {
        name: "Gemini",
        sign_in: "Uses your Gemini CLI login",
        command: "npx",
        args: &["-y", "@google/gemini-cli@latest", "--acp"],
    },
];

impl Settings {
    pub fn load(path: &Path) -> Self {
        let Ok(text) = std::fs::read_to_string(path) else {
            return Self::default();
        };
        serde_json::from_str(&text).unwrap_or_else(|err| {
            log::warn!("ignoring invalid settings in {}: {err}", path.display());
            Self::default()
        })
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let json = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        std::fs::write(path, json + "\n")
    }
}

pub fn settings_path(config_dir: &Path) -> PathBuf {
    config_dir.join("settings.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_means_no_agent() {
        let settings = Settings::load(Path::new("/nonexistent/settings.json"));
        assert!(settings.agent.is_none());
    }

    #[test]
    fn round_trips_through_disk() {
        let path = std::env::temp_dir().join("greatheights-settings-test.json");
        let settings = Settings {
            agent: Some(AGENT_PRESETS[0].config()),
        };
        settings.save(&path).unwrap();
        assert_eq!(Settings::load(&path), settings);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn presets_match_regardless_of_name() {
        let mut renamed = AGENT_PRESETS[1].config();
        renamed.name = Some("My Codex".into());
        assert!(renamed.same_launch(&AGENT_PRESETS[1].config()));
        assert!(!renamed.same_launch(&AGENT_PRESETS[0].config()));
    }

    #[test]
    fn name_falls_back_to_command() {
        let agent: AgentConfig =
            serde_json::from_str(r#"{"command": "/opt/bin/my-agent"}"#).unwrap();
        assert_eq!(agent.display_name(), "my-agent");
    }
}
