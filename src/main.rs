mod browser;
mod config;
mod events;
mod evm_chain;
mod graph;
mod hosting;
mod image_gen;
mod llm;
mod nmap;
mod notes;
mod orchestrator;
mod osint;
mod pool;
mod prompts;
mod sqlmap;
mod terminal;
mod tui;
mod web_search;
mod worker_agent;

use crate::browser::NativeBrowserEngine;
use crate::llm::NativeLLMEngine;
use crate::notes::NotesEngine;
use crate::orchestrator::Orchestrator;
use crate::pool::WorkerPool;
use crate::web_search::NativeWebSearch;
use std::sync::Arc;
use tokio::sync::mpsc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenv::dotenv().ok();

    let (event_tx, event_rx) = mpsc::channel::<String>(1000);
    let (cmd_tx, mut cmd_rx) = mpsc::channel::<String>(100);
    let graph = Arc::new(tokio::sync::RwLock::new(crate::graph::ShadowGraph::new()));

    let graph_tui = graph.clone();
    let target_shared = Arc::new(tokio::sync::RwLock::new("None".to_string()));
    let target_tui = target_shared.clone();

    // Booting engines first so we can pass them to TUI
    let tx = event_tx.clone();
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    let _ = tx.send("=== Serpantoxide Engine ===".to_string()).await;

    let notes_engine = Arc::new(NotesEngine::launch().await?);
    let llm_engine = Arc::new(NativeLLMEngine::launch().await?);

    let mut browser_engine = None;
    let _ = tx
        .send("Booting Chromiumoxide Native Engine over CDP...".to_string())
        .await;
    match NativeBrowserEngine::launch().await {
        Ok(engine) => {
            let browser = Arc::new(engine);
            let _ = tx
                .send("   -> Chromiumoxide CDP bound successfully!".to_string())
                .await;
            browser_engine = Some(browser);
        }
        Err(e) => {
            let _ = tx
                .send(format!("   [Native Browser Engine Error] {}", e))
                .await;
        }
    }

    let search_key =
        std::env::var("TAVILY_API_KEY").unwrap_or_else(|_| "MOCK_SEARCH_KEY".to_string());
    let search_engine = Arc::new(NativeWebSearch::new(&search_key));
    let worker_pool = WorkerPool::new(
        event_tx.clone(),
        llm_engine.clone(),
        notes_engine.clone(),
        graph.clone(),
        search_engine.clone(),
        browser_engine.clone(),
    );
    let worker_pool_tui = worker_pool.clone();

    let llm_tui = llm_engine.clone();
    let tui_handler = tokio::spawn(async move {
        let _ = crate::tui::run_tui(
            event_rx,
            cmd_tx,
            graph_tui,
            llm_tui,
            target_tui,
            worker_pool_tui,
        )
        .await;
    });

    // Initialize Ported Orchestrator with shared graph
    let orchestrator = Orchestrator::new(
        llm_engine.clone(),
        worker_pool.clone(),
        notes_engine.clone(),
        browser_engine.clone(),
        search_engine,
        graph.clone(),
        target_shared.clone(),
        event_tx.clone(),
    );

    let _ = tx
        .send("=== Initialization Complete. Awaiting Commands... ===".to_string())
        .await;

    // Command Processing Loop
    while let Some(command) = cmd_rx.recv().await {
        let trimmed = command.trim().to_string();
        let parts: Vec<&str> = trimmed.split_whitespace().collect();
        if parts.is_empty() {
            continue;
        }

        match parts[0] {
            "/quit" | "/exit" | "/q" => {
                let _ = tx
                    .send("System Shutdown initiated by user.".to_string())
                    .await;
                break;
            }
            "/help" | "/h" | "/?" => {
                for line in prompts::help_text().lines() {
                    let _ = tx.send(line.to_string()).await;
                }
            }
            "/modes" => {
                for line in prompts::modes_text().lines() {
                    let _ = tx.send(line.to_string()).await;
                }
            }
            "/tools" => {
                for line in prompts::worker_capabilities_text().lines() {
                    let _ = tx.send(line.to_string()).await;
                }
            }
            "/target" => {
                if parts.len() > 1 {
                    let new_target = parts[1..].join(" ");
                    let mut t = target_shared.write().await;
                    *t = new_target.clone();
                    let _ = tx.send(format!("Target set to: {}", new_target)).await;
                } else {
                    let _ = tx.send("Usage: /target <hostname|ip>".to_string()).await;
                }
            }
            "/notes" | "/nodes" => {
                if parts.len() > 1 {
                    let cat = parts[1];
                    let entries = notes_engine.get_notes_by_category(cat).await;
                    let _ = tx
                        .send(format!("--- Notes for category: {} ---", cat))
                        .await;
                    for note in entries {
                        let _ = tx.send(format!("  • {}", note.payload)).await;
                    }
                } else {
                    let cats = notes_engine.list_categories().await;
                    let _ = tx.send("--- Intelligence Categories ---".to_string()).await;
                    for (name, count) in cats {
                        let _ = tx.send(format!("  [{}] ({} findings)", name, count)).await;
                    }
                }
            }
            "/memory" => {
                let insights = graph.read().await.get_strategic_insights();
                let _ = tx.send("--- Strategic Memory ---".to_string()).await;
                for item in insights {
                    let _ = tx.send(format!("  • {}", item)).await;
                }
            }
            "/prompt" => {
                let target = target_shared.read().await.clone();
                let preview = orchestrator
                    .prompt_preview(&target, "Show the current system prompt")
                    .await;
                for line in preview.lines() {
                    let _ = tx.send(line.to_string()).await;
                }
            }
            "/report" => {
                let target = target_shared.read().await.clone();
                match orchestrator.generate_report(&target).await {
                    Ok(report) => {
                        for line in report.lines() {
                            let _ = tx.send(line.to_string()).await;
                        }
                    }
                    Err(e) => {
                        let _ = tx.send(format!("Report generation failed: {}", e)).await;
                    }
                }
            }
            "/agent" => {
                let target = target_shared.read().await.clone();
                let task = if parts.len() > 1 {
                    parts[1..].join(" ")
                } else {
                    "Focused autonomous assessment".to_string()
                };
                let orch = orchestrator.clone();
                tokio::spawn(async move {
                    let _ = orch.run(&target, &task).await;
                });
            }
            "/crew" => {
                let target = {
                    let t = target_shared.read().await;
                    t.clone()
                };
                let task = if parts.len() > 1 {
                    parts[1..].join(" ")
                } else {
                    "Full autonomous assessment".to_string()
                };
                let orch = orchestrator.clone();
                tokio::spawn(async move {
                    let _ = orch.run_swarm_mode(&target, &task).await;
                });
            }
            "/clear" => {
                // TUI currently doesn't have a flush event, but we can send placeholders
                for _ in 0..50 {
                    let _ = tx.send(" ".to_string()).await;
                }
                let _ = tx.send("--- Log Buffered Clear ---".to_string()).await;
            }
            _ => {
                // Default to orchestration
                let _ = tx
                    .send(format!("🚀 Received Instruction: {}", trimmed))
                    .await;
                let orchestrator_task = orchestrator.clone();
                let cmd = trimmed.clone();
                let target = target_shared.read().await.clone();
                match orchestrator_task.run(&target, &cmd).await {
                    Ok(_) => {
                        let _ = tx
                            .send("✅ Orchestrator mission concluded successfully.".to_string())
                            .await;
                    }
                    Err(e) => {
                        let _ = tx.send(format!("❌ Orchestrator Error: {}", e)).await;
                    }
                }
                let _ = tx.send("--- Ready for next command ---".to_string()).await;
            }
        }
    }

    // Await User Input (Q) implicitly halting the pipeline safely
    let _ = tui_handler.await;

    // Fallback dump
    println!("=== System Shutdown ===");
    Ok(())
}
