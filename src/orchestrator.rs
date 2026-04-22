use std::sync::Arc;

use serde_json::json;
use tokio::sync::RwLock;
use tokio::sync::mpsc::Sender;

use crate::browser::NativeBrowserEngine;
use crate::config::AppConfig;
use crate::events::UiEvent;
use crate::graph::ShadowGraph;
use crate::llm::{ChatResponse, NativeLLMEngine, ToolCall};
use crate::mission::{self, DiscoverySignals, MissionProfile};
use crate::notes::NotesEngine;
use crate::pool::WorkerPool;
use crate::prompts;
use crate::web_search::NativeWebSearch;

#[derive(Clone)]
pub struct Orchestrator {
    llm: Arc<NativeLLMEngine>,
    pool: WorkerPool,
    notes: Arc<NotesEngine>,
    browser: Option<Arc<NativeBrowserEngine>>,
    search: Arc<NativeWebSearch>,
    graph: Arc<RwLock<ShadowGraph>>,
    target_shared: Arc<RwLock<String>>,
    preset_shared: Arc<RwLock<String>>,
    tx: Sender<UiEvent>,
}

impl Orchestrator {
    pub fn new(
        llm: Arc<NativeLLMEngine>,
        pool: WorkerPool,
        notes: Arc<NotesEngine>,
        browser: Option<Arc<NativeBrowserEngine>>,
        search: Arc<NativeWebSearch>,
        graph: Arc<RwLock<ShadowGraph>>,
        target_shared: Arc<RwLock<String>>,
        preset_shared: Arc<RwLock<String>>,
        tx: Sender<UiEvent>,
    ) -> Self {
        Self {
            llm,
            pool,
            notes,
            browser,
            search,
            graph,
            target_shared,
            preset_shared,
            tx,
        }
    }

    pub async fn run_swarm_mode(&self, target: &str, task: &str) -> Result<(), String> {
        self.run_crew_mode(target, task).await
    }

    pub async fn run_crew_mode(&self, target: &str, task: &str) -> Result<(), String> {
        {
            let mut shared = self.target_shared.write().await;
            *shared = target.to_string();
        }

        let _ = self
            .tx
            .send(UiEvent::log(format!("Crew mode engaged for {}", target)))
            .await;

        let mut current_plan: Vec<String> = Vec::new();
        let initial_mission = self.build_mission_profile(target, task).await;
        let mut messages = vec![json!({
            "role": "user",
            "content": initial_mission.execution_brief(target)
        })];

        let max_iterations = AppConfig::load().max_iterations.max(1);

        for iteration in 0..max_iterations {
            let mission = self.build_mission_profile(target, task).await;
            let system_prompt = self
                .build_system_prompt(target, task, &mission, &current_plan)
                .await;
            let response = self
                .llm
                .generate_with_tools(&system_prompt, messages.clone(), orchestration_tools())
                .await?;

            if !response.content.trim().is_empty() {
                let _ = self
                    .tx
                    .send(UiEvent::log(format!(
                        "Crew reasoning: {}",
                        response.content.trim()
                    )))
                    .await;
            }

            let assistant_message = build_assistant_message(&response);
            messages.push(assistant_message);

            let has_plan_update = response
                .tool_calls
                .iter()
                .any(|call| call.name == "update_plan");
            if current_plan.is_empty() && !has_plan_update {
                let _ = self
                    .tx
                    .send(UiEvent::log(
                        "Crew has not published a checklist yet; requesting update_plan before execution."
                            .to_string(),
                    ))
                    .await;
                messages.push(json!({
                    "role": "user",
                    "content": "Before spawning, waiting, cancelling, or finishing, call update_plan with a concise checklist for this crew mission. Then continue with the mission."
                }));
                continue;
            }

            if response.tool_calls.is_empty() {
                if iteration + 1 >= max_iterations {
                    let fallback = self
                        .finish_task(format!(
                            "Crew iteration budget reached while pursuing: {}",
                            mission.desired_outcome
                        ))
                        .await?;
                    self.emit_completion(&fallback).await;
                    return Ok(());
                }

                let worker_status = self.worker_status_lines().await;
                let nudge = mission.continuation_nudge(&worker_status);
                let _ = self
                    .tx
                    .send(UiEvent::log(
                        "Crew returned no tool calls; injecting continuation nudge.".to_string(),
                    ))
                    .await;
                messages.push(json!({
                    "role": "user",
                    "content": nudge
                }));
                continue;
            }

            for tool_call in response.tool_calls {
                let tool_name = tool_call.name.clone();
                let result = self.execute_tool(&tool_call, &mut current_plan).await?;
                messages.push(json!({
                    "role": "tool",
                    "tool_call_id": tool_call.id,
                    "content": result
                }));

                if tool_name == "finish" {
                    self.emit_completion(
                        messages
                            .last()
                            .and_then(|msg| msg["content"].as_str())
                            .unwrap_or_default(),
                    )
                    .await;
                    return Ok(());
                }
            }
        }

        Ok(())
    }

