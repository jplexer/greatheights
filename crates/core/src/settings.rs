//! User settings, read from a JSON file in the platform config directory.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Deserialize)]
pub struct Settings {
    /// The ACP agent Quick Ask talks to. There is deliberately no default:
    /// asking only works once the user picks an agent here.
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
    #[serde(default)]
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
}

/// Written when the settings file doesn't exist yet. Nothing is enabled;
/// the examples (from the ACP registry) are there to copy into `agent`.
const TEMPLATE: &str = r#"{
  "agent": null,

  "agent_examples": [
    {
      "name": "Claude",
      "command": "npx",
      "args": ["-y", "@agentclientprotocol/claude-agent-acp@latest"]
    },
    {
      "name": "Codex",
      "command": "npx",
      "args": ["-y", "@agentclientprotocol/codex-acp@latest"]
    },
    {
      "name": "Gemini",
      "command": "npx",
      "args": ["-y", "@google/gemini-cli@latest", "--acp"]
    }
  ]
}
"#;

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

    /// Creates the settings file from the template if it's missing, so
    /// "open settings" always has something to open.
    pub fn ensure_file(path: &Path) -> std::io::Result<()> {
        if path.exists() {
            return Ok(());
        }
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, TEMPLATE)
    }
}

pub fn settings_path(config_dir: &Path) -> PathBuf {
    config_dir.join("settings.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn template_has_no_agent() {
        let settings: Settings = serde_json::from_str(TEMPLATE).unwrap();
        assert!(settings.agent.is_none());
    }

    #[test]
    fn template_examples_parse_as_agents() {
        let value: serde_json::Value = serde_json::from_str(TEMPLATE).unwrap();
        let examples: Vec<AgentConfig> =
            serde_json::from_value(value["agent_examples"].clone()).unwrap();
        assert_eq!(examples[0].display_name(), "Claude");
    }

    #[test]
    fn name_falls_back_to_command() {
        let agent: AgentConfig =
            serde_json::from_str(r#"{"command": "/opt/bin/my-agent"}"#).unwrap();
        assert_eq!(agent.display_name(), "my-agent");
    }
}
