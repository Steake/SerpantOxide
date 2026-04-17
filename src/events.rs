use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum UiEvent {
    Log {
        message: String,
    },
    Checklist {
        completed: Vec<String>,
        remaining: Vec<String>,
    },
    CrewComplete {
        summary: String,
    },
}

impl UiEvent {
    pub fn log<S: Into<String>>(message: S) -> Self {
        Self::Log {
            message: message.into(),
        }
    }

    pub fn serialize(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| {
            "{\"kind\":\"log\",\"message\":\"Failed to serialize UI event\"}".to_string()
        })
    }
}
