use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AppConfig {
    #[serde(default = "default_selected_model")]
    pub selected_model: String,
    #[serde(default = "default_last_target")]
    pub last_target: String,
}

fn default_selected_model() -> String {
    "openai/gpt-4o".to_string()
}

fn default_last_target() -> String {
    "None".to_string()
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            selected_model: default_selected_model(),
            last_target: default_last_target(),
        }
    }
}

impl AppConfig {
    fn path() -> PathBuf {
        runtime_home_dir().join(".serpantoxide_config")
    }

    pub fn load() -> Self {
        let path = Self::path();
        if path.exists() {
            if let Ok(content) = fs::read_to_string(&path) {
                if let Ok(config) = serde_json::from_str::<AppConfig>(&content) {
                    return config;
                }
            }
        }

        let mut config = Self::default();
        // Fallback to Env if present
        if let Ok(env_model) = std::env::var("LLM_MODEL") {
            config.selected_model = env_model;
        }
        config
    }

    pub fn save(&self) -> Result<(), String> {
        let content = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        fs::write(Self::path(), content).map_err(|e| e.to_string())?;
        Ok(())
    }
}

fn runtime_home_dir() -> PathBuf {
    if let Ok(explicit_home) = std::env::var("SERPANTOXIDE_HOME") {
        let path = PathBuf::from(explicit_home);
        if !path.as_os_str().is_empty() {
            return path;
        }
    }

    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(contents_dir) = exe_path
            .parent()
            .and_then(|macos_dir| macos_dir.parent())
        {
            let bundled_runtime = contents_dir.join("Resources").join("runtime");
            if bundled_runtime.exists() {
                return bundled_runtime;
            }
        }
    }

    std::env::current_dir().unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")))
}
