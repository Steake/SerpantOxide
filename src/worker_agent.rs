use std::sync::Arc;

use serde_json::{Value, json};
use tokio::sync::{RwLock, mpsc};

use crate::browser::NativeBrowserEngine;
use crate::events::UiEvent;
use crate::graph::ShadowGraph;
use crate::llm::{ChatResponse, NativeLLMEngine, ToolCall};
use crate::notes::NotesEngine;
use crate::pool::WorkerPoolState;
use crate::prompts;
use crate::sqlmap::NativeSqlmap;
use crate::web_search::NativeWebSearch;

#[derive(Clone, Debug, PartialEq, Eq)]
enum StepStatus {
    Pending,
    Complete,
    Skip,
    Fail,
}

#[derive(Clone, Debug)]
struct PlanStep {
    id: usize,
    description: String,
    status: StepStatus,
    result: Option<String>,
}

impl PlanStep {
    fn as_line(&self) -> String {
        let status = match self.status {
            StepStatus::Pending => "PENDING",
            StepStatus::Complete => "COMPLETE",
            StepStatus::Skip => "SKIP",
            StepStatus::Fail => "FAIL",
        };
        format!("{}. [{}] {}", self.id, status, self.description)
    }
}

#[derive(Clone)]
pub struct WorkerHandle {
    pub worker_id: String,
    state: Arc<RwLock<WorkerPoolState>>,
    ui_tx: mpsc::Sender<String>,
}

impl WorkerHandle {
    pub fn new(
        worker_id: String,
        state: Arc<RwLock<WorkerPoolState>>,
        ui_tx: mpsc::Sender<String>,
    ) -> Self {
        Self {
            worker_id,
            state,
            ui_tx,
        }
    }

    pub async fn log<S: Into<String>>(&self, message: S) {
        let message = message.into();
        {
            let mut state = self.state.write().await;
            if let Some(worker) = state.workers.get_mut(&self.worker_id) {
                worker.logs.push(message.clone());
            }
        }
        let _ = self
            .ui_tx
            .send(UiEvent::log(format!("[{}] {}", self.worker_id, message)).serialize())
            .await;
    }

    pub async fn set_status(&self, status: &str) {
        let mut state = self.state.write().await;
        if let Some(worker) = state.workers.get_mut(&self.worker_id) {
            worker.status = status.to_string();
        }
    }

    pub async fn record_tool(&self, tool_name: &str) {
        let mut state = self.state.write().await;
        if let Some(worker) = state.workers.get_mut(&self.worker_id) {
            if !worker.tools_used.iter().any(|name| name == tool_name) {
                worker.tools_used.push(tool_name.to_string());
            }
        }
    }

    pub async fn add_loot<S: Into<String>>(&self, loot: S) {
        let mut state = self.state.write().await;
        if let Some(worker) = state.workers.get_mut(&self.worker_id) {
            worker.loot.push(loot.into());
        }
    }
}

pub struct WorkerAgent {
    llm: Arc<NativeLLMEngine>,
    notes: Arc<NotesEngine>,
    browser: Option<Arc<NativeBrowserEngine>>,
    search: Arc<NativeWebSearch>,
    graph: Arc<RwLock<ShadowGraph>>,
    max_iterations: usize,
}

impl WorkerAgent {
    pub fn new(
        llm: Arc<NativeLLMEngine>,
        notes: Arc<NotesEngine>,
        browser: Option<Arc<NativeBrowserEngine>>,
        search: Arc<NativeWebSearch>,
        graph: Arc<RwLock<ShadowGraph>>,
    ) -> Self {
        Self {
            llm,
            notes,
            browser,
            search,
            graph,
            max_iterations: 10,
        }
    }

