use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct Note {
    #[serde(default)]
    pub key: String,
    pub category: String,
    pub payload: String,
    #[serde(default)]
    pub target: Option<String>,
    #[serde(default)]
    pub metadata: Value,
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

    pub async fn list_categories(&self) -> Vec<(String, usize)> {
        let map = self.store.read().await;
        map.iter().map(|(k, v)| (k.clone(), v.len())).collect()
    }

    pub async fn get_notes_by_category(&self, category: &str) -> Vec<Note> {
        let map = self.store.read().await;
        map.get(category).cloned().unwrap_or_default()
    }

    pub async fn upsert_note(
        &self,
        key: &str,
        category: &str,
        payload: &str,
        target: Option<String>,
        metadata: Value,
    ) -> Result<(), String> {
        let mut map = self.store.write().await;
        let entry = map.entry(category.to_string()).or_insert_with(Vec::new);

        if let Some(existing) = entry.iter_mut().find(|note| note.key == key) {
            existing.payload = payload.to_string();
            existing.target = target;
            existing.metadata = metadata;
        } else {
            entry.push(Note {
                key: key.to_string(),
                category: category.to_string(),
                payload: payload.to_string(),
                target,
                metadata,
            });
        }

        self.flush_locked(&map)
    }

    pub async fn read_note(&self, key: &str) -> Option<Note> {
        let map = self.store.read().await;
        map.values()
            .flat_map(|notes| notes.iter())
            .find(|note| note.key == key)
            .cloned()
    }

    pub async fn list_note_keys(&self) -> Vec<String> {
        let map = self.store.read().await;
        let mut keys = map
            .values()
            .flat_map(|notes| notes.iter())
            .filter(|note| !note.key.is_empty())
            .map(|note| note.key.clone())
            .collect::<Vec<_>>();
        keys.sort();
        keys
    }

    fn flush_locked(&self, map: &HashMap<String, Vec<Note>>) -> Result<(), String> {
        let data = serde_json::to_string_pretty(map).map_err(|e| e.to_string())?;
        fs::write(&self.backing_file, data).map_err(|e| e.to_string())
    }
}