    pub async fn generate_report(&self, target: &str) -> Result<String, String> {
        let insights = self.graph.read().await.get_strategic_insights().join("\n");
        let note_categories = self.notes.list_categories().await;
        let note_summary = if note_categories.is_empty() {
            "No saved notes.".to_string()
        } else {
            note_categories
                .into_iter()
                .map(|(name, count)| format!("- {}: {} entries", name, count))
                .collect::<Vec<_>>()
                .join("\n")
        };

        let prompt = format!(
            "You are an expert offensive security reporting engine.\n\
             Generate a concise markdown penetration test report.\n\n\
             Target: {target}\n\n\
             Graph insights:\n{insights}\n\n\
             Notes summary:\n{note_summary}\n\n\
             Include sections: Executive Summary, Attack Surface, Findings, Recommendations."
        );

        self.llm
            .generate_with_history(vec![json!({"role": "system", "content": prompt})])
            .await
    }

    pub async fn prompt_preview(&self, target: &str, task: &str) -> String {
        let mission = self.build_mission_profile(target, task).await;
        self.build_system_prompt(target, task, &mission, &[]).await
    }

    async fn build_mission_profile(&self, target: &str, task: &str) -> MissionProfile {
        let selected_preset = self.preset_shared.read().await.clone();
        let topology = self.graph.read().await.snapshot();
        let note_categories = self.notes.list_categories().await;
        let signals = DiscoverySignals::new(topology, note_categories);
        mission::resolve_mission(&selected_preset, target, task, &signals)
    }

