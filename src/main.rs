mod pool;
mod browser;
mod notes;
mod llm;
mod tui;
mod orchestrator;
mod terminal;
mod web_search;
mod graph;
mod nmap;
mod sqlmap;
mod config;
mod python_vm;

use tokio::sync::mpsc;
use crate::pool::WorkerPool;
use crate::browser::NativeBrowserEngine;
use crate::notes::NotesEngine;
use crate::llm::NativeLLMEngine;
use crate::orchestrator::Orchestrator;
use crate::web_search::NativeWebSearch;
use std::sync::Arc;

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
    let _ = tx.send("Booting Chromiumoxide Native Engine over CDP...".to_string()).await;
    match NativeBrowserEngine::launch().await {
        Ok(engine) => {
            let browser = Arc::new(engine);
            let _ = tx.send("   -> Chromiumoxide CDP bound successfully!".to_string()).await;
            browser_engine = Some(browser);
        },
        Err(e) => {
            let _ = tx.send(format!("   [Native Browser Engine Error] {}", e)).await;
        }
    }

    let search_key = std::env::var("TAVILY_API_KEY").unwrap_or_else(|_| "MOCK_SEARCH_KEY".to_string());
    let search_engine = Arc::new(NativeWebSearch::new(&search_key));
    let worker_pool = WorkerPool::new(event_tx.clone(), graph.clone(), search_engine.clone(), browser_engine.clone());
    let worker_pool_tui = worker_pool.clone();

    let llm_tui = llm_engine.clone();
    let tui_handler = tokio::spawn(async move {
        let _ = crate::tui::run_tui(event_rx, cmd_tx, graph_tui, llm_tui, target_tui, worker_pool_tui).await;
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

    let _ = tx.send("=== Initialization Complete. Awaiting Commands... ===".to_string()).await;

    // Command Processing Loop
    while let Some(command) = cmd_rx.recv().await {
        let parts: Vec<&str> = command.split_whitespace().collect();
        if parts.is_empty() { continue; }

        match parts[0] {
            "/quit" | "/exit" | "/q" => {
                let _ = tx.send("System Shutdown initiated by user.".to_string()).await;
                break;
            }
            "/help" | "/h" | "/?" => {
                let _ = tx.send("--- Command Help ---".to_string()).await;
                let _ = tx.send("/agent <task>   - Autonomous agent loop".to_string()).await;
                let _ = tx.send("/crew <task>    - Multi-agent swarm mode".to_string()).await;
                let _ = tx.send("/target <host>  - Set mission target".to_string()).await;
                let _ = tx.send("/notes [cat]    - View findings".to_string()).await;
                let _ = tx.send("/model          - Switch LLM interactively".to_string()).await;
                let _ = tx.send("/topology       - Redraw network map".to_string()).await;
                let _ = tx.send("/report         - Generate session summary".to_string()).await;
                let _ = tx.send("/clear          - Clear terminal telemetry".to_string()).await;
                let _ = tx.send("/quit           - Shutdown system".to_string()).await;
            }
            "/target" => {
                if parts.len() > 1 {
                    let new_target = parts[1].to_string();
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
                    let _ = tx.send(format!("--- Notes for category: {} ---", cat)).await;
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
            "/report" => {
                let _ = tx.send("=== MISSION REPORT SUMMARY ===".to_string()).await;
                let cats = notes_engine.list_categories().await;
                for (name, count) in cats {
                    let _ = tx.send(format!("Category [{}]: {} discoveries matched.", name, count)).await;
                }
                let _ = tx.send("--- End of Report ---".to_string()).await;
            }
            "/crew" => {
                let target = {
                    let t = target_shared.read().await;
                    t.clone()
                };
                let task = if parts.len() > 1 { parts[1..].join(" ") } else { "Full autonomous assessment".to_string() };
                let orch = orchestrator.clone();
                tokio::spawn(async move {
                    let _ = orch.run_swarm_mode(&target, &task).await;
                });
            }
            "/clear" => {
                // TUI currently doesn't have a flush event, but we can send placeholders
                for _ in 0..50 { let _ = tx.send(" ".to_string()).await; }
                let _ = tx.send("--- Log Buffered Clear ---".to_string()).await;
            }
            _ => {
                // Default to orchestration
                let _ = tx.send(format!("🚀 Received Instruction: {}", command)).await;
                let orchestrator_task = orchestrator.clone();
                let cmd = command.clone();
                match orchestrator_task.run("User-Session", &cmd).await {
                    Ok(_) => { let _ = tx.send("✅ Orchestrator mission concluded successfully.".to_string()).await; },
                    Err(e) => { let _ = tx.send(format!("❌ Orchestrator Error: {}", e)).await; }
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
