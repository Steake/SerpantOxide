use reqwest::Client;
use serde_json::json;

pub struct NativeWebSearch {
    client: Client,
    api_key: String,
}

impl NativeWebSearch {
    pub fn new(api_key: &str) -> Self {
        Self {
            client: Client::new(),
            api_key: api_key.to_string(),
        }
    }

    pub async fn search(&self, query: &str) -> Result<String, String> {
        let response = self
            .client
            .post("https://api.tavily.com/search")
            .json(&json!({
                "api_key": &self.api_key,
                "query": query,
                "search_depth": "advanced",
                "include_answer": true,
                "max_results": 5,
            }))
            .send()
            .await
            .map_err(|e| e.to_string())?;

        let data: serde_json::Value = response.json().await.map_err(|e| e.to_string())?;

        let mut parts = vec![format!("Search Query: {}\n", query)];

        if let Some(answer) = data.get("answer").and_then(|a| a.as_str()) {
            parts.push(format!("Summary:\n{}\n", answer));
        }

        if let Some(results) = data.get("results").and_then(|r| r.as_array()) {
            parts.push("Sources:".to_string());
            for (i, res) in results.iter().enumerate() {
                let title = res
                    .get("title")
                    .and_then(|t| t.as_str())
                    .unwrap_or("Untitled");
                let url = res.get("url").and_then(|u| u.as_str()).unwrap_or("");
                parts.push(format!("  [{}] {}", i + 1, title));
                parts.push(format!("      {}", url));
            }
        }

        Ok(parts.join("\n"))
    }

    pub fn api_key(&self) -> &str {
        &self.api_key
    }
}
