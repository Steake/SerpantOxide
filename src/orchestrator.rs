use std::sync::Arc;
use tokio::sync::mpsc::Sender;
use tokio::sync::RwLock;
use crate::llm::NativeLLMEngine;
use crate::pool::WorkerPool;
use crate::notes::NotesEngine;
use crate::browser::NativeBrowserEngine;
use crate::terminal::NativeTerminal;
use crate::web_search::NativeWebSearch;
use crate::graph::{ShadowGraph};
use crate::python_vm::{PythonExecutor};
use serde_json::json;

use crate::nmap::NativeNmap;
use crate::sqlmap::NativeSqlmap;
use chrono::Local;

#[derive(Clone)]
pub struct Orchestrator {
    llm: Arc<NativeLLMEngine>,
    pool: WorkerPool,
    notes: Arc<NotesEngine>,
    browser: Option<Arc<NativeBrowserEngine>>,
    search: Arc<NativeWebSearch>,
    graph: Arc<RwLock<ShadowGraph>>,
    target_shared: Arc<RwLock<String>>,
    tx: Sender<String>,
    python: Arc<PythonExecutor>,
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
        tx: Sender<String>,
    ) -> Self {
        let python = Arc::new(PythonExecutor::new());

        Self { 
            llm, 
            pool, 
            notes, 
            browser, 
            search,
            graph,
            target_shared,
            tx,
            python
        }
    }

    pub async fn run(&self, target: &str, task: &str) -> Result<(), String> {
        let mission_name = if task.len() > 30 { format!("{}...", &task[..30]) } else { task.to_string() };
        {
            let mut t = self.target_shared.write().await;
            *t = mission_name.clone();
        }

        let mut history = vec![
            json!({
                "role": "user",
                "content": format!("Target: {}\n\nTask: {}", target, task)
            })
        ];
 
        let mut iteration = 0;
        let max_iterations = 10;
        let mut current_plan = String::new();
        let mut consecutive_searches = 0;
        let mut previous_commands_history: Vec<String> = Vec::new();

        let _ = self.tx.send(format!("🚀 Launching Autonomous Orchestrator: {}", mission_name)).await;

        while iteration < max_iterations {
            iteration += 1;
            let _ = self.tx.send(format!("--- Cycle {}/{} ---", iteration, max_iterations)).await;

            // Context Culling (Anti-Drift)
            if history.len() > 15 {
                let skip_count = history.len() - 10;
                let mut culled = vec![history[0].clone()]; // Keep initial context
                let tail: Vec<_> = history.into_iter().skip(skip_count).collect();
                culled.extend(tail);
                history = culled;
                let _ = self.tx.send("🧹 Context window culled to maintain focus.".to_string()).await;
            }

            // Step 1: Strategic Intelligence Analysis (Memory Injection)
            let insights = self.graph.read().await.get_strategic_insights();
            let system_prompt = self.build_system_prompt(target, &insights, &current_plan).await;
            
            // Step 2: Reasoning & Action Selection
            let mut messages = vec![json!({"role": "system", "content": system_prompt})];
            messages.extend(history.clone());

            let response = self.llm.generate_with_history(messages).await?;
            
            // Tactical Reasoning Truncation (Extract ANALYSIS)
            let analysis_block = response.lines()
                .skip_while(|l| !l.starts_with("ANALYSIS:"))
                .take_while(|l| !l.starts_with("PLAN:"))
                .map(|l| l.strip_prefix("ANALYSIS:").unwrap_or(l).trim())
                .collect::<Vec<_>>()
                .join(" ");

            let summary = if analysis_block.is_empty() { 
                "Proceeding with next step.".to_string() 
            } else if analysis_block.len() > 120 { 
                format!("{}...", &analysis_block[..120]) 
            } else { 
                analysis_block.clone() 
            };
            let _ = self.tx.send(format!("🧠 {}", summary)).await;

            // Step 3: Tool Execution & Plan Extraction
            let plan_lines: Vec<&str> = response.lines()
                .skip_while(|l| !l.starts_with("PLAN:"))
                .take_while(|l| !l.starts_with("ACTION:"))
                .collect();
            
            if !plan_lines.is_empty() {
                current_plan = plan_lines.join("\n");
                let _ = self.tx.send("📝 Mission Strategy Updated.".to_string()).await;
            }

            let mut tool_used = false;
            for line in response.lines().skip_while(|l| !l.starts_with("ACTION:")) {
                let trimmed = line.trim();
                let is_background = trimmed.starts_with("BACKGROUND:");
                let cmd_part = if is_background {
                    trimmed.strip_prefix("BACKGROUND:").unwrap_or(trimmed).trim()
                } else if trimmed.starts_with("ACTION:") {
                    trimmed.strip_prefix("ACTION:").unwrap_or(trimmed).trim()
                } else {
                    trimmed
                };

                if cmd_part.contains("FINISH") {
                    let _ = self.tx.send("🏁 Objective achieved. Concluding mission...".to_string()).await;
                    return Ok(());
                }

                // Universal Agent Delegation
                let tool_type = if cmd_part.contains("NMAP:") { "NMAP" }
                    else if cmd_part.contains("SQLMAP:") { "SQLMAP" }
                    else if cmd_part.contains("SEARCH:") { "SEARCH" }
                    else if cmd_part.contains("BROWSER:") { "BROWSER" }
                    else if cmd_part.contains("TERMINAL:") { "TERMINAL" }
                    else { continue; };

                // Anti-Looping Guardrail
                let raw_cmd = cmd_part.to_string();
                if previous_commands_history.contains(&raw_cmd) {
                     let _ = self.tx.send("⚠️ Detected repetition. Intervening...".to_string()).await;
                     history.push(json!({"role": "system", "content": "⚠️ You are repeating the exact same command. Abort this approach and try alternative tactics."}));
                     continue;
                }
                previous_commands_history.push(raw_cmd.clone());
                tool_used = true;

                consecutive_searches = if tool_type == "SEARCH" { consecutive_searches + 1 } else { 0 };

                if tool_type == "SEARCH" && consecutive_searches >= 3 {
                    let _ = self.tx.send("⚠️ Search Threshold Exceeded. Forcing direct target interaction...".to_string()).await;
                    history.push(json!({"role": "system", "content": "You are over-researching. You MUST now use NMAP, SQLMAP, or BROWSER on the target directly."}));
                    continue;
                }

                let wid = self.pool.spawn(cmd_part.to_string()).await;
                if is_background {
                     let _ = self.tx.send(format!("⚙️ Dispatching Background Agent: {} [{}]", tool_type, wid)).await;
                } else {
                     let _ = self.tx.send(format!("🚀 Activity: [{}] delegated to {}", tool_type, wid)).await;
                     let result = self.wait_for_worker(&wid).await;
                     history.push(json!({"role": "system", "content": format!("{} Result ({}): {}", tool_type, wid, result)}));
                }
            }

            if tool_used {
                history.push(json!({"role": "assistant", "content": response}));
            } else {
                let workers_active = {
                    let s = self.pool.state.read().await;
                    s.workers.values().any(|w| w.status == "Scanning" || w.status == "Injecting" || w.status == "Searching")
                };
                if workers_active {
                    let _ = self.tx.send("⏳ Background operations in progress. Observing intelligence flow...".to_string()).await;
                }
                
                history.push(json!({"role": "assistant", "content": response}));
                if iteration > 5 && !workers_active { break; }
            }
        }

        let _ = self.tx.send("Orchestrator session concluded successfully.".to_string()).await;
        Ok(())
    }

    async fn wait_for_worker(&self, wid: &str) -> String {
        loop {
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
            let s = self.pool.state.read().await;
            if let Some(w) = s.workers.get(wid) {
                if w.status == "Finished" {
                    return w.logs.join("\n");
                }
            } else {
                return "Agent failed to initialize.".to_string();
            }
        }
    }

    async fn process_nmap_output(&self, host: &str, output: &str) {
        let ports = NativeNmap::parse_discovered_ports(output);
        let mut g = self.graph.write().await;
        g.ingest_nmap(host, ports);
    }

    async fn process_sqlmap_output(&self, url: &str, output: &str) {
        let vulns = NativeSqlmap::parse_vulnerabilities(output);
        let mut g = self.graph.write().await;
        g.ingest_sqlmap(url, vulns);
    }

    async fn process_terminal_output(&self, cmd: &str, output: &str) {
        let mut g = self.graph.write().await;
        g.extract_from_note("terminal", output);
        let _ = self.notes.execute("insert", &format!("cmd_{}", cmd.chars().take(5).collect::<String>()), output).await;
    }

    async fn build_system_prompt(&self, target: &str, insights: &[String], plan: &str) -> String {
        let now = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        let os = std::env::consts::OS;
        let arch = std::env::consts::ARCH;
        let user = std::env::var("USER").unwrap_or_else(|_| "root".to_string());

        format!(
            "You are the Serpantoxide Elite Orchestrator – an autonomous offensive security agent. \n\
             [PHASE: RECON & EXPLOIT]\n\
             LOCAL CONTEXT (Grounding):\n\
             - OS: {}\n\
             - Arch: {}\n\
             - User: {}\n\
             - Time: {}\n\n\
             [OBJECTIVE]: Compromise {}\n\n\
             [CONSTRAINTS & RULES]:\n\
             1. NO DRIFTING: Use tools directly against the target. Never search for generic tutorials.\n\
             2. REFLECTION REQUIRED: You MUST always format your output with three explicit blocks before executing logic. \n\
             3. NEVER REPEAT: If a command fails, do not execute the exact same command. Alter your tactics.\n\n\
             [INTELLIGENCE MEMORY]:\n{}\n\n\
             [CURRENT PLAN TRACKER]:\n{}\n\n\
             [TOOLSET COMMANDS]:\n\
             - SEARCH: <query> (Target-specific discovery only)\n\
             - NMAP: <host> (Fast protocol discovery)\n\
             - SQLMAP: <url> (Automated injection testing)\n\
             - BROWSER: <url> (Native web interaction & rendering)\n\
             - TERMINAL: <cmd> (Custom local tool execution)\n\
             - FINISH (Objective achieved)\n\n\
             FORMAT RESTRICTION: Your response MUST STRICTLY follow this format:\n\
             ANALYSIS: [1-2 sentences on what the last tool output means or changes]\n\
             PLAN: \n[ ] 1. Next step\n[ ] 2. Following step\n\
             ACTION: [A command from the toolset above. Or BACKGROUND: <cmd> to not wait for it]\n",
            os, arch, user, now, target, insights.join("\n"), plan
        )
    }

    pub async fn generate_report(&self, target: &str) -> Result<String, String> {
        let _ = self.tx.send("📑 Compiling Intelligence for Executive Report...".to_string()).await;
        
        let insights = self.graph.read().await.get_strategic_insights().join("\n");
        // Simplified note retrieval placeholder for report
        let notes_data = "Loot gathered during operations...".to_string(); 

        let prompt = format!(
            "You are an expert offensive security reporting engine. \
             Generate a comprehensive Markdown penetration test report for the target: {}.\n\n\
             INTELLIGENCE TOPOLOGY GRAPH:\n{}\n\n\
             DISCOVERED LOOT:\n{}\n\n\
             The report should include an Executive Summary, Discovered Scope/Attack Surface, High-Level Vulnerabilities (if any), and Recommendations. Ensure proper Markdown headers (#, ##) and bullet points.", 
            target, insights, notes_data
        );

        let messages = vec![json!({"role": "system", "content": prompt})];
        self.llm.generate_with_history(messages).await
    }
    pub async fn run_swarm_mode(&self, target: &str, task: &str) -> Result<(), String> {
        let _ = self.tx.send("🚀 [Crew Mode] Initializing Strategic Python Orchestrator...".to_string()).await;
        
        // Load the crew script (we'll embed a simple loader or read from file)
        let script = match std::fs::read_to_string("python_assets/crew_swarm.py") {
            Ok(s) => s,
            Err(e) => return Err(format!("Failed to load swarm script: {}", e)),
        };

        let python = self.python.clone();
        let target_str = target.to_string();
        let task_str = task.to_string();

        match tokio::task::spawn_blocking(move || {
            python.call_function(&script, "run_swarm", &target_str, &task_str)
        }).await {
            Ok(inner_res) => match inner_res {
                Ok(json_res) => {
                    let _ = self.tx.send(format!("🧠 Swarm Strategy Logic Resolved: {}", json_res)).await;
                    
                    // Parse the task list
                    if let Ok(tasks) = serde_json::from_str::<Vec<String>>(&json_res) {
                        for task in tasks {
                            let wid = self.pool.spawn(task.clone()).await;
                            let _ = self.tx.send(format!("🚀 [Swarm Dispatch] Task '{}' assigned to Worker {}", task, wid)).await;
                        }
                        Ok(())
                    } else {
                        let _ = self.tx.send("❌ Failed to parse strategy mission plan.".to_string()).await;
                        Err("Invalid strategy output".to_string())
                    }
                },
                Err(e) => {
                    let _ = self.tx.send(format!("❌ Strategic Layer Error: {}", e)).await;
                    Err(e)
                }
            },
            Err(e) => {
                let err_msg = format!("Task Spawn Error: {}", e);
                let _ = self.tx.send(format!("❌ {}", err_msg)).await;
                Err(err_msg)
            }
        }
    }
}
