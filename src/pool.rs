use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use tokio::sync::{RwLock, mpsc};
use tokio::task::JoinHandle;

use crate::events::UiEvent;
use crate::graph::ShadowGraph;
use crate::llm::NativeLLMEngine;
use crate::notes::NotesEngine;
use crate::web_search::NativeWebSearch;
use crate::worker_agent::{WorkerAgent, WorkerHandle};

#[derive(Clone, Debug)]
pub struct WorkerInfo {
    pub id: String,
    pub task: String,
    pub command: String,
    pub status: String,
    pub logs: Vec<String>,
    pub loot: Vec<String>,
    pub result: Option<String>,
    pub error: Option<String>,
    pub tools_used: Vec<String>,
    pub priority: i64,
    pub depends_on: Vec<String>,
    pub started_at: Option<u64>,
    pub finished_at: Option<u64>,
}

pub struct WorkerPoolState {
    pub workers: HashMap<String, WorkerInfo>,
    pub handles: HashMap<String, JoinHandle<()>>,
    pub next_id: usize,
}

#[derive(Clone)]
pub struct WorkerPool {
    pub state: Arc<RwLock<WorkerPoolState>>,
    llm: Arc<NativeLLMEngine>,
    notes: Arc<NotesEngine>,
    graph: Arc<RwLock<ShadowGraph>>,
    search: Arc<NativeWebSearch>,
    browser: Option<Arc<crate::browser::NativeBrowserEngine>>,
    pub event_tx: mpsc::Sender<String>,
}

impl WorkerPool {
    pub fn new(
        event_tx: mpsc::Sender<String>,
        llm: Arc<NativeLLMEngine>,
        notes: Arc<NotesEngine>,
        graph: Arc<RwLock<ShadowGraph>>,
        search: Arc<NativeWebSearch>,
        browser: Option<Arc<crate::browser::NativeBrowserEngine>>,
    ) -> Self {
        Self {
            state: Arc::new(RwLock::new(WorkerPoolState {
                workers: HashMap::new(),
                handles: HashMap::new(),
                next_id: 0,
            })),
            llm,
            notes,
            graph,
            search,
            browser,
            event_tx,
        }
    }

    pub async fn spawn(&self, task: String, priority: i64, depends_on: Vec<String>) -> String {
        let worker_id = {
            let mut state = self.state.write().await;
            let worker_id = format!("agent-{}", state.next_id);
            state.next_id += 1;
            state.workers.insert(
                worker_id.clone(),
                WorkerInfo {
                    id: worker_id.clone(),
                    task: task.clone(),
                    command: task.clone(),
                    status: "Queued".to_string(),
                    logs: vec![format!("Queued task: {}", task)],
                    loot: Vec::new(),
                    result: None,
                    error: None,
                    tools_used: Vec::new(),
                    priority,
                    depends_on: depends_on.clone(),
                    started_at: None,
                    finished_at: None,
                },
            );
            worker_id
        };

        let _ = self
            .event_tx
            .send(UiEvent::log(format!("Spawned {} -> {}", worker_id, task)).serialize())
            .await;

        let pool = self.clone();
        let worker_id_clone = worker_id.clone();
        let task_clone = task.clone();
        let handle = tokio::spawn(async move {
            pool.run_worker(worker_id_clone, task_clone, depends_on)
                .await;
        });

        let mut state = self.state.write().await;
        state.handles.insert(worker_id.clone(), handle);

        worker_id
    }

    pub async fn wait_for(
        &self,
        agent_ids: Option<Vec<String>>,
    ) -> HashMap<String, serde_json::Value> {
        loop {
            let snapshot = {
                let state = self.state.read().await;
                let worker_ids: Vec<String> = agent_ids
                    .clone()
                    .unwrap_or_else(|| state.workers.keys().cloned().collect());
                let pending = worker_ids.iter().any(|id| {
                    state
                        .workers
                        .get(id)
                        .map(|worker| !is_terminal_status(&worker.status))
                        .unwrap_or(false)
                });

                if pending {
                    None
                } else {
                    Some(
                        worker_ids
                            .into_iter()
                            .filter_map(|id| {
                                state.workers.get(&id).cloned().map(|worker| (id, worker))
                            })
                            .collect::<Vec<_>>(),
                    )
                }
            };

            if let Some(done) = snapshot {
                return done
                    .into_iter()
                    .map(|(id, worker)| {
                        (
                            id,
                            serde_json::json!({
                                "status": worker.status,
                                "task": worker.task,
                                "result": worker.result,
                                "error": worker.error,
                                "tools_used": worker.tools_used,
                            }),
                        )
                    })
                    .collect();
            }

            tokio::time::sleep(tokio::time::Duration::from_millis(250)).await;
        }
    }

