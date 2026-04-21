use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;
use tokio::sync::RwLock;

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

#[derive(Clone, Debug, Default)]
pub struct ChatResponse {
    pub content: String,
    pub tool_calls: Vec<ToolCall>,
    pub usage: Option<Usage>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Usage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
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

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct LlmTelemetrySnapshot {
    pub model: String,
    pub status: String,
    pub is_thinking: bool,
    pub last_latency_ms: u64,
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub available_models: Vec<OpenRouterModel>,
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
                OpenRouterModel {
                    id: "gpt-4o".into(),
                    name: "GPT-4o".into(),
                    pricing: Some(Pricing {
                        prompt: "0.0".into(),
                        completion: "0.0".into(),
                    }),
                },
                OpenRouterModel {
                    id: "google/gemini-pro-1.5".into(),
                    name: "Gemini 1.5 Pro".into(),
                    pricing: Some(Pricing {
                        prompt: "0.0".into(),
                        completion: "0.0".into(),
                    }),
                },
                OpenRouterModel {
                    id: "mistral/mixtral-8x7b-free".into(),
                    name: "Mixtral 8x7B (Free)".into(),
                    pricing: Some(Pricing {
                        prompt: "0.0".into(),
                        completion: "0.0".into(),
                    }),
                },
            ];
            return Ok(());
        }

        let res = match self
            .client
            .get("https://openrouter.ai/api/v1/models")
            .send()
            .await
        {
            Ok(res) => res,
            Err(e) => {
                let message = format!("Model list fetch failed: {}", e);
                self.set_status_message(message.clone()).await;
                return Err(message);
            }
        };
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
                                completion: m["pricing"]["completion"]
                                    .as_str()
                                    .unwrap_or("0")
                                    .to_string(),
                            }),
                        });
                    }
                }
                let mut s = self.state.write().await;
                s.available_models = parsed;
            }
        } else {
            let status = res.status();
            let err_body = res.text().await.unwrap_or_default();
            let message = summarize_provider_error(status.as_u16(), &err_body);
            self.set_status_message(format!("Model list error: {}", message))
                .await;
            return Err(message);
        }
        Ok(())
    }

    pub async fn set_model(&self, model_id: String) -> Result<(), String> {
        {
            let mut state = self.state.write().await;
            state.model = model_id.clone();
        }

        let mut config = crate::config::AppConfig::load();
        config.selected_model = model_id;
        config.save()
    }

    pub async fn telemetry_snapshot(&self) -> LlmTelemetrySnapshot {
        let state = self.state.read().await;
        LlmTelemetrySnapshot {
            model: state.model.clone(),
            status: state.status.clone(),
            is_thinking: state.is_thinking,
            last_latency_ms: state.last_latency_ms,
            prompt_tokens: state.prompt_tokens,
            completion_tokens: state.completion_tokens,
            available_models: state.available_models.clone(),
        }
    }

    pub async fn generate_with_history(
        &self,
        messages: Vec<serde_json::Value>,
    ) -> Result<String, String> {
        let response = self.generate_chat(None, messages, None).await?;
        Ok(response.content)
    }

    pub async fn generate_with_tools(
        &self,
        system_prompt: &str,
        messages: Vec<serde_json::Value>,
        tools: Vec<serde_json::Value>,
    ) -> Result<ChatResponse, String> {
        self.generate_chat(Some(system_prompt), messages, Some(tools))
            .await
    }

    async fn generate_chat(
        &self,
        system_prompt: Option<&str>,
        mut messages: Vec<serde_json::Value>,
        tools: Option<Vec<serde_json::Value>>,
    ) -> Result<ChatResponse, String> {
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

        if let Some(prompt) = system_prompt {
            messages.insert(0, json!({"role": "system", "content": prompt}));
        }

        if self.api_key == "MOCK_KEY" {
            let mock = self.mock_response(&messages, tools.as_ref());
            self.finish_request(start_time.elapsed().as_millis() as u64, mock.usage.clone())
                .await;
            return Ok(mock);
        }

        let mut payload = json!({
            "model": model,
            "messages": messages,
        });

        if let Some(tool_defs) = tools {
            payload["tools"] = serde_json::Value::Array(tool_defs);
            payload["tool_choice"] = json!("auto");
        }

        let response = match self
            .client
            .post("https://openrouter.ai/api/v1/chat/completions")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("HTTP-Referer", "https://github.com/Steake/pentestagent")
            .header("X-Title", "Serpantoxide Orchestrator")
            .json(&payload)
            .send()
            .await
        {
            Ok(response) => response,
            Err(e) => {
                let detail = format!("Provider request failed: {}", e);
                self.fail_request(start_time.elapsed().as_millis() as u64, &detail)
                    .await;
                return Err(detail);
            }
        };

        if !response.status().is_success() {
            let status = response.status();
            let err_body = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error body".to_string());
            let message = summarize_provider_error(status.as_u16(), &err_body);
            self.fail_request(start_time.elapsed().as_millis() as u64, &message)
                .await;
            return Err(message);
        }

        let duration = start_time.elapsed().as_millis() as u64;
        let body: serde_json::Value = response.json().await.map_err(|e| e.to_string())?;
        let usage = body.get("usage").map(|usage| Usage {
            prompt_tokens: usage["prompt_tokens"].as_u64().unwrap_or(0) as u32,
            completion_tokens: usage["completion_tokens"].as_u64().unwrap_or(0) as u32,
            total_tokens: usage["total_tokens"].as_u64().unwrap_or(0) as u32,
        });
        self.finish_request(duration, usage.clone()).await;

        let message = &body["choices"][0]["message"];
        let content = message["content"].as_str().unwrap_or_default().to_string();
        let tool_calls = message["tool_calls"]
            .as_array()
            .map(|calls| {
                calls
                    .iter()
                    .map(|call| {
                        let args_str = call["function"]["arguments"].as_str().unwrap_or("{}");
                        let arguments =
                            serde_json::from_str(args_str).unwrap_or_else(|_| json!({}));
                        ToolCall {
                            id: call["id"].as_str().unwrap_or_default().to_string(),
                            name: call["function"]["name"]
                                .as_str()
                                .unwrap_or_default()
                                .to_string(),
                            arguments,
                        }
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        Ok(ChatResponse {
            content,
            tool_calls,
            usage,
        })
    }

    pub async fn ai_suggest_completion(
        &self,
        partial_input: &str,
        context: &str,
    ) -> Result<String, String> {
        if partial_input.trim().is_empty() {
            return Ok("".to_string());
        }

        let model = { self.state.read().await.model.clone() };

        let payload = json!({
            "model": model,
            "messages": [
                {
                    "role": "system",
                    "content": format!(
                        "You complete operator input for a pentest terminal.\n\
                         Existing input: {:?}\n\
                         Context: {}\n\n\
                         Return ONLY the exact characters to append next.\n\
                         Rules:\n\
                         - do not repeat or paraphrase the existing input\n\
                         - do not explain anything\n\
                         - do not include quotes, markdown, labels, or newlines\n\
                         - if unsure, return an empty string\n\
                         - maximum 18 characters",
                        partial_input,
                        context
                    )
                }
            ],
            "max_tokens": 12,
            "temperature": 0.0
        });

        if self.api_key == "MOCK_KEY" {
            tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
            if partial_input == "/sc" {
                return Ok("an".to_string());
            }
            return Ok("".to_string());
        }

        let response = match self
            .client
            .post("https://openrouter.ai/api/v1/chat/completions")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("HTTP-Referer", "https://github.com/Steake/pentestagent")
            .header("X-Title", "Serpantoxide AutoComplete")
            .json(&payload)
            .send()
            .await
        {
            Ok(response) => response,
            Err(e) => {
                let message = format!("Autocomplete request failed: {}", e);
                self.set_status_message(message.clone()).await;
                return Err(message);
            }
        };

        if !response.status().is_success() {
            let status = response.status();
            let err_body = response.text().await.unwrap_or_default();
            let message = format!(
                "Autocomplete unavailable: {}",
                summarize_provider_error(status.as_u16(), &err_body)
            );
            self.set_status_message(message.clone()).await;
            return Err(message);
        }

        let body: serde_json::Value = response.json().await.map_err(|e| e.to_string())?;
        let output = body["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("")
            .trim()
            .to_string();

        Ok(normalize_completion_suffix(partial_input, &output))
    }

    async fn finish_request(&self, duration: u64, usage: Option<Usage>) {
        let mut s = self.state.write().await;
        s.is_thinking = false;
        s.status = "Idle".to_string();
        s.last_latency_ms = duration;
        s.prompt_tokens = usage.as_ref().map(|u| u.prompt_tokens).unwrap_or(0);
        s.completion_tokens = usage.as_ref().map(|u| u.completion_tokens).unwrap_or(0);
    }

    async fn fail_request(&self, duration: u64, status: &str) {
        let mut s = self.state.write().await;
        s.is_thinking = false;
        s.status = status.to_string();
        s.last_latency_ms = duration;
        s.prompt_tokens = 0;
        s.completion_tokens = 0;
    }

    async fn set_status_message<S: Into<String>>(&self, status: S) {
        let mut s = self.state.write().await;
        s.status = status.into();
        s.is_thinking = false;
    }

    fn mock_response(
        &self,
        messages: &[serde_json::Value],
        tools: Option<&Vec<serde_json::Value>>,
    ) -> ChatResponse {
        if tools.is_none() {
            return ChatResponse {
                content: "Reasoning: simulated completion.".to_string(),
                tool_calls: Vec::new(),
                usage: Some(Usage {
                    prompt_tokens: 32,
                    completion_tokens: 18,
                    total_tokens: 50,
                }),
            };
        }

        let tool_names = tools
            .map(|defs| {
                defs.iter()
                    .filter_map(|tool| tool["function"]["name"].as_str().map(ToString::to_string))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        if tool_names.iter().any(|name| name == "create_plan") {
            let request = messages
                .iter()
                .rev()
                .find(|msg| msg["role"].as_str() == Some("user"))
                .and_then(|msg| msg["content"].as_str())
                .unwrap_or("task");
            return ChatResponse {
                content: "Creating a concise execution plan.".to_string(),
                tool_calls: vec![ToolCall {
                    id: "mock-create-plan".to_string(),
                    name: "create_plan".to_string(),
                    arguments: json!({
                        "feasible": true,
                        "reason": "Generated a mock plan for local execution.",
                        "steps": [
                            format!("Collect first-pass reconnaissance for {}", request),
                            format!("Record the most relevant finding for {}", request)
                        ]
                    }),
                }],
                usage: Some(Usage {
                    prompt_tokens: 72,
                    completion_tokens: 44,
                    total_tokens: 116,
                }),
            };
        }

        let last_user = messages
            .iter()
            .rev()
            .find(|msg| msg["role"].as_str() == Some("user"))
            .and_then(|msg| msg["content"].as_str())
            .unwrap_or_default()
            .to_string();
        let target = last_user
            .lines()
            .find_map(|line| line.strip_prefix("Target: "))
            .unwrap_or("target");

        let tool_messages = messages
            .iter()
            .filter(|msg| msg["role"].as_str() == Some("tool"))
            .count();

        if tool_names.iter().any(|name| name == "terminal")
            && tool_names.iter().any(|name| name == "finish")
        {
            if tool_messages == 0 {
                return ChatResponse {
                    content: "Starting with direct collection to ground the task.".to_string(),
                    tool_calls: vec![
                        ToolCall {
                            id: "mock-terminal".to_string(),
                            name: "terminal".to_string(),
                            arguments: json!({
                                "command": "echo mock worker reconnaissance",
                                "timeout": 30
                            }),
                        },
                        ToolCall {
                            id: "mock-finish-step-1".to_string(),
                            name: "finish".to_string(),
                            arguments: json!({
                                "action": "complete",
                                "step_id": 1,
                                "result": "Collected an initial command-driven reconnaissance sample."
                            }),
                        },
                    ],
                    usage: Some(Usage {
                        prompt_tokens: 90,
                        completion_tokens: 48,
                        total_tokens: 138,
                    }),
                };
            }

            if tool_messages <= 2 {
                return ChatResponse {
                    content: "Persisting the most relevant observation for the crew.".to_string(),
                    tool_calls: vec![
                        ToolCall {
                            id: "mock-note".to_string(),
                            name: "notes".to_string(),
                            arguments: json!({
                                "action": "create",
                                "key": "mock_worker_finding",
                                "value": "Mock reconnaissance completed by Rust WorkerAgent",
                                "category": "finding",
                                "target": target
                            }),
                        },
                        ToolCall {
                            id: "mock-finish-step-2".to_string(),
                            name: "finish".to_string(),
                            arguments: json!({
                                "action": "complete",
                                "step_id": 2,
                                "result": "Stored a worker finding in shared notes."
                            }),
                        },
                    ],
                    usage: Some(Usage {
                        prompt_tokens: 84,
                        completion_tokens: 42,
                        total_tokens: 126,
                    }),
                };
            }

            return ChatResponse {
                content: "Worker task complete.".to_string(),
                tool_calls: Vec::new(),
                usage: Some(Usage {
                    prompt_tokens: 40,
                    completion_tokens: 18,
                    total_tokens: 58,
                }),
            };
        }

        if tool_messages == 0 {
            return ChatResponse {
                content: "Planning a parallel first pass across discovery, web validation, and intelligence gathering.".to_string(),
                tool_calls: vec![
                    ToolCall {
                        id: "mock-plan".to_string(),
                        name: "update_plan".to_string(),
                        arguments: json!({
                            "completed_tasks": [],
                            "remaining_tasks": [
                                format!("Network reconnaissance for {}", target),
                                format!("Web surface inspection for {}", target),
                                format!("Target-specific intelligence for {}", target)
                            ]
                        }),
                    },
                    ToolCall {
                        id: "mock-spawn-1".to_string(),
                        name: "spawn_agent".to_string(),
                        arguments: json!({
                            "task": format!("NMAP: {}", target),
                            "priority": 3
                        }),
                    },
                    ToolCall {
                        id: "mock-spawn-2".to_string(),
                        name: "spawn_agent".to_string(),
                        arguments: json!({
                            "task": format!("SEARCH: known attack surface and vulnerabilities for {}", target),
                            "priority": 2
                        }),
                    },
                    ToolCall {
                        id: "mock-spawn-3".to_string(),
                        name: "spawn_agent".to_string(),
                        arguments: json!({
                            "task": format!("BROWSER: http://{}", target),
                            "priority": 2
                        }),
                    },
                    ToolCall {
                        id: "mock-wait".to_string(),
                        name: "wait_for_agents".to_string(),
                        arguments: json!({}),
                    },
                ],
                usage: Some(Usage {
                    prompt_tokens: 180,
                    completion_tokens: 96,
                    total_tokens: 276,
                }),
            };
        }

        ChatResponse {
            content: "Initial reconnaissance finished. Consolidating the worker output."
                .to_string(),
            tool_calls: vec![
                ToolCall {
                    id: "mock-plan-done".to_string(),
                    name: "update_plan".to_string(),
                    arguments: json!({
                        "completed_tasks": [
                            format!("Network reconnaissance for {}", target),
                            format!("Web surface inspection for {}", target),
                            format!("Target-specific intelligence for {}", target)
                        ],
                        "remaining_tasks": []
                    }),
                },
                ToolCall {
                    id: "mock-finish".to_string(),
                    name: "finish".to_string(),
                    arguments: json!({
                        "context": format!("Parallel assessment of {} complete.", target)
                    }),
                },
            ],
            usage: Some(Usage {
                prompt_tokens: 120,
                completion_tokens: 54,
                total_tokens: 174,
            }),
        }
    }
}

fn normalize_completion_suffix(partial_input: &str, raw_output: &str) -> String {
    let candidate = raw_output
        .lines()
        .next()
        .unwrap_or("")
        .trim()
        .trim_matches('`')
        .trim_matches('"')
        .trim_matches('\'')
        .to_string();

    if candidate.is_empty() {
        return String::new();
    }

    if let Some(stripped) = candidate.strip_prefix(partial_input) {
        return stripped.to_string();
    }

    let lower_partial = partial_input.to_lowercase();
    let lower_candidate = candidate.to_lowercase();
    if lower_candidate.starts_with(&lower_partial) {
        return candidate
            .chars()
            .skip(partial_input.chars().count())
            .collect();
    }

    if candidate.contains("The user's input so far")
        || candidate.contains("Provide ONLY")
        || candidate.contains("Context:")
    {
        return String::new();
    }

    if candidate.contains('\n')
        || candidate.contains(':')
        || candidate.contains('{')
        || candidate.contains('}')
        || candidate.contains('[')
        || candidate.contains(']')
        || candidate.chars().count() > 24
        || candidate.split_whitespace().count() > 4
    {
        return String::new();
    }

    if partial_input.starts_with('/') {
        return String::new();
    }

    candidate
}

fn summarize_provider_error(status_code: u16, body: &str) -> String {
    let parsed_message = serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|json| {
            json.get("error")
                .and_then(|value| {
                    value
                        .get("message")
                        .and_then(|msg| msg.as_str())
                        .or_else(|| value.as_str())
                })
                .or_else(|| json.get("message").and_then(|msg| msg.as_str()))
                .map(ToString::to_string)
        })
        .unwrap_or_else(|| body.trim().to_string());

    let compact = if parsed_message.is_empty() {
        format!("HTTP {}", status_code)
    } else {
        parsed_message
    };

    match status_code {
        401 => format!("Provider rejected credentials (HTTP 401): {}", compact),
        402 => format!(
            "Provider refused request for billing reasons (HTTP 402): {}",
            compact
        ),
        403 => format!("Provider denied access (HTTP 403): {}", compact),
        404 => format!("Model or route not available (HTTP 404): {}", compact),
        408 => format!("Provider timed out (HTTP 408): {}", compact),
        429 => format!("Rate limited by provider (HTTP 429): {}", compact),
        500..=599 => format!(
            "Provider failed upstream (HTTP {}): {}",
            status_code, compact
        ),
        _ => format!("Provider error (HTTP {}): {}", status_code, compact),
    }
}
