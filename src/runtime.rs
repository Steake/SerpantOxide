use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::{RwLock, broadcast, mpsc};

use crate::browser::NativeBrowserEngine;
use crate::config::AppConfig;
use crate::events::UiEvent;
use crate::graph::{ShadowGraph, TopologySnapshot};
use crate::llm::{LlmTelemetrySnapshot, NativeLLMEngine, OpenRouterModel};
use crate::notes::{Note, NotesEngine};
use crate::orchestrator::Orchestrator;
use crate::pool::{ToolRecord, WorkerInfo, WorkerPool};
use crate::prompts;
use crate::web_search::NativeWebSearch;

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub enum RuntimeCommand {
    SetTarget { target: String },
    RunAgent { task: String },
    RunCrew { task: String },
    GenerateReport,
    SelectModel { model_id: String },
    OpenNotes { category: Option<String> },
    CancelWorker { worker_id: String },
    RetryWorker { worker_id: String },
    ShowPromptPreview,
    ShowTopology,
    ShowMemory,
    ShowTools,
    ShowHelp,
    ShowModes,
    ClearLogs,
    Shutdown,
}

#[derive(Clone, Debug, Default)]
struct RuntimeUiState {
    logs: Vec<String>,
    completed_checklist: Vec<String>,
    remaining_checklist: Vec<String>,
    latest_report: Option<String>,
    last_crew_summary: Option<String>,
    shutdown_requested: bool,
}

#[derive(Clone, Debug, Default)]
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub struct RuntimeWorkerSnapshot {
    pub id: String,
    pub task: String,
    pub command: String,
    pub status: String,
    pub logs: Vec<String>,
    pub loot: Vec<String>,
    pub result: Option<String>,
    pub error: Option<String>,
    pub tools_used: Vec<String>,
    pub tool_history: Vec<ToolRecord>,
    pub priority: i64,
    pub depends_on: Vec<String>,
    pub started_at: Option<u64>,
    pub finished_at: Option<u64>,
}

#[derive(Clone, Debug, Default)]
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub struct RuntimeNoteCategory {
    pub name: String,
    pub count: usize,
}

#[derive(Clone, Debug, Default)]
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub struct RuntimeSnapshot {
    pub target: String,
    pub llm: LlmTelemetrySnapshot,
    pub completed_checklist: Vec<String>,
    pub remaining_checklist: Vec<String>,
    pub activity_log: Vec<String>,
    pub workers: Vec<RuntimeWorkerSnapshot>,
    pub topology: TopologySnapshot,
    pub note_categories: Vec<RuntimeNoteCategory>,
    pub notes_by_category: HashMap<String, Vec<Note>>,
    pub latest_report: Option<String>,
    pub last_crew_summary: Option<String>,
    pub shutdown_requested: bool,
}

impl From<&WorkerInfo> for RuntimeWorkerSnapshot {
    fn from(value: &WorkerInfo) -> Self {
        Self {
            id: value.id.clone(),
            task: value.task.clone(),
            command: value.command.clone(),
            status: value.status.clone(),
            logs: value.logs.clone(),
            loot: value.loot.clone(),
            result: value.result.clone(),
            error: value.error.clone(),
            tools_used: value.tools_used.clone(),
            tool_history: value.tool_history.clone(),
            priority: value.priority,
            depends_on: value.depends_on.clone(),
            started_at: value.started_at,
            finished_at: value.finished_at,
        }
    }
}

#[derive(Clone)]
pub struct RuntimeService {
    command_tx: mpsc::Sender<RuntimeCommand>,
    events_tx: broadcast::Sender<UiEvent>,
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    ui_state: Arc<RwLock<RuntimeUiState>>,
    llm_engine: Arc<NativeLLMEngine>,
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    notes_engine: Arc<NotesEngine>,
    graph: Arc<RwLock<ShadowGraph>>,
    target_shared: Arc<RwLock<String>>,
    worker_pool: WorkerPool,
}