    pub async fn cancel(&self, agent_id: &str) -> bool {
        let handle = {
            let mut state = self.state.write().await;
            if let Some(worker) = state.workers.get_mut(agent_id) {
                if is_terminal_status(&worker.status) {
                    return false;
                }
                worker.status = "Cancelled".to_string();
                worker.error = Some("Cancelled by orchestrator".to_string());
                worker.finished_at = Some(now_epoch_secs());
            }
            state.handles.remove(agent_id)
        };

        if let Some(handle) = handle {
            handle.abort();
            let _ = self
                .event_tx
                .send(UiEvent::log(format!("Cancelled {}", agent_id)).serialize())
                .await;
            return true;
        }

        false
    }

    pub async fn get_status(&self, agent_id: &str) -> Option<serde_json::Value> {
        let state = self.state.read().await;
        state.workers.get(agent_id).map(|worker| {
            serde_json::json!({
                "id": worker.id,
                "status": worker.status,
                "task": worker.task,
                "result": worker.result,
                "error": worker.error,
                "tools_used": worker.tools_used,
                "priority": worker.priority,
                "depends_on": worker.depends_on,
            })
        })
    }

    pub async fn get_workers(&self) -> Vec<WorkerInfo> {
        let state = self.state.read().await;
        let mut workers = state.workers.values().cloned().collect::<Vec<_>>();
        workers.sort_by(|a, b| a.id.cmp(&b.id));
        workers
    }

    async fn run_worker(&self, worker_id: String, task: String, depends_on: Vec<String>) {
        if !depends_on.is_empty() {
            self.wait_for(Some(depends_on)).await;
        }

        self.update_worker(&worker_id, |worker| {
            worker.status = "Running".to_string();
            worker.started_at = Some(now_epoch_secs());
            worker.logs.push("Worker runtime booted.".to_string());
        })
        .await;

        let handle =
            WorkerHandle::new(worker_id.clone(), self.state.clone(), self.event_tx.clone());
        let agent = WorkerAgent::new(
            self.llm.clone(),
            self.notes.clone(),
            self.browser.clone(),
            self.search.clone(),
            self.graph.clone(),
        );

        let effective_task = convert_forced_prefix_task(&task).unwrap_or(task.clone());
        let execution = agent.run(&effective_task, &handle).await;

        match execution {
            Ok(result) => {
                self.update_worker(&worker_id, |worker| {
                    worker.status = "Finished".to_string();
                    worker.result = Some(result.clone());
                    worker.finished_at = Some(now_epoch_secs());
                })
                .await;
                let _ = self
                    .event_tx
                    .send(UiEvent::log(format!("{} completed", worker_id)).serialize())
                    .await;
            }
            Err(error) => {
                self.update_worker(&worker_id, |worker| {
                    worker.status = "Error".to_string();
                    worker.error = Some(error.clone());
                    worker.finished_at = Some(now_epoch_secs());
                    worker.logs.push(format!("ERROR: {}", error));
                })
                .await;
                let _ = self
                    .event_tx
                    .send(UiEvent::log(format!("{} failed: {}", worker_id, error)).serialize())
                    .await;
            }
        }
    }

    async fn update_worker<F>(&self, worker_id: &str, mut updater: F)
    where
        F: FnMut(&mut WorkerInfo),
    {
        let mut state = self.state.write().await;
        if let Some(worker) = state.workers.get_mut(worker_id) {
            updater(worker);
        }
    }
}

fn is_terminal_status(status: &str) -> bool {
    matches!(status, "Finished" | "Error" | "Cancelled")
}

fn now_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn convert_forced_prefix_task(task: &str) -> Option<String> {
    let trimmed = task.trim();
    if let Some(target) = trimmed.strip_prefix("NMAP:") {
        Some(format!(
            "Run an nmap scan against {} and capture service findings.",
            target.trim()
        ))
    } else if let Some(url) = trimmed.strip_prefix("SQLMAP:") {
        Some(format!(
            "Run sqlmap against {} and report confirmed findings.",
            url.trim()
        ))
    } else if let Some(query) = trimmed.strip_prefix("SEARCH:") {
        Some(format!(
            "Use web_search to research {} and summarize target-specific intelligence.",
            query.trim()
        ))
    } else if let Some(url) = trimmed.strip_prefix("BROWSER:") {
        Some(format!(
            "Use browser navigation to inspect {} and summarize the page.",
            url.trim()
        ))
    } else if let Some(command) = trimmed.strip_prefix("TERMINAL:") {
        Some(format!(
            "Execute the shell command '{}' and report the result.",
            command.trim()
        ))
    } else if let Some(spec) = trimmed.strip_prefix("OSINT:") {
        Some(format!(
            "Run the requested OSINT workflow for {} and summarize the relevant findings.",
            spec.trim()
        ))
    } else if let Some(spec) = trimmed.strip_prefix("HOSTING:") {
        Some(format!(
            "Use the hosting tool with the requested action for {} and report the exposed URL or server state.",
            spec.trim()
        ))
    } else if let Some(spec) = trimmed.strip_prefix("IMAGE:") {
        Some(format!(
            "Use image_gen to create the requested asset for {} and report the output path.",
            spec.trim()
        ))
    } else if let Some(spec) = trimmed.strip_prefix("EVM:") {
        Some(format!(
            "Use evm_chain to investigate {} and summarize the chain results.",
            spec.trim()
        ))
    } else {
        None
    }
}
