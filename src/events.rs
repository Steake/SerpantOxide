use serde::{Deserialize, Serialize};

use crate::graph::TopologySnapshot;
use crate::llm::OpenRouterModel;

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
    ReportReady {
        report: String,
    },
    WorkerSpawn {
        worker_id: String,
        task: String,
    },
    WorkerStatus {
        worker_id: String,
        status: String,
    },
    WorkerOutput {
        worker_id: String,
        message: String,
    },
    WorkerTool {
        worker_id: String,
        tool_name: String,
        args: String,
        result: Option<String>,
    },
    TargetUpdated {
        target: String,
    },
    ModelChanged {
        model_id: String,
    },
    ModelsUpdated {
        models: Vec<OpenRouterModel>,
    },
    TelemetryUpdated {
        model: String,
        status: String,
        is_thinking: bool,
        last_latency_ms: u64,
        prompt_tokens: u32,
        completion_tokens: u32,
    },
    TopologyUpdated {
        snapshot: TopologySnapshot,
    },
    NotesUpdated {
        categories: Vec<(String, usize)>,
    },
    LogsCleared,
    ShutdownRequested,
}

impl UiEvent {
    pub fn log<S: Into<String>>(message: S) -> Self {
        Self::Log {
            message: message.into(),
        }
    }

    pub fn worker_spawn<S: Into<String>>(worker_id: S, task: S) -> Self {
        Self::WorkerSpawn {
            worker_id: worker_id.into(),
            task: task.into(),
        }
    }

    pub fn worker_status<S: Into<String>>(worker_id: S, status: S) -> Self {
        Self::WorkerStatus {
            worker_id: worker_id.into(),
            status: status.into(),
        }
    }

    pub fn worker_output<S: Into<String>>(worker_id: S, message: S) -> Self {
        Self::WorkerOutput {
            worker_id: worker_id.into(),
            message: message.into(),
        }
    }
}