    pub async fn run(&self, task: &str, handle: &WorkerHandle) -> Result<String, String> {
        let mut plan = self.generate_plan(task).await?;
        if plan.is_empty() {
            plan.push(PlanStep {
                id: 1,
                description: task.to_string(),
                status: StepStatus::Pending,
                result: None,
            });
        }

        handle
            .log(format!(
                "Plan:\n{}",
                plan.iter()
                    .map(PlanStep::as_line)
                    .collect::<Vec<_>>()
                    .join("\n")
            ))
            .await;

        let mut messages = vec![json!({
            "role": "user",
            "content": task
        })];

        for _ in 0..self.max_iterations {
            if plan_complete(&plan) {
                return self.summarize(task, &plan, &messages).await;
            }

            let prompt = prompts::build_worker_prompt(
                task,
                &plan.iter().map(PlanStep::as_line).collect::<Vec<_>>(),
            );
            let response = self
                .llm
                .generate_with_tools(&prompt, messages.clone(), worker_tools())
                .await?;

            if !response.content.trim().is_empty() {
                handle
                    .log(format!("Thinking: {}", response.content.trim()))
                    .await;
            }

            if response.tool_calls.is_empty() {
                messages.push(json!({
                    "role": "assistant",
                    "content": response.content
                }));
                continue;
            }

            messages.push(build_assistant_message(&response));

            for tool_call in response.tool_calls {
                let tool_name = tool_call.name.clone();
                handle.record_tool(&tool_name).await;
                let result = self.execute_tool(&tool_call, &mut plan, handle).await?;
                messages.push(json!({
                    "role": "tool",
                    "tool_call_id": tool_call.id,
                    "content": result
                }));

                if tool_name == "finish" && plan_complete(&plan) {
                    return self.summarize(task, &plan, &messages).await;
                }
            }

            if plan_has_failure(&plan) {
                let replanned = self.replan(task, &plan).await?;
                handle
                    .log(format!(
                        "Replanned:\n{}",
                        replanned
                            .iter()
                            .map(PlanStep::as_line)
                            .collect::<Vec<_>>()
                            .join("\n")
                    ))
                    .await;
                plan = replanned;
            }
        }

        Err(format!(
            "WorkerAgent reached max iterations ({}) for task: {}",
            self.max_iterations, task
        ))
    }