impl RuntimeService {
    pub async fn launch() -> Result<Self, String> {
        let (raw_event_tx, raw_event_rx) = mpsc::channel::<UiEvent>(1000);
        let (events_tx, _) = broadcast::channel::<UiEvent>(2048);
        let (command_tx, command_rx) = mpsc::channel::<RuntimeCommand>(256);
        let ui_state = Arc::new(RwLock::new(RuntimeUiState::default()));

        let persisted_config = AppConfig::load();
        let graph = Arc::new(RwLock::new(ShadowGraph::new()));
        let target_shared = Arc::new(RwLock::new(persisted_config.last_target));

        let notes_engine = Arc::new(NotesEngine::launch().await?);
        let llm_engine = Arc::new(NativeLLMEngine::launch().await?);

        let _ = raw_event_tx
            .send(UiEvent::log("=== Serpantoxide Engine ==="))
            .await;

        let browser_engine = match NativeBrowserEngine::launch().await {
            Ok(engine) => {
                let _ = raw_event_tx
                    .send(UiEvent::log(
                        "Booting Chromiumoxide Native Engine over CDP...",
                    ))
                    .await;
                let _ = raw_event_tx
                    .send(UiEvent::log("   -> Chromiumoxide CDP bound successfully!"))
                    .await;
                Some(Arc::new(engine))
            }
            Err(error) => {
                let _ = raw_event_tx
                    .send(UiEvent::log(format!(
                        "[Native Browser Engine Error] {}. Read-only browser fallback remains available for navigate/get_content/get_links/get_forms.",
                        error
                    )))
                    .await;
                None
            }
        };

        let search_key =
            std::env::var("TAVILY_API_KEY").unwrap_or_else(|_| "MOCK_SEARCH_KEY".to_string());
        let search_engine = Arc::new(NativeWebSearch::new(&search_key));
        let worker_pool = WorkerPool::new(
            raw_event_tx.clone(),
            llm_engine.clone(),
            notes_engine.clone(),
            graph.clone(),
            search_engine.clone(),
            browser_engine.clone(),
        );
        let orchestrator = Orchestrator::new(
            llm_engine.clone(),
            worker_pool.clone(),
            notes_engine.clone(),
            browser_engine.clone(),
            search_engine,
            graph.clone(),
            target_shared.clone(),
            raw_event_tx.clone(),
        );

        tokio::spawn(run_event_pump(
            raw_event_rx,
            events_tx.clone(),
            ui_state.clone(),
            llm_engine.clone(),
            notes_engine.clone(),
            graph.clone(),
        ));

        tokio::spawn(run_command_loop(
            command_rx,
            raw_event_tx.clone(),
            llm_engine.clone(),
            notes_engine.clone(),
            graph.clone(),
            target_shared.clone(),
            worker_pool.clone(),
            orchestrator,
        ));

        emit_initial_state(
            &raw_event_tx,
            &llm_engine,
            &notes_engine,
            &graph,
            &target_shared,
        )
        .await;

        let _ = raw_event_tx
            .send(UiEvent::log(
                "=== Initialization Complete. Awaiting Commands... ===",
            ))
            .await;

        Ok(Self {
            command_tx,
            events_tx,
            ui_state,
            llm_engine,
            notes_engine,
            graph,
            target_shared,
            worker_pool,
        })
    }

    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub async fn send_command(&self, command: RuntimeCommand) -> Result<(), String> {
        self.command_tx
            .send(command)
            .await
            .map_err(|error| error.to_string())
    }

    pub fn command_sender(&self) -> mpsc::Sender<RuntimeCommand> {
        self.command_tx.clone()
    }

    pub fn subscribe(&self) -> broadcast::Receiver<UiEvent> {
        self.events_tx.subscribe()
    }

    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub async fn snapshot(&self) -> RuntimeSnapshot {
        let ui_state = self.ui_state.read().await.clone();
        let target = self.target_shared.read().await.clone();
        let llm = self.llm_engine.telemetry_snapshot().await;
        let workers = self
            .worker_pool
            .get_workers()
            .await
            .iter()
            .map(RuntimeWorkerSnapshot::from)
            .collect::<Vec<_>>();
        let topology = self.graph.read().await.snapshot();
        let note_categories = self
            .notes_engine
            .list_categories()
            .await
            .into_iter()
            .map(|(name, count)| RuntimeNoteCategory { name, count })
            .collect();
        let notes_by_category = self.notes_engine.all_notes().await;

        RuntimeSnapshot {
            target,
            llm,
            completed_checklist: ui_state.completed_checklist,
            remaining_checklist: ui_state.remaining_checklist,
            activity_log: ui_state.logs,
            workers,
            topology,
            note_categories,
            notes_by_category,
            latest_report: ui_state.latest_report,
            last_crew_summary: ui_state.last_crew_summary,
            shutdown_requested: ui_state.shutdown_requested,
        }
    }