    async fn build_system_prompt(
        &self,
        target: &str,
        task: &str,
        mission: &MissionProfile,
        current_plan: &[String],
    ) -> String {
        let note_categories = self.notes.list_categories().await;
        let insights = self.graph.read().await.get_strategic_insights();
        let mut augmented_insights = insights;
        if !note_categories.is_empty() {
            augmented_insights.push(format!(
                "Saved note categories: {}",
                note_categories
                    .iter()
                    .map(|(name, count)| format!("{} ({})", name, count))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        if self.browser.is_some() {
            augmented_insights
                .push("Native browser engine is available for web interaction.".to_string());
        } else {
            augmented_insights.push(
                "Read-only browser fallback is available for navigate/get_content/get_links/get_forms. Screenshot, click, type, and execute_js require Chromium.".to_string(),
            );
        }
        if !self.search.api_key().is_empty() {
            augmented_insights.push("Web intelligence search is available.".to_string());
        }
        let worker_status = self.worker_status_lines().await;

        prompts::build_crew_prompt(
            target,
            task,
            mission,
            &augmented_insights,
            current_plan,
            &worker_status,
        )
    }

    async fn worker_status_lines(&self) -> Vec<String> {
        self.pool
            .get_workers()
            .await
            .into_iter()
            .take(8)
            .map(|worker| {
                let outcome = worker
                    .result
                    .as_deref()
                    .or(worker.error.as_deref())
                    .unwrap_or("");
                if outcome.is_empty() {
                    format!("{} [{}] {}", worker.id, worker.status, worker.task)
                } else {
                    format!(
                        "{} [{}] {} -> {}",
                        worker.id, worker.status, worker.task, outcome
                    )
                }
            })
            .collect()
    }

    async fn execute_tool(
        &self,
        tool_call: &ToolCall,
        current_plan: &mut Vec<String>,
    ) -> Result<String, String> {
        let _ = self
            .tx
            .send(UiEvent::log(format!(
                "Tool call: {} {}",
                tool_call.name, tool_call.arguments
            )))
            .await;

        match tool_call.name.as_str() {
            "spawn_agent" => {
                let task = tool_call.arguments["task"]
                    .as_str()
                    .ok_or_else(|| "spawn_agent.task is required".to_string())?
                    .to_string();
                let priority = tool_call.arguments["priority"].as_i64().unwrap_or(1);
                let depends_on = tool_call.arguments["depends_on"]
                    .as_array()
                    .map(|items| {
                        items
                            .iter()
                            .filter_map(|item| item.as_str().map(ToString::to_string))
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                let worker_id = self.pool.spawn(task.clone(), priority, depends_on).await;
                Ok(format!("Spawned {} for {}", worker_id, task))
            }
            "spawn_parallel_agents" => {
                let agents = tool_call.arguments["agents"]
                    .as_array()
                    .ok_or_else(|| "spawn_parallel_agents.agents must be an array".to_string())?;

                if agents.is_empty() {
                    return Err("spawn_parallel_agents.agents cannot be empty".to_string());
                }

                let mut spawned = Vec::new();
                for agent in agents {
                    let task = agent["task"]
                        .as_str()
                        .ok_or_else(|| {
                            "spawn_parallel_agents.agents[].task is required".to_string()
                        })?
                        .to_string();
                    let priority = agent["priority"].as_i64().unwrap_or(1);
                    let depends_on = agent["depends_on"]
                        .as_array()
                        .map(|items| {
                            items
                                .iter()
                                .filter_map(|item| item.as_str().map(ToString::to_string))
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default();

                    let worker_id = self.pool.spawn(task.clone(), priority, depends_on).await;
                    spawned.push((worker_id, task));
                }

                Ok(spawned
                    .into_iter()
                    .map(|(worker_id, task)| format!("Spawned {} for {}", worker_id, task))
                    .collect::<Vec<_>>()
                    .join("\n"))
            }
            "wait_for_agents" => {
                let agent_ids = tool_call.arguments["agent_ids"].as_array().map(|items| {
                    items
                        .iter()
                        .filter_map(|item| item.as_str().map(ToString::to_string))
                        .collect::<Vec<_>>()
                });
                let results = self.pool.wait_for(agent_ids).await;
                Ok(format_worker_results(&results))
            }
            "get_agent_status" => {
                let agent_id = tool_call.arguments["agent_id"]
                    .as_str()
                    .ok_or_else(|| "get_agent_status.agent_id is required".to_string())?;
                Ok(self
                    .pool
                    .get_status(agent_id)
                    .await
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| format!("{} not found", agent_id)))
            }
            "cancel_agent" => {
                let agent_id = tool_call.arguments["agent_id"]
                    .as_str()
                    .ok_or_else(|| "cancel_agent.agent_id is required".to_string())?;
                let cancelled = self.pool.cancel(agent_id).await;
                Ok(if cancelled {
                    format!("Cancelled {}", agent_id)
                } else {
                    format!("Could not cancel {}", agent_id)
                })
            }
            "formulate_strategy" => Ok(format_strategy(&tool_call.arguments)),
            "update_plan" => {
                let completed = extract_string_array(&tool_call.arguments["completed_tasks"]);
                let remaining = extract_string_array(&tool_call.arguments["remaining_tasks"]);
                *current_plan = remaining.clone();
                let _ = self
                    .tx
                    .send(UiEvent::Checklist {
                        completed,
                        remaining: remaining.clone(),
                    })
                    .await;
                Ok("Checklist updated.".to_string())
            }
            "finish" => {
                let context = tool_call.arguments["context"]
                    .as_str()
                    .unwrap_or("Crew objectives completed.")
                    .to_string();
                self.finish_task(context).await
            }
            other => Err(format!("Unknown tool: {}", other)),
        }
    }

    async fn finish_task(&self, context: String) -> Result<String, String> {
        let workers = self.pool.get_workers().await;
        let worker_summary = workers
            .iter()
            .map(|worker| {
                format!(
                    "## {} [{}]\nTask: {}\n{}\n{}",
                    worker.id,
                    worker.status,
                    worker.task,
                    worker.result.clone().unwrap_or_default(),
                    worker
                        .error
                        .as_ref()
                        .map(|error| format!("Error: {}", error))
                        .unwrap_or_default()
                )
            })
            .collect::<Vec<_>>()
            .join("\n\n");

        let prompt = format!(
            "Synthesize these crew findings into a concise operator-facing summary.\n\n\
             Context: {context}\n\n\
             Worker results:\n{worker_summary}\n"
        );

        self.llm
            .generate_with_history(vec![
                json!({
                    "role": "system",
                    "content": "You summarize penetration testing crew output clearly and factually."
                }),
                json!({
                    "role": "user",
                    "content": prompt
                }),
            ])
            .await
    }

    async fn emit_completion(&self, summary: &str) {
        let _ = self
            .tx
            .send(UiEvent::CrewComplete {
                summary: summary.to_string(),
            })
            .await;
    }
}

fn orchestration_tools() -> Vec<serde_json::Value> {
    vec![
        tool(
            "spawn_agent",
            "Spawn a new worker. Provide a concrete natural-language task. Direct tool prefixes remain available when needed.",
            json!({
                "type": "object",
                "properties": {
                    "task": {"type": "string"},
                    "priority": {"type": "integer"},
                    "depends_on": {"type": "array", "items": {"type": "string"}}
                },
                "required": ["task"]
            }),
        ),
        tool(
            "spawn_parallel_agents",
            "Spawn multiple independent workers in one batch. Use this for initial reconnaissance spreads or any time several tasks can run concurrently.",
            json!({
                "type": "object",
                "properties": {
                    "agents": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "task": {"type": "string"},
                                "priority": {"type": "integer"},
                                "depends_on": {"type": "array", "items": {"type": "string"}}
                            },
                            "required": ["task"]
                        }
                    }
                },
                "required": ["agents"]
            }),
        ),
        tool(
            "wait_for_agents",
            "Wait for workers and gather their results.",
            json!({
                "type": "object",
                "properties": {
                    "agent_ids": {"type": "array", "items": {"type": "string"}}
                }
            }),
        ),
        tool(
            "get_agent_status",
            "Check one worker.",
            json!({
                "type": "object",
                "properties": {
                    "agent_id": {"type": "string"}
                },
                "required": ["agent_id"]
            }),
        ),
        tool(
            "cancel_agent",
            "Cancel one worker.",
            json!({
                "type": "object",
                "properties": {
                    "agent_id": {"type": "string"}
                },
                "required": ["agent_id"]
            }),
        ),
        tool(
            "formulate_strategy",
            "Record strategic reasoning for a course of action.",
            json!({
                "type": "object",
                "properties": {
                    "problem": {"type": "string"},
                    "candidates": {"type": "array"},
                    "selected_id": {"type": "string"},
                    "rationale": {"type": "string"},
                    "feasible": {"type": "boolean"}
                },
                "required": ["problem", "rationale"]
            }),
        ),
        tool(
            "update_plan",
            "Update the visible task checklist.",
            json!({
                "type": "object",
                "properties": {
                    "completed_tasks": {"type": "array", "items": {"type": "string"}},
                    "remaining_tasks": {"type": "array", "items": {"type": "string"}}
                },
                "required": ["completed_tasks", "remaining_tasks"]
            }),
        ),
        tool(
            "finish",
            "Wait for all workers and synthesize the final summary.",
            json!({
                "type": "object",
                "properties": {
                    "context": {"type": "string"}
                }
            }),
        ),
    ]
}

fn tool(name: &str, description: &str, parameters: serde_json::Value) -> serde_json::Value {
    json!({
        "type": "function",
        "function": {
            "name": name,
            "description": description,
            "parameters": parameters
        }
    })
}

fn build_assistant_message(response: &ChatResponse) -> serde_json::Value {
    let mut message = json!({
        "role": "assistant",
        "content": response.content
    });

    if !response.tool_calls.is_empty() {
        message["tool_calls"] = serde_json::Value::Array(
            response
                .tool_calls
                .iter()
                .map(|call| {
                    json!({
                        "id": call.id,
                        "type": "function",
                        "function": {
                            "name": call.name,
                            "arguments": call.arguments.to_string()
                        }
                    })
                })
                .collect(),
        );
    }

    message
}

fn format_worker_results(results: &std::collections::HashMap<String, serde_json::Value>) -> String {
    if results.is_empty() {
        return "No workers available.".to_string();
    }

    let mut sections = Vec::new();
    for (worker_id, result) in results {
        sections.push(format!(
            "## {}\nStatus: {}\nTask: {}\nResult: {}\nError: {}\nTools: {}",
            worker_id,
            result["status"].as_str().unwrap_or("unknown"),
            result["task"].as_str().unwrap_or_default(),
            result["result"].as_str().unwrap_or_default(),
            result["error"].as_str().unwrap_or_default(),
            result["tools_used"]
                .as_array()
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|item| item.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .unwrap_or_default()
        ));
    }
    sections.join("\n\n")
}

fn format_strategy(arguments: &serde_json::Value) -> String {
    let problem = arguments["problem"].as_str().unwrap_or("Unknown problem");
    let rationale = arguments["rationale"]
        .as_str()
        .unwrap_or("No rationale provided.");
    let feasible = arguments["feasible"].as_bool().unwrap_or(true);

    if !feasible {
        return format!(
            "Strategic decision: mission infeasible.\nProblem: {}\nRationale: {}",
            problem, rationale
        );
    }

    let selected_id = arguments["selected_id"].as_str().unwrap_or("");
    let mut lines = vec![format!("Problem: {}", problem)];
    if let Some(candidates) = arguments["candidates"].as_array() {
        for candidate in candidates {
            let candidate_id = candidate["id"].as_str().unwrap_or("");
            let marker = if candidate_id == selected_id {
                " [selected]"
            } else {
                ""
            };
            lines.push(format!(
                "- {}{}: pros={} cons={} risk={}",
                candidate["name"].as_str().unwrap_or("Unnamed"),
                marker,
                candidate["pros"].as_str().unwrap_or(""),
                candidate["cons"].as_str().unwrap_or(""),
                candidate["risk"].as_str().unwrap_or("")
            ));
        }
    }
    lines.push(format!("Rationale: {}", rationale));
    lines.join("\n")
}

fn extract_string_array(value: &serde_json::Value) -> Vec<String> {
    value
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(ToString::to_string))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}