    async fn generate_plan(&self, task: &str) -> Result<Vec<PlanStep>, String> {
        let response = self
            .llm
            .generate_with_tools(
                "You create concise penetration testing plans. Always call create_plan.",
                vec![json!({
                    "role": "user",
                    "content": format!("Break this pentest task into 2-4 actionable steps.\nTask: {}", task)
                })],
                vec![json!({
                    "type": "function",
                    "function": {
                        "name": "create_plan",
                        "description": "Create a concise task plan.",
                        "parameters": {
                            "type": "object",
                            "properties": {
                                "steps": {
                                    "type": "array",
                                    "items": {"type": "string"}
                                }
                            },
                            "required": ["steps"]
                        }
                    }
                })],
            )
            .await?;

        for tool_call in response.tool_calls {
            if tool_call.name == "create_plan" {
                let steps = tool_call.arguments["steps"]
                    .as_array()
                    .map(|items| {
                        items
                            .iter()
                            .enumerate()
                            .map(|(idx, item)| PlanStep {
                                id: idx + 1,
                                description: item.as_str().unwrap_or("Unnamed step").to_string(),
                                status: StepStatus::Pending,
                                result: None,
                            })
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                return Ok(steps);
            }
        }

        Ok(vec![PlanStep {
            id: 1,
            description: task.to_string(),
            status: StepStatus::Pending,
            result: None,
        }])
    }

    async fn execute_tool(
        &self,
        tool_call: &ToolCall,
        plan: &mut [PlanStep],
        handle: &WorkerHandle,
    ) -> Result<String, String> {
        handle
            .log(format!(
                "Tool call: {} {}",
                tool_call.name, tool_call.arguments
            ))
            .await;

        match tool_call.name.as_str() {
            "terminal" => self.execute_terminal(&tool_call.arguments, handle).await,
            "browser" => self.execute_browser(&tool_call.arguments, handle).await,
            "web_search" => self.execute_web_search(&tool_call.arguments, handle).await,
            "notes" => self.execute_notes(&tool_call.arguments, handle).await,
            "nmap" => self.execute_nmap(&tool_call.arguments, handle).await,
            "sqlmap" => self.execute_sqlmap(&tool_call.arguments, handle).await,
            "osint" => self.execute_osint(&tool_call.arguments, handle).await,
            "hosting" => self.execute_hosting(&tool_call.arguments, handle).await,
            "image_gen" => self.execute_image_gen(&tool_call.arguments, handle).await,
            "evm_chain" => self.execute_evm_chain(&tool_call.arguments, handle).await,
            "finish" => {
                self.execute_finish(&tool_call.arguments, plan, handle)
                    .await
            }
            other => Err(format!("Unknown worker tool: {}", other)),
        }
    }

    async fn execute_terminal(
        &self,
        arguments: &Value,
        handle: &WorkerHandle,
    ) -> Result<String, String> {
        handle.set_status("Executing").await;
        let command = arguments["command"]
            .as_str()
            .ok_or_else(|| "terminal.command is required".to_string())?;
        let timeout = arguments["timeout"].as_u64().unwrap_or(300);
        let working_dir = arguments["working_dir"].as_str();
        let inputs = arguments["inputs"].as_str();
        let privileged = arguments["privileged"].as_bool().unwrap_or(false);
        let output = crate::terminal::NativeTerminal::execute_with_options(
            command,
            timeout,
            working_dir,
            inputs,
            privileged,
        )
        .await?;
        {
            let mut graph = self.graph.write().await;
            graph.extract_from_note("terminal", &output);
        }
        handle.log(output.clone()).await;
        Ok(output)
    }

    async fn execute_browser(
        &self,
        arguments: &Value,
        handle: &WorkerHandle,
    ) -> Result<String, String> {
        handle.set_status("Browsing").await;
        let action = arguments["action"]
            .as_str()
            .ok_or_else(|| "browser.action is required".to_string())?;
        let browser = self
            .browser
            .clone()
            .ok_or_else(|| "Native browser engine unavailable".to_string())?;
        let timeout = arguments["timeout"].as_u64().unwrap_or(30) * 1000;

        let result = match action {
            "navigate" => {
                let url = arguments["url"]
                    .as_str()
                    .ok_or_else(|| "browser.url is required for navigate".to_string())?;
                browser
                    .navigate(url, arguments["wait_for"].as_str(), timeout)
                    .await?
            }
            "screenshot" => {
                browser
                    .screenshot(arguments["url"].as_str(), timeout)
                    .await?
            }
            "get_content" => {
                browser
                    .get_content(arguments["url"].as_str(), timeout)
                    .await?
            }
            "get_links" => {
                browser
                    .get_links(arguments["url"].as_str(), timeout)
                    .await?
            }
            "get_forms" => {
                browser
                    .get_forms(arguments["url"].as_str(), timeout)
                    .await?
            }
            "click" => {
                let selector = arguments["selector"]
                    .as_str()
                    .ok_or_else(|| "browser.selector is required for click".to_string())?;
                browser
                    .click(selector, arguments["wait_for"].as_str(), timeout)
                    .await?
            }
            "type" => {
                let selector = arguments["selector"]
                    .as_str()
                    .ok_or_else(|| "browser.selector is required for type".to_string())?;
                let text = arguments["text"].as_str().unwrap_or("");
                browser
                    .type_text(selector, text, arguments["wait_for"].as_str(), timeout)
                    .await?
            }
            "execute_js" => {
                let javascript = arguments["javascript"]
                    .as_str()
                    .ok_or_else(|| "browser.javascript is required for execute_js".to_string())?;
                browser.execute_js(javascript).await?
            }
            unsupported => {
                return Err(format!(
                    "Browser action '{}' is not implemented in the Rust engine yet",
                    unsupported
                ));
            }
        };

        {
            let mut graph = self.graph.write().await;
            graph.extract_from_note("browser", &result);
        }
        if action == "screenshot" {
            handle.add_loot(result.clone()).await;
        }
        handle.log(result.clone()).await;
        Ok(result)
    }

    async fn execute_web_search(
        &self,
        arguments: &Value,
        handle: &WorkerHandle,
    ) -> Result<String, String> {
        handle.set_status("Researching").await;
        let query = arguments["query"]
            .as_str()
            .ok_or_else(|| "web_search.query is required".to_string())?;
        let result = self.search.search(query).await?;
        handle.log(result.clone()).await;
        Ok(result)
    }

    async fn execute_notes(
        &self,
        arguments: &Value,
        handle: &WorkerHandle,
    ) -> Result<String, String> {
        let action = arguments["action"].as_str().unwrap_or("list");
        match action {
            "create" | "update" => {
                let key = arguments["key"]
                    .as_str()
                    .ok_or_else(|| "notes.key is required".to_string())?;
                let value = arguments["value"]
                    .as_str()
                    .ok_or_else(|| "notes.value is required".to_string())?;
                let category = arguments["category"].as_str().unwrap_or("info");
                let target = arguments["target"].as_str().map(ToString::to_string);
                let metadata = arguments.clone();
                self.notes
                    .upsert_note(key, category, value, target.clone(), metadata)
                    .await?;
                if let Some(target) = target {
                    handle.add_loot(format!("Note {} -> {}", key, target)).await;
                } else {
                    handle.add_loot(format!("Note {}", key)).await;
                }
                let message = format!("Stored note '{}' in category '{}'", key, category);
                handle.log(message.clone()).await;
                Ok(message)
            }
            "read" => {
                let key = arguments["key"]
                    .as_str()
                    .ok_or_else(|| "notes.key is required".to_string())?;
                let result = self
                    .notes
                    .read_note(key)
                    .await
                    .map(|note| serde_json::to_string_pretty(&note).unwrap_or_default())
                    .unwrap_or_else(|| format!("Note '{}' not found", key));
                handle.log(result.clone()).await;
                Ok(result)
            }
            "list" => {
                let keys = self.notes.list_note_keys().await;
                let result = if keys.is_empty() {
                    "No notes stored.".to_string()
                } else {
                    format!("Notes:\n{}", keys.join("\n"))
                };
                handle.log(result.clone()).await;
                Ok(result)
            }
            other => Err(format!("Unsupported notes action: {}", other)),
        }
    }

    async fn execute_nmap(
        &self,
        arguments: &Value,
        handle: &WorkerHandle,
    ) -> Result<String, String> {
        handle.set_status("Scanning").await;
        let target = arguments["target"]
            .as_str()
            .ok_or_else(|| "nmap.target is required".to_string())?;
        let result = crate::nmap::NativeNmap::scan(target).await?;
        let ports = crate::nmap::NativeNmap::parse_discovered_ports(&result);
        {
            let mut graph = self.graph.write().await;
            graph.ingest_nmap(target, ports.clone());
        }
        for (port, service) in ports {
            handle
                .add_loot(format!("Open service {} ({}) on {}", port, service, target))
                .await;
        }
        handle.log(result.clone()).await;
        Ok(result)
    }

    async fn execute_sqlmap(
        &self,
        arguments: &Value,
        handle: &WorkerHandle,
    ) -> Result<String, String> {
        handle.set_status("Injecting").await;
        let url = arguments["url"]
            .as_str()
            .ok_or_else(|| "sqlmap.url is required".to_string())?;
        let result = NativeSqlmap::scan(url).await?;
        let vulns = NativeSqlmap::parse_vulnerabilities(&result);
        {
            let mut graph = self.graph.write().await;
            graph.ingest_sqlmap(url, vulns.clone());
        }
        for vuln in vulns {
            handle.add_loot(vuln).await;
        }
        handle.log(result.clone()).await;
        Ok(result)
    }

    async fn execute_osint(
        &self,
        arguments: &Value,
        handle: &WorkerHandle,
    ) -> Result<String, String> {
        handle.set_status("OSINT").await;
        let tool = arguments["tool"]
            .as_str()
            .ok_or_else(|| "osint.tool is required".to_string())?;
        let target = arguments["target"]
            .as_str()
            .ok_or_else(|| "osint.target is required".to_string())?;
        let result = crate::osint::run(tool, target).await?;
        {
            let mut graph = self.graph.write().await;
            graph.extract_from_note("osint", &result);
        }
        handle
            .add_loot(format!("OSINT {} -> {}", tool, target))
            .await;
        handle.log(result.clone()).await;
        Ok(result)
    }

    async fn execute_hosting(
        &self,
        arguments: &Value,
        handle: &WorkerHandle,
    ) -> Result<String, String> {
        handle.set_status("Hosting").await;
        let action = arguments["action"]
            .as_str()
            .ok_or_else(|| "hosting.action is required".to_string())?;
        let content_path = arguments["content_path"].as_str();
        let result = crate::hosting::control(action, content_path).await?;
        if result.contains("http://127.0.0.1:8000") {
            handle
                .add_loot("Hosted content at http://127.0.0.1:8000")
                .await;
        }
        handle.log(result.clone()).await;
        Ok(result)
    }

    async fn execute_image_gen(
        &self,
        arguments: &Value,
        handle: &WorkerHandle,
    ) -> Result<String, String> {
        handle.set_status("Generating").await;
        let prompt = arguments["prompt"]
            .as_str()
            .ok_or_else(|| "image_gen.prompt is required".to_string())?;
        let model = arguments["model"].as_str();
        let output_file = arguments["output_file"].as_str();
        let result = crate::image_gen::generate(prompt, model, output_file).await?;
        if let Some((_, path)) = result.rsplit_once(": ") {
            handle.add_loot(format!("Generated image {}", path)).await;
        }
        handle.log(result.clone()).await;
        Ok(result)
    }

    async fn execute_evm_chain(
        &self,
        arguments: &Value,
        handle: &WorkerHandle,
    ) -> Result<String, String> {
        handle.set_status("Chain Analysis").await;
        let action = arguments["action"]
            .as_str()
            .ok_or_else(|| "evm_chain.action is required".to_string())?;
        let result = crate::evm_chain::run(
            action,
            arguments["address"].as_str(),
            arguments["rpc_url"].as_str(),
            arguments["network"].as_str(),
            arguments,
        )
        .await?;
        {
            let mut graph = self.graph.write().await;
            graph.extract_from_note("evm_chain", &result);
        }
        if let Some(address) = arguments["address"].as_str() {
            handle
                .add_loot(format!("EVM {} -> {}", action, address))
                .await;
        }
        handle.log(result.clone()).await;
        Ok(result)
    }

    async fn execute_finish(
        &self,
        arguments: &Value,
        plan: &mut [PlanStep],
        handle: &WorkerHandle,
    ) -> Result<String, String> {
        let action = arguments["action"]
            .as_str()
            .ok_or_else(|| "finish.action is required".to_string())?;
        let step_id = arguments["step_id"]
            .as_u64()
            .ok_or_else(|| "finish.step_id is required".to_string())?
            as usize;
        let step = plan
            .iter_mut()
            .find(|step| step.id == step_id)
            .ok_or_else(|| format!("Step {} not found", step_id))?;

        match action {
            "complete" => {
                let result = arguments["result"]
                    .as_str()
                    .unwrap_or("Completed")
                    .to_string();
                step.status = StepStatus::Complete;
                step.result = Some(result.clone());
                handle
                    .log(format!("Step {} complete: {}", step_id, result))
                    .await;
                Ok(format!("Step {} complete", step_id))
            }
            "skip" => {
                let reason = arguments["reason"]
                    .as_str()
                    .unwrap_or("Skipped")
                    .to_string();
                step.status = StepStatus::Skip;
                step.result = Some(reason.clone());
                handle
                    .log(format!("Step {} skipped: {}", step_id, reason))
                    .await;
                Ok(format!("Step {} skipped", step_id))
            }
            "fail" => {
                let reason = arguments["reason"].as_str().unwrap_or("Failed").to_string();
                step.status = StepStatus::Fail;
                step.result = Some(reason.clone());
                handle
                    .log(format!("Step {} failed: {}", step_id, reason))
                    .await;
                Ok(format!("Step {} failed", step_id))
            }
            other => Err(format!("Unsupported finish action: {}", other)),
        }
    }

    async fn summarize(
        &self,
        task: &str,
        plan: &[PlanStep],
        messages: &[Value],
    ) -> Result<String, String> {
        let plan_summary = plan
            .iter()
            .map(|step| {
                format!(
                    "- Step {} [{}]: {} {}",
                    step.id,
                    match step.status {
                        StepStatus::Pending => "PENDING",
                        StepStatus::Complete => "COMPLETE",
                        StepStatus::Skip => "SKIP",
                        StepStatus::Fail => "FAIL",
                    },
                    step.description,
                    step.result.clone().unwrap_or_default()
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        let prompt = format!(
            "Summarize the worker task succinctly.\nTask: {}\n\nPlan outcome:\n{}",
            task, plan_summary
        );
        let mut summary_messages = messages.to_vec();
        summary_messages.push(json!({
            "role": "user",
            "content": prompt
        }));
        self.llm.generate_with_history(summary_messages).await
    }

    async fn replan(&self, task: &str, current_plan: &[PlanStep]) -> Result<Vec<PlanStep>, String> {
        let failed_step = current_plan
            .iter()
            .find(|step| step.status == StepStatus::Fail)
            .ok_or_else(|| "No failed step available for replanning".to_string())?;
        let prior_plan = current_plan
            .iter()
            .map(PlanStep::as_line)
            .collect::<Vec<_>>()
            .join("\n");
        let response = self
            .llm
            .generate_with_tools(
                "You are a tactical replanning assistant. Always call create_plan with a revised plan or feasible=false.",
                vec![json!({
                    "role": "user",
                    "content": format!(
                        "The worker plan failed.\nTask: {}\nFailed step: {}\nFailure detail: {}\nPrevious plan:\n{}",
                        task,
                        failed_step.description,
                        failed_step.result.clone().unwrap_or_default(),
                        prior_plan
                    )
                })],
                vec![json!({
                    "type": "function",
                    "function": {
                        "name": "create_plan",
                        "description": "Create a revised task plan.",
                        "parameters": {
                            "type": "object",
                            "properties": {
                                "feasible": {"type": "boolean"},
                                "reason": {"type": "string"},
                                "steps": {
                                    "type": "array",
                                    "items": {"type": "string"}
                                }
                            },
                            "required": ["feasible", "reason"]
                        }
                    }
                })],
            )
            .await?;

        for tool_call in response.tool_calls {
            if tool_call.name == "create_plan" {
                if !tool_call.arguments["feasible"].as_bool().unwrap_or(true) {
                    return Err(tool_call.arguments["reason"]
                        .as_str()
                        .unwrap_or("Task is infeasible after replanning.")
                        .to_string());
                }
                let steps = tool_call.arguments["steps"]
                    .as_array()
                    .map(|items| {
                        items
                            .iter()
                            .enumerate()
                            .map(|(idx, item)| PlanStep {
                                id: idx + 1,
                                description: item.as_str().unwrap_or("Unnamed step").to_string(),
                                status: StepStatus::Pending,
                                result: None,
                            })
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                if !steps.is_empty() {
                    return Ok(steps);
                }
            }
        }

        Err("Replanning failed to produce a revised plan".to_string())
    }
}

fn worker_tools() -> Vec<Value> {
    vec![
        tool(
            "terminal",
            "Execute a shell command.",
            json!({
                "type": "object",
                "properties": {
                    "command": {"type": "string"},
                    "timeout": {"type": "integer"},
                    "working_dir": {"type": "string"},
                    "inputs": {"type": "string"},
                    "privileged": {"type": "boolean"}
                },
                "required": ["command"]
            }),
        ),
        tool(
            "browser",
            "Navigate or inspect a web page.",
            json!({
                "type": "object",
                "properties": {
                    "action": {"type": "string"},
                    "url": {"type": "string"},
                    "selector": {"type": "string"},
                    "text": {"type": "string"},
                    "javascript": {"type": "string"},
                    "wait_for": {"type": "string"},
                    "timeout": {"type": "integer"}
                },
                "required": ["action"]
            }),
        ),
        tool(
            "web_search",
            "Search the web for target-specific intelligence.",
            json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string"}
                },
                "required": ["query"]
            }),
        ),
        tool(
            "notes",
            "Create, read, or list persistent notes shared with the crew.",
            json!({
                "type": "object",
                "properties": {
                    "action": {"type": "string"},
                    "key": {"type": "string"},
                    "value": {"type": "string"},
                    "category": {"type": "string"},
                    "target": {"type": "string"},
                    "source": {"type": "string"},
                    "username": {"type": "string"},
                    "password": {"type": "string"},
                    "protocol": {"type": "string"},
                    "port": {"type": "string"},
                    "cve": {"type": "string"},
                    "url": {"type": "string"},
                    "evidence_path": {"type": "string"}
                },
                "required": ["action"]
            }),
        ),
        tool(
            "nmap",
            "Run native nmap scanning against a target.",
            json!({
                "type": "object",
                "properties": {
                    "target": {"type": "string"}
                },
                "required": ["target"]
            }),
        ),
        tool(
            "sqlmap",
            "Run native sqlmap against a URL.",
            json!({
                "type": "object",
                "properties": {
                    "url": {"type": "string"}
                },
                "required": ["url"]
            }),
        ),
        tool(
            "osint",
            "Run native OSINT utilities like holehe, sherlock, or theHarvester.",
            json!({
                "type": "object",
                "properties": {
                    "tool": {"type": "string"},
                    "target": {"type": "string"}
                },
                "required": ["tool", "target"]
            }),
        ),
        tool(
            "hosting",
            "Start, stop, or inspect the lightweight local hosting server.",
            json!({
                "type": "object",
                "properties": {
                    "action": {"type": "string"},
                    "content_path": {"type": "string"}
                },
                "required": ["action"]
            }),
        ),
        tool(
            "image_gen",
            "Generate an image locally through the configured image model provider.",
            json!({
                "type": "object",
                "properties": {
                    "prompt": {"type": "string"},
                    "model": {"type": "string"},
                    "output_file": {"type": "string"}
                },
                "required": ["prompt"]
            }),
        ),
        tool(
            "evm_chain",
            "Run EVM chain analysis actions through RPC and explorer APIs.",
            json!({
                "type": "object",
                "properties": {
                    "action": {"type": "string"},
                    "address": {"type": "string"},
                    "rpc_url": {"type": "string"},
                    "network": {"type": "string"},
                    "selector": {"type": "string"},
                    "slot": {"type": "string"},
                    "data": {"type": "string"},
                    "topics": {
                        "type": "array",
                        "items": {"type": "string"}
                    },
                    "from_block": {"type": "string"},
                    "to_block": {"type": "string"},
                    "block_number": {"type": "string"},
                    "tx_hash": {"type": "string"},
                    "offset": {"type": "integer"}
                },
                "required": ["action"]
            }),
        ),
        tool(
            "finish",
            "Mark plan steps complete, skipped, or failed.",
            json!({
                "type": "object",
                "properties": {
                    "action": {"type": "string"},
                    "step_id": {"type": "integer"},
                    "result": {"type": "string"},
                    "reason": {"type": "string"}
                },
                "required": ["action", "step_id"]
            }),
        ),
    ]
}

fn tool(name: &str, description: &str, parameters: Value) -> Value {
    json!({
        "type": "function",
        "function": {
            "name": name,
            "description": description,
            "parameters": parameters
        }
    })
}

fn build_assistant_message(response: &ChatResponse) -> Value {
    let mut message = json!({
        "role": "assistant",
        "content": response.content
    });

    if !response.tool_calls.is_empty() {
        message["tool_calls"] = Value::Array(
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

fn plan_complete(plan: &[PlanStep]) -> bool {
    plan.iter()
        .all(|step| matches!(step.status, StepStatus::Complete | StepStatus::Skip))
}

fn plan_has_failure(plan: &[PlanStep]) -> bool {
    plan.iter().any(|step| step.status == StepStatus::Fail)
}