    pub fn llm_engine(&self) -> Arc<NativeLLMEngine> {
        self.llm_engine.clone()
    }

    pub fn graph(&self) -> Arc<RwLock<ShadowGraph>> {
        self.graph.clone()
    }

    pub fn target_shared(&self) -> Arc<RwLock<String>> {
        self.target_shared.clone()
    }

    pub fn worker_pool(&self) -> WorkerPool {
        self.worker_pool.clone()
    }
}

async fn emit_initial_state(
    raw_event_tx: &mpsc::Sender<UiEvent>,
    llm_engine: &Arc<NativeLLMEngine>,
    notes_engine: &Arc<NotesEngine>,
    graph: &Arc<RwLock<ShadowGraph>>,
    target_shared: &Arc<RwLock<String>>,
) {
    let llm = llm_engine.telemetry_snapshot().await;
    let _ = raw_event_tx
        .send(UiEvent::TelemetryUpdated {
            model: llm.model.clone(),
            status: llm.status.clone(),
            is_thinking: llm.is_thinking,
            last_latency_ms: llm.last_latency_ms,
            prompt_tokens: llm.prompt_tokens,
            completion_tokens: llm.completion_tokens,
        })
        .await;
    let _ = raw_event_tx
        .send(UiEvent::ModelsUpdated {
            models: llm.available_models.clone(),
        })
        .await;
    let _ = raw_event_tx
        .send(UiEvent::TargetUpdated {
            target: target_shared.read().await.clone(),
        })
        .await;
    let _ = raw_event_tx
        .send(UiEvent::TopologyUpdated {
            snapshot: graph.read().await.snapshot(),
        })
        .await;
    let _ = raw_event_tx
        .send(UiEvent::NotesUpdated {
            categories: notes_engine.list_categories().await,
        })
        .await;
}

async fn run_event_pump(
    mut raw_event_rx: mpsc::Receiver<UiEvent>,
    events_tx: broadcast::Sender<UiEvent>,
    ui_state: Arc<RwLock<RuntimeUiState>>,
    llm_engine: Arc<NativeLLMEngine>,
    notes_engine: Arc<NotesEngine>,
    graph: Arc<RwLock<ShadowGraph>>,
) {
    while let Some(event) = raw_event_rx.recv().await {
        {
            let mut state = ui_state.write().await;
            match &event {
                UiEvent::Log { message } => state.logs.push(message.clone()),
                UiEvent::Checklist {
                    completed,
                    remaining,
                } => {
                    state.completed_checklist = completed.clone();
                    state.remaining_checklist = remaining.clone();
                }
                UiEvent::CrewComplete { summary } => {
                    state.last_crew_summary = Some(summary.clone());
                }
                UiEvent::ReportReady { report } => {
                    state.latest_report = Some(report.clone());
                }
                UiEvent::LogsCleared => state.logs.clear(),
                UiEvent::ShutdownRequested => state.shutdown_requested = true,
                _ => {}
            }
        }

        let _ = events_tx.send(event.clone());

        if matches!(
            event,
            UiEvent::WorkerSpawn { .. }
                | UiEvent::WorkerStatus { .. }
                | UiEvent::WorkerTool { .. }
                | UiEvent::CrewComplete { .. }
                | UiEvent::ReportReady { .. }
                | UiEvent::TargetUpdated { .. }
                | UiEvent::ModelChanged { .. }
                | UiEvent::ModelsUpdated { .. }
        ) {
            let llm = llm_engine.telemetry_snapshot().await;
            let _ = events_tx.send(UiEvent::TelemetryUpdated {
                model: llm.model,
                status: llm.status,
                is_thinking: llm.is_thinking,
                last_latency_ms: llm.last_latency_ms,
                prompt_tokens: llm.prompt_tokens,
                completion_tokens: llm.completion_tokens,
            });
            let _ = events_tx.send(UiEvent::TopologyUpdated {
                snapshot: graph.read().await.snapshot(),
            });
            let _ = events_tx.send(UiEvent::NotesUpdated {
                categories: notes_engine.list_categories().await,
            });
        }
    }
}

