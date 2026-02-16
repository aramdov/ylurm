use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Deserialize, Clone)]
#[serde(default)]
pub struct Config {
    pub general: GeneralConfig,
    pub keybindings: KeyBindings,
    pub display: DisplayConfig,
    pub remote: RemoteConfig,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(default)]
pub struct GeneralConfig {
    /// Refresh interval in seconds
    pub refresh_interval: u64,
    /// Default squeue arguments
    pub squeue_args: Vec<String>,
    /// Show all users' jobs (false = only yours)
    pub all_users: bool,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(default)]
pub struct KeyBindings {
    pub quit: String,
    pub up: String,
    pub down: String,
    pub top: String,
    pub bottom: String,
    pub toggle_logs: String,
    pub cancel_job: String,
    pub refresh: String,
    pub ssh_view_log: String,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(default)]
pub struct DisplayConfig {
    /// Columns to display in the job list
    pub columns: Vec<String>,
    /// Color scheme: "default", "minimal", "solarized"
    pub theme: String,
    /// Show job details panel
    pub show_details: bool,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(default)]
pub struct RemoteConfig {
    /// Enable SSH-based log reading for node-local paths
    pub ssh_enabled: bool,
    /// Path mappings: node-local path prefix -> NFS-accessible path
    /// e.g., { "/raid/asds/" = "/auto/home/asds/" }
    pub path_mappings: HashMap<String, String>,
    /// SSH timeout in seconds
    pub ssh_timeout: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            general: GeneralConfig::default(),
            keybindings: KeyBindings::default(),
            display: DisplayConfig::default(),
            remote: RemoteConfig::default(),
        }
    }
}

impl Default for GeneralConfig {
    fn default() -> Self {
        Self {
            refresh_interval: 2,
            squeue_args: vec![],
            all_users: false,
        }
    }
}

impl Default for KeyBindings {
    fn default() -> Self {
        Self {
            quit: "q".to_string(),
            up: "k".to_string(),
            down: "j".to_string(),
            top: "g".to_string(),
            bottom: "G".to_string(),
            toggle_logs: "l".to_string(),
            cancel_job: "x".to_string(),
            refresh: "r".to_string(),
            ssh_view_log: "s".to_string(),
        }
    }
}

impl Default for DisplayConfig {
    fn default() -> Self {
        Self {
            columns: vec![
                "JobID".into(),
                "Partition".into(),
                "Name".into(),
                "User".into(),
                "State".into(),
                "Time".into(),
                "Nodes".into(),
                "NodeList".into(),
            ],
            theme: "default".into(),
            show_details: true,
        }
    }
}

impl Default for RemoteConfig {
    fn default() -> Self {
        Self {
            ssh_enabled: true,
            path_mappings: HashMap::new(),
            ssh_timeout: 5,
        }
    }
}

impl Config {
    pub fn load() -> Self {
        let config_path = Self::config_path();
        if config_path.exists() {
            match fs::read_to_string(&config_path) {
                Ok(contents) => match toml::from_str(&contents) {
                    Ok(config) => return config,
                    Err(e) => eprintln!("Warning: invalid config ({}), using defaults", e),
                },
                Err(e) => eprintln!("Warning: can't read config ({}), using defaults", e),
            }
        }
        Config::default()
    }

    pub fn config_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("~/.config"))
            .join("ylurm")
            .join("config.toml")
    }

    /// Generate a default config file with comments
    pub fn generate_default() -> String {
        r#"# ylurm configuration
# Place this file at ~/.config/ylurm/config.toml

[general]
# Refresh interval in seconds
refresh_interval = 2
# Show all users' jobs (false = only yours)
all_users = false
# Extra squeue arguments
# squeue_args = ["--partition=a100"]

[keybindings]
quit = "q"
up = "k"
down = "j"
top = "g"
bottom = "G"
toggle_logs = "l"
cancel_job = "x"
refresh = "r"
ssh_view_log = "s"

[display]
theme = "default"
show_details = true
columns = ["JobID", "Partition", "Name", "User", "State", "Time", "Nodes", "NodeList"]

[remote]
# SSH to compute nodes to read node-local log files
ssh_enabled = true
ssh_timeout = 5

# Map node-local paths to NFS-accessible paths (avoids SSH when possible)
# [remote.path_mappings]
# "/raid/asds/" = "/nfs/a100/asds/"
"#
        .to_string()
    }
}
