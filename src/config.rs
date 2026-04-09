use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AppConfig {
    pub selected_model: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            selected_model: "openai/gpt-4o".to_string(),
        }
    }
}

impl AppConfig {
    pub fn load() -> Self {
        let path = Path::new(".serpantoxide_config");
        if path.exists() {
            if let Ok(content) = fs::read_to_string(path) {
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
        fs::write(".serpantoxide_config", content).map_err(|e| e.to_string())?;
        Ok(())
    }
}