async fn run_command_loop(
    mut command_rx: mpsc::Receiver<RuntimeCommand>,
    raw_event_tx: mpsc::Sender<UiEvent>,
    llm_engine: Arc<NativeLLMEngine>,
    notes_engine: Arc<NotesEngine>,
    graph: Arc<RwLock<ShadowGraph>>,
    target_shared: Arc<RwLock<String>>,
    worker_pool: WorkerPool,
    orchestrator: Orchestrator,
) {
    while let Some(command) = command_rx.recv().await {
        if let Err(error) = handle_command(
            command,
            &raw_event_tx,
            &llm_engine,
            &notes_engine,
            &graph,
            &target_shared,
            &worker_pool,
            &orchestrator,
        )
        .await
        {
            let _ = raw_event_tx.send(UiEvent::log(error)).await;
        }
    }
}

async fn handle_command(
    command: RuntimeCommand,
    raw_event_tx: &mpsc::Sender<UiEvent>,
    llm_engine: &Arc<NativeLLMEngine>,
    notes_engine: &Arc<NotesEngine>,
    graph: &Arc<RwLock<ShadowGraph>>,
    target_shared: &Arc<RwLock<String>>,
    worker_pool: &WorkerPool,
    orchestrator: &Orchestrator,
) -> Result<(), String> {
    match command {
        RuntimeCommand::SetTarget { target } => {
            {
                let mut shared = target_shared.write().await;
                *shared = target.clone();
            }
            let mut config = AppConfig::load();
            config.last_target = target.clone();
            config.save()?;

            raw_event_tx
                .send(UiEvent::TargetUpdated {
                    target: target.clone(),
                })
                .await
                .map_err(|error| error.to_string())?;
            raw_event_tx
                .send(UiEvent::log(format!("Target set to: {}", target)))
                .await
                .map_err(|error| error.to_string())?;
        }
        RuntimeCommand::RunAgent { task } => {
            let target = target_shared.read().await.clone();
            let effective_task = if !target.trim().is_empty()
                && target != "None"
                && !task.to_lowercase().contains(&target.to_lowercase())
            {
                format!("Active target: {}\nObjective: {}", target, task)
            } else {
                task.clone()
            };
            let worker_id = worker_pool.spawn(effective_task, 1, Vec::new()).await;
            raw_event_tx
                .send(UiEvent::log(format!(
                    "Started autonomous single-agent run: {}",
                    worker_id
                )))
                .await
                .map_err(|error| error.to_string())?;
        }
        RuntimeCommand::RunCrew { task } => {
            let target = target_shared.read().await.clone();
            let orch = orchestrator.clone();
            let tx = raw_event_tx.clone();
            tokio::spawn(async move {
                if let Err(error) = orch.run_swarm_mode(&target, &task).await {
                    let _ = tx
                        .send(UiEvent::log(format!("Crew run failed: {}", error)))
                        .await;
                }
            });
        }
        RuntimeCommand::GenerateReport => {
            let target = target_shared.read().await.clone();
            let report = orchestrator.generate_report(&target).await?;
            raw_event_tx
                .send(UiEvent::ReportReady { report })
                .await
                .map_err(|error| error.to_string())?;
        }
        RuntimeCommand::SelectModel { model_id } => {
            llm_engine.set_model(model_id.clone()).await?;
            let llm = llm_engine.telemetry_snapshot().await;
            raw_event_tx
                .send(UiEvent::ModelChanged {
                    model_id: model_id.clone(),
                })
                .await
                .map_err(|error| error.to_string())?;
            raw_event_tx
                .send(UiEvent::ModelsUpdated {
                    models: llm.available_models,
                })
                .await
                .map_err(|error| error.to_string())?;
            raw_event_tx
                .send(UiEvent::log(format!("Model set to: {}", model_id)))
                .await
                .map_err(|error| error.to_string())?;
        }
        RuntimeCommand::OpenNotes { category } => {
            if let Some(category) = category {
                let entries = notes_engine.get_notes_by_category(&category).await;
                raw_event_tx
                    .send(UiEvent::log(format!(
                        "--- Notes for category: {} ---",
                        category
                    )))
                    .await
                    .map_err(|error| error.to_string())?;
                for note in entries {
                    raw_event_tx
                        .send(UiEvent::log(format!("  • {}", note.payload)))
                        .await
                        .map_err(|error| error.to_string())?;
                }
            } else {
                raw_event_tx
                    .send(UiEvent::NotesUpdated {
                        categories: notes_engine.list_categories().await,
                    })
                    .await
                    .map_err(|error| error.to_string())?;
                raw_event_tx
                    .send(UiEvent::log("--- Intelligence Categories ---"))
                    .await
                    .map_err(|error| error.to_string())?;
                for (name, count) in notes_engine.list_categories().await {
                    raw_event_tx
                        .send(UiEvent::log(format!("  [{}] ({} findings)", name, count)))
                        .await
                        .map_err(|error| error.to_string())?;
                }
            }
        }
        RuntimeCommand::CancelWorker { worker_id } => {
            let cancelled = worker_pool.cancel(&worker_id).await;
            let message = if cancelled {
                format!("Cancelled {}", worker_id)
            } else {
                format!("Could not cancel {}", worker_id)
            };
            raw_event_tx
                .send(UiEvent::log(message))
                .await
                .map_err(|error| error.to_string())?;
        }
        RuntimeCommand::RetryWorker { worker_id } => {
            let worker = worker_pool
                .get_worker(&worker_id)
                .await
                .ok_or_else(|| format!("{} not found", worker_id))?;
            let new_worker = worker_pool
                .spawn(
                    worker.task.clone(),
                    worker.priority,
                    worker.depends_on.clone(),
                )
                .await;
            raw_event_tx
                .send(UiEvent::log(format!(
                    "Retried {} as {}",
                    worker_id, new_worker
                )))
                .await
                .map_err(|error| error.to_string())?;
        }
        RuntimeCommand::ShowPromptPreview => {
            let target = target_shared.read().await.clone();
            let preview = orchestrator
                .prompt_preview(&target, "Show the current system prompt")
                .await;
            for line in preview.lines() {
                raw_event_tx
                    .send(UiEvent::log(line.to_string()))
                    .await
                    .map_err(|error| error.to_string())?;
            }
        }
        RuntimeCommand::ShowTopology => {
            let topology = graph.read().await.to_ascii_topology(120, 40);
            raw_event_tx
                .send(UiEvent::TopologyUpdated {
                    snapshot: graph.read().await.snapshot(),
                })
                .await
                .map_err(|error| error.to_string())?;
            for line in topology.lines() {
                raw_event_tx
                    .send(UiEvent::log(line.to_string()))
                    .await
                    .map_err(|error| error.to_string())?;
            }
        }
        RuntimeCommand::ShowMemory => {
            raw_event_tx
                .send(UiEvent::log("--- Strategic Memory ---"))
                .await
                .map_err(|error| error.to_string())?;
            for item in graph.read().await.get_strategic_insights() {
                raw_event_tx
                    .send(UiEvent::log(format!("  • {}", item)))
                    .await
                    .map_err(|error| error.to_string())?;
            }
        }
        RuntimeCommand::ShowTools => {
            for line in prompts::worker_capabilities_text().lines() {
                raw_event_tx
                    .send(UiEvent::log(line.to_string()))
                    .await
                    .map_err(|error| error.to_string())?;
            }
        }
        RuntimeCommand::ShowHelp => {
            for line in prompts::help_text().lines() {
                raw_event_tx
                    .send(UiEvent::log(line.to_string()))
                    .await
                    .map_err(|error| error.to_string())?;
            }
        }
        RuntimeCommand::ShowModes => {
            for line in prompts::modes_text().lines() {
                raw_event_tx
                    .send(UiEvent::log(line.to_string()))
                    .await
                    .map_err(|error| error.to_string())?;
            }
        }
        RuntimeCommand::ClearLogs => {
            raw_event_tx
                .send(UiEvent::LogsCleared)
                .await
                .map_err(|error| error.to_string())?;
        }
        RuntimeCommand::Shutdown => {
            raw_event_tx
                .send(UiEvent::log("System Shutdown initiated by user."))
                .await
                .map_err(|error| error.to_string())?;
            raw_event_tx
                .send(UiEvent::ShutdownRequested)
                .await
                .map_err(|error| error.to_string())?;
        }
    }

    Ok(())
}

