use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tokio::task::JoinHandle;
use crate::web_search::NativeWebSearch;

#[derive(Clone)]
pub struct WorkerInfo {
    pub id: String,
    pub command: String,
    pub status: String,
    pub logs: Vec<String>,
    pub loot: Vec<String>,
}

pub struct WorkerPoolState {
    pub workers: HashMap<String, WorkerInfo>,
    _handles: HashMap<String, JoinHandle<()>>,
    _next_id: usize,
}

use crate::graph::ShadowGraph;

#[derive(Clone)]
pub struct WorkerPool {
    pub state: Arc<RwLock<WorkerPoolState>>,
    graph: Arc<RwLock<ShadowGraph>>,
    search: Arc<NativeWebSearch>,
    browser: Option<Arc<crate::browser::NativeBrowserEngine>>,
    pub event_tx: mpsc::Sender<String>,
}

use crate::nmap::NativeNmap;
use crate::sqlmap::NativeSqlmap;

impl WorkerPool {
    pub fn new(
        event_tx: mpsc::Sender<String>, 
        graph: Arc<RwLock<ShadowGraph>>, 
        search: Arc<NativeWebSearch>,
        browser: Option<Arc<crate::browser::NativeBrowserEngine>>
    ) -> Self {
        WorkerPool {
            state: Arc::new(RwLock::new(WorkerPoolState {
                workers: HashMap::new(),
                _handles: HashMap::new(),
                _next_id: 0,
            })),
            graph,
            search,
            browser,
            event_tx,
        }
    }

    pub async fn spawn(&self, payload: String) -> String {
        let (worker_id, state_clone) = {
            let mut state = self.state.write().await;
            let wid = format!("agent-{}", state._next_id);
            state._next_id += 1;
            
            let info = WorkerInfo {
                id: wid.clone(),
                command: payload.clone(),
                status: "Booting".to_string(),
                logs: vec![format!("Instance {} initialized.", wid)],
                loot: vec![],
            };
            state.workers.insert(wid.clone(), info);
            (wid, self.state.clone())
        };

        let tx = self.event_tx.clone();
        let wid = worker_id.clone();
        let graph = self.graph.clone();
        let search = self.search.clone();
        let browser = self.browser.clone();
        let payload_inner = payload.clone();

        let handle = tokio::spawn(async move {
            let log = |msg: String| {
                let state = state_clone.clone();
                let wid = wid.clone();
                tokio::spawn(async move {
                    let mut s = state.write().await;
                    if let Some(w) = s.workers.get_mut(&wid) {
                        w.logs.push(msg);
                    }
                });
            };

            let _ = tx.send(format!("⚙️ Agent {} initializing: {}", wid, payload_inner)).await;
            log(format!("Executing objective: {}", payload_inner));
            
            {
                let mut s = state_clone.write().await;
                if let Some(w) = s.workers.get_mut(&wid) {
                    w.status = "Initializing".to_string();
                }
            }
            
            if payload_inner.starts_with("NMAP:") {
                let target = payload_inner.split("NMAP:").last().unwrap_or("").trim();
                {
                    let mut s = state_clone.write().await;
                    if let Some(w) = s.workers.get_mut(&wid) { w.status = "Scanning".to_string(); }
                }
                if let Ok(res) = NativeNmap::scan(target).await {
                    let ports = NativeNmap::parse_discovered_ports(&res);
                    for (port, service) in &ports {
                         let _ = tx.send(format!("🎯 [Loot] Agent {} found port {} ({}) on {}", wid, port, service, target)).await;
                         let mut s = state_clone.write().await;
                         if let Some(w) = s.workers.get_mut(&wid) { w.loot.push(format!("Port Open: {} ({})", port, service)); }
                    }
                    {
                        let mut g = graph.write().await;
                        g.ingest_nmap(target, ports);
                    }
                    log("Network discovery finalized. Topology synchronized.".to_string());
                }
            } else if payload_inner.starts_with("SQLMAP:") {
                let url = payload_inner.split("SQLMAP:").last().unwrap_or("").trim();
                {
                    let mut s = state_clone.write().await;
                    if let Some(w) = s.workers.get_mut(&wid) { w.status = "Injecting".to_string(); }
                }
                if let Ok(res) = NativeSqlmap::scan(url).await {
                    let vulns = NativeSqlmap::parse_vulnerabilities(&res);
                    for v in &vulns {
                         let _ = tx.send(format!("🎯 [Loot] Agent {} confirmed vulnerability: {}", wid, v)).await;
                         let mut s = state_clone.write().await;
                         if let Some(w) = s.workers.get_mut(&wid) { w.loot.push(format!("Vulnerability: {}", v)); }
                    }
                    {
                        let mut g = graph.write().await;
                        g.ingest_sqlmap(url, vulns);
                    }
                    log("Injection sequence concluded.".to_string());
                }
            } else if payload_inner.starts_with("SEARCH:") {
                let query = payload_inner.split("SEARCH:").last().unwrap_or("").trim();
                {
                    let mut s = state_clone.write().await;
                    if let Some(w) = s.workers.get_mut(&wid) { w.status = "Searching".to_string(); }
                }
                match search.search(query).await {
                    Ok(res) => {
                        let _ = tx.send(format!("🔍 [Intel] Agent {} retrieved knowledge for: {}", wid, query)).await;
                        log(format!("Intelligence gathered ({} bytes). Syncing to ShadowGraph.", res.len()));
                    },
                    Err(e) => {
                        log(format!("Search error: {}", e));
                    }
                }
            } else if payload_inner.starts_with("TERMINAL:") {
                let cmd = payload_inner.split("TERMINAL:").last().unwrap_or("").trim();
                {
                    let mut s = state_clone.write().await;
                    if let Some(w) = s.workers.get_mut(&wid) { w.status = "Executing".to_string(); }
                }
                match crate::terminal::NativeTerminal::execute(cmd, 60).await {
                    Ok(out) => {
                        log(out.clone());
                        let mut g = graph.write().await;
                        g.extract_from_note("terminal", &out);
                        log("Terminal command output synchronized to ShadowGraph.".to_string());
                    },
                    Err(e) => {
                        log(format!("Terminal error: {}", e));
                    }
                }
            } else if payload_inner.starts_with("BROWSER:") {
                let url = payload_inner.split("BROWSER:").last().unwrap_or("").trim();
                {
                    let mut s = state_clone.write().await;
                    if let Some(w) = s.workers.get_mut(&wid) { w.status = "Browsing".to_string(); }
                }
                
                if let Some(engine) = browser {
                    log(format!("Navigating to {} via Chromiumoxide CDP...", url));
                    match engine.action("navigate", url).await {
                        Ok(res) => {
                            log(res);
                            // Follow up with content extraction
                            if let Ok(content) = engine.action("get_content", "").await {
                                log(format!("Content Strategy: Extraction complete ({} bytes).", content.len()));
                                let mut g = graph.write().await;
                                g.extract_from_note("browser", &content);
                            }
                        },
                        Err(e) => log(format!("Browser error: {}", e)),
                    }
                } else {
                    log("Error: Native Browser Engine not available in this instance.".to_string());
                }
            }

            {
                let mut s = state_clone.write().await;
                if let Some(w) = s.workers.get_mut(&wid) {
                    w.status = "Finished".to_string();
                }
            }
            let _ = tx.send(format!("✅ Agent {} concluded its mission.", wid)).await;
            log("Agent resources deallocated. Mission complete.".to_string());
        });

        {
            let mut state = self.state.write().await;
            state._handles.insert(worker_id.clone(), handle);
        }
        worker_id
    }
}
