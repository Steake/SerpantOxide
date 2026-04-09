use std::sync::Arc;
use tokio::sync::RwLock;
use reqwest::Client;
use serde_json::json;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct OpenRouterModel {
    pub id: String,
    pub name: String,
    pub pricing: Option<Pricing>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Pricing {
    pub prompt: String,
    pub completion: String,
}

pub struct LLMState {
    pub model: String,
    pub is_thinking: bool,
    pub status: String,
    pub last_latency_ms: u64,
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub available_models: Vec<OpenRouterModel>,
}

pub struct NativeLLMEngine {
    client: Arc<Client>,
    api_key: String,
    pub state: Arc<RwLock<LLMState>>,
}

impl NativeLLMEngine {
    pub async fn launch() -> Result<Self, String> {
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .build()
            .map_err(|e| e.to_string())?;

        let config = crate::config::AppConfig::load();

        let engine = NativeLLMEngine {
            client: Arc::new(client),
            api_key: std::env::var("OPENROUTER_API_KEY").unwrap_or_else(|_| "MOCK_KEY".to_string()),
            state: Arc::new(RwLock::new(LLMState {
                model: config.selected_model,
                is_thinking: false,
                status: "Idle".to_string(),
                last_latency_ms: 0,
                prompt_tokens: 0,
                completion_tokens: 0,
                available_models: Vec::new(),
            })),
        };

        // Trigger initial model fetch
        let _ = engine.refresh_models().await;

        Ok(engine)
    }

    pub async fn refresh_models(&self) -> Result<(), String> {
        if self.api_key == "MOCK_KEY" {
            let mut s = self.state.write().await;
            s.available_models = vec![
                OpenRouterModel { id: "gpt-4o".into(), name: "GPT-4o".into(), pricing: Some(Pricing { prompt: "0.0".into(), completion: "0.0".into() }) },
                OpenRouterModel { id: "google/gemini-pro-1.5".into(), name: "Gemini 1.5 Pro".into(), pricing: Some(Pricing { prompt: "0.0".into(), completion: "0.0".into() }) },
                OpenRouterModel { id: "mistral/mixtral-8x7b-free".into(), name: "Mixtral 8x7B (Free)".into(), pricing: Some(Pricing { prompt: "0.0".into(), completion: "0.0".into() }) },
            ];
            return Ok(());
        }

        let res = self.client.get("https://openrouter.ai/api/v1/models").send().await.map_err(|e| e.to_string())?;
        if res.status().is_success() {
            let data: serde_json::Value = res.json().await.map_err(|e| e.to_string())?;
            if let Some(models) = data["data"].as_array() {
                let mut parsed = Vec::new();
                for m in models {
                    if let (Some(id), Some(name)) = (m["id"].as_str(), m["name"].as_str()) {
                        parsed.push(OpenRouterModel {
                            id: id.to_string(),
                            name: name.to_string(),
                            pricing: Some(Pricing {
                                prompt: m["pricing"]["prompt"].as_str().unwrap_or("0").to_string(),
                                completion: m["pricing"]["completion"].as_str().unwrap_or("0").to_string(),
                            }),
                        });
                    }
                }
                let mut s = self.state.write().await;
                s.available_models = parsed;
            }
        }
        Ok(())
    }

    pub async fn generate_with_history(&self, messages: Vec<serde_json::Value>) -> Result<String, String> {
        let model = {
            let s = self.state.read().await;
            s.model.clone()
        };

        {
            let mut s = self.state.write().await;
            s.is_thinking = true;
            s.status = "Thinking".to_string();
        }

        let start_time = std::time::Instant::now();

        let payload = json!({
            "model": model,
            "messages": messages
        });

        // We simulate the I/O HTTP block if the key is generic to avoid burning active endpoints during scaffolding.
        if self.api_key == "MOCK_KEY" {
            tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
            return Ok("Reasoning: Simulated history response.\nFINISH".to_string());
        }

        let response = self.client.post("https://openrouter.ai/api/v1/chat/completions")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("HTTP-Referer", "https://github.com/Steake/pentestagent")
            .header("X-Title", "Serpantoxide Orchestrator")
            .json(&payload)
            .send()
            .await.map_err(|e| e.to_string())?;

        if !response.status().is_success() {
            let status = response.status();
            let err_body = response.text().await.unwrap_or_else(|_| "Unknown error body".to_string());
            return Err(format!("Native LLM Completion Error HTTP Code: {} -> {}", status, err_body));
        }

        let duration = start_time.elapsed().as_millis() as u64;
        let body: serde_json::Value = response.json().await.map_err(|e| e.to_string())?;
        
        {
            let mut s = self.state.write().await;
            s.is_thinking = false;
            s.status = "Idle".to_string();
            s.last_latency_ms = duration;
            if let Some(usage) = body.get("usage") {
                s.prompt_tokens = usage["prompt_tokens"].as_u64().unwrap_or(0) as u32;
                s.completion_tokens = usage["completion_tokens"].as_u64().unwrap_or(0) as u32;
            }
        }

        let output = body["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("No completion generation.")
            .to_string();

        Ok(output)
    }
}