pub fn parse_slash_command(input: &str) -> Result<RuntimeCommand, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("Empty command".to_string());
    }

    let parts = trimmed.split_whitespace().collect::<Vec<_>>();
    match parts[0] {
        "/quit" | "/exit" | "/q" => Ok(RuntimeCommand::Shutdown),
        "/help" | "/h" | "/?" => Ok(RuntimeCommand::ShowHelp),
        "/modes" => Ok(RuntimeCommand::ShowModes),
        "/tools" => Ok(RuntimeCommand::ShowTools),
        "/target" => {
            if parts.len() <= 1 {
                Err("Usage: /target <hostname|ip>".to_string())
            } else {
                Ok(RuntimeCommand::SetTarget {
                    target: parts[1..].join(" "),
                })
            }
        }
        "/notes" | "/nodes" => Ok(RuntimeCommand::OpenNotes {
            category: parts.get(1).map(|value| value.to_string()),
        }),
        "/memory" => Ok(RuntimeCommand::ShowMemory),
        "/prompt" => Ok(RuntimeCommand::ShowPromptPreview),
        "/report" => Ok(RuntimeCommand::GenerateReport),
        "/agent" => {
            if parts.len() <= 1 {
                Err("Usage: /agent <task>".to_string())
            } else {
                Ok(RuntimeCommand::RunAgent {
                    task: parts[1..].join(" "),
                })
            }
        }
        "/crew" => Ok(RuntimeCommand::RunCrew {
            task: if parts.len() > 1 {
                parts[1..].join(" ")
            } else {
                "Full autonomous assessment".to_string()
            },
        }),
        "/clear" => Ok(RuntimeCommand::ClearLogs),
        other => Err(format!("Unknown command: {}", other)),
    }
}

