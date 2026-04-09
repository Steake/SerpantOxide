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
        Self { 
            llm, 
            pool, 
            notes, 
            browser, 
            search,
            graph,
            target_shared,
            tx 
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

        let _ = self.tx.send(format!("🚀 Launching Autonomous Orchestrator: {}", mission_name)).await;

        while iteration < max_iterations {
            iteration += 1;
            let _ = self.tx.send(format!("--- Cycle {}/{} ---", iteration, max_iterations)).await;

            // Step 1: Strategic Intelligence Analysis
            let insights = self.graph.read().await.get_strategic_insights();
            let system_prompt = self.build_system_prompt(target, &insights, &current_plan).await;
            
            // Step 2: Reasoning & Action Selection
            let mut messages = vec![json!({"role": "system", "content": system_prompt})];
            messages.extend(history.clone());

            let response = self.llm.generate_with_history(messages).await?;
            
            // Tactical Reasoning Truncation
            let tactical_reasoning = response.lines()
                .find(|l| l.to_lowercase().contains("reasoning:"))
                .unwrap_or(response.lines().next().unwrap_or(""));
            let summary = if tactical_reasoning.len() > 120 { format!("{}...", &tactical_reasoning[..120]) } else { tactical_reasoning.to_string() };
            let _ = self.tx.send(format!("🧠 {}", summary)).await;

            // Step 3: Tool Execution & Plan Extraction
            if response.contains("PLAN:") && current_plan.is_empty() {
                current_plan = response.clone();
                let _ = self.tx.send("📝 Mission Strategy established.".to_string()).await;
            }

            let mut tool_used = false;
            for line in response.lines() {
                let trimmed = line.trim();
                let is_background = trimmed.starts_with("BACKGROUND:");
                let cmd_part = if is_background {
                    trimmed.strip_prefix("BACKGROUND:").unwrap_or(trimmed).trim()
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
            "You are the Serpantoxide Elite Orchestrator – an autonomous, high-efficiency offensive security agent. \n\
             LOCAL CONTEXT (Grounding):\n\
             - OS: {}\n\
             - Arch: {}\n\
             - User: {}\n\
             - Time: {}\n\n\
             Target: {}.\n\n\
             ROLE DIRECTIVES:\n\
             1. DIRECT ACTION: You are an expert. Never search for generic attack 'techniques' or academic definitions. \
                Use SEARCH only for target-specific intelligence (e.g., 'CVE for [version]') or discovering specific exploits for found services.\n\
             2. TOOL FIRST: Prioritize NMAP, SQLMAP, and BROWSER on the target. Your goal is compromise and discovery, not research.\n\
             3. PARALLELISM: Use BACKGROUND: to run reconnaissance (NMAP/SEARCH) in parallel with active analysis.\n\
             4. CONCISION: Your reasoning should be tactical and brief. Execute immediately.\n\n\
             STRATEGIC INTELLIGENCE (Loot & Topology):\n{}\n\n\
             ACTIVE MISSION PLAN:\n{}\n\n\
             TOOLSET COMMANDS:\n\
             - SEARCH: <query> (Target-specific discovery only)\n\
             - NMAP: <host> (Fast protocol discovery)\n\
             - SQLMAP: <url> (Automated injection testing)\n\
             - BROWSER: <url> (Native web interaction & rendering)\n\
             - TERMINAL: <cmd> (Custom local tool execution)\n\
             - FINISH (Objective achieved)\n\n\
             FORMAT: Always include a tactical REASONING line followed by one or more COMMANDs. \
             Example: Reasoning: Found port 80 open. BACKGROUND: NMAP: 192.168.1.1\nBROWSER: http://192.168.1.1",
            os, arch, user, now, target, insights.join("\n"), plan
        )
    }
}
