use std::collections::HashMap;
use std::fs;
use std::sync::Arc;
use tokio::sync::RwLock;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone)]
pub struct Note {
    pub category: String,
    pub payload: String,
}

pub struct NotesEngine {
    store: Arc<RwLock<HashMap<String, Vec<Note>>>>,
    backing_file: String,
}

impl NotesEngine {
    pub async fn launch() -> Result<Self, String> {
        let path = "loot/notes.json";
        let _ = fs::create_dir_all("loot");
        
        let store = if let Ok(data) = fs::read_to_string(path) {
            serde_json::from_str(&data).unwrap_or_else(|_| HashMap::new())
        } else {
            HashMap::new()
        };

        Ok(NotesEngine {
            store: Arc::new(RwLock::new(store)),
            backing_file: path.to_string(),
        })
    }

    pub async fn execute(&self, action: &str, category: &str, payload: &str) -> String {
        let mut map = self.store.write().await;
        
        if action == "insert" {
            let entry = map.entry(category.to_string()).or_insert_with(Vec::new);
            entry.push(Note { category: category.to_string(), payload: payload.to_string() });
            
            // Sync flushing to disk (in real usage we background channel this)
            let d = serde_json::to_string(&*map).unwrap_or_else(|_| "{}".to_string());
            let _ = fs::write(&self.backing_file, d);
            
            return format!("Successfully tracked Note inside native Rust Mapping: {category}");
        } else if action == "read" {
            if let Some(notes) = map.get(category) {
                return serde_json::to_string(notes).unwrap_or_else(|_| "[]".to_string());
            }
            return "[]".to_string();
        }
        
        "Unknown action execution pattern".to_string()
    }

    pub async fn list_categories(&self) -> Vec<(String, usize)> {
        let map = self.store.read().await;
        map.iter().map(|(k, v)| (k.clone(), v.len())).collect()
    }

    pub async fn get_notes_by_category(&self, category: &str) -> Vec<Note> {
        let map = self.store.read().await;
        map.get(category).cloned().unwrap_or_default()
    }
}