pub fn parse_operator_input(input: &str) -> Result<RuntimeCommand, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("Empty command".to_string());
    }

    if trimmed.starts_with('/') {
        parse_slash_command(trimmed)
    } else {
        Ok(RuntimeCommand::RunCrew {
            task: trimmed.to_string(),
        })
    }
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn runtime_models_to_options(models: &[OpenRouterModel]) -> Vec<String> {
    models
        .iter()
        .map(|model| format!("{} ({})", model.name, model.id))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn worker() -> WorkerInfo {
        WorkerInfo {
            id: "agent-1".to_string(),
            task: "scan".to_string(),
            command: "scan".to_string(),
            status: "Finished".to_string(),
            logs: vec!["boot".to_string()],
            loot: vec!["finding".to_string()],
            result: Some("ok".to_string()),
            error: None,
            tools_used: vec!["nmap".to_string()],
            tool_history: vec![ToolRecord {
                id: 1,
                name: "nmap".to_string(),
                args: "{}".to_string(),
                result: Some("done".to_string()),
            }],
            priority: 1,
            depends_on: vec![],
            started_at: Some(1),
            finished_at: Some(2),
        }
    }

    #[test]
    fn parses_agent_command() {
        let command = parse_slash_command("/agent enumerate ssh").unwrap();
        assert_eq!(
            command,
            RuntimeCommand::RunAgent {
                task: "enumerate ssh".to_string()
            }
        );
    }

    #[test]
    fn parses_default_crew_command() {
        let command = parse_slash_command("/crew").unwrap();
        assert_eq!(
            command,
            RuntimeCommand::RunCrew {
                task: "Full autonomous assessment".to_string()
            }
        );
    }

    #[test]
    fn rejects_missing_target_argument() {
        let error = parse_slash_command("/target").unwrap_err();
        assert!(error.contains("Usage"));
    }

    #[test]
    fn parses_plain_text_as_crew_task() {
        let command = parse_operator_input("enumerate ssh and web").unwrap();
        assert_eq!(
            command,
            RuntimeCommand::RunCrew {
                task: "enumerate ssh and web".to_string()
            }
        );
    }

    #[test]
    fn preserves_multiline_plain_text_task() {
        let command = parse_operator_input("  map attack surface\nfind creds  ").unwrap();
        assert_eq!(
            command,
            RuntimeCommand::RunCrew {
                task: "map attack surface\nfind creds".to_string()
            }
        );
    }

    #[test]
    fn adapts_worker_into_runtime_snapshot() {
        let snapshot = RuntimeWorkerSnapshot::from(&worker());
        assert_eq!(snapshot.id, "agent-1");
        assert_eq!(snapshot.tool_history.len(), 1);
        assert_eq!(snapshot.loot, vec!["finding".to_string()]);
    }
}
