use crossterm::{
    event::{Event, KeyCode, EventStream, EnableMouseCapture, DisableMouseCapture, MouseEventKind, MouseButton},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use futures::StreamExt;
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style, Modifier},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, List, ListItem, Clear, Wrap},
    Terminal,
};
use std::io;
use std::sync::Arc;
use tokio::sync::mpsc::Receiver;
use serde_json::json;

use tokio::sync::RwLock;
use crate::graph::ShadowGraph;
use crate::pool::WorkerPool;
use chrono::Local;

pub async fn run_tui(
    mut event_rx: Receiver<String>,
    cmd_tx: tokio::sync::mpsc::Sender<String>,
    graph: Arc<RwLock<ShadowGraph>>,
    llm_engine: Arc<crate::llm::NativeLLMEngine>,
    target_shared: Arc<RwLock<String>>,
    worker_pool: WorkerPool,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut logs: Vec<String> = Vec::new();
    let mut reader = EventStream::new();
    let mut input = String::new();
    
    // UI State
    let mut is_nav_mode = false;
    let mut telemetry_scroll: u16 = 0;
    let mut agent_cursor: usize = 0;
    let mut selected_agent_id: Option<String> = None;
    let mut agent_detail_scroll: u16 = 0;

    // Existing Popup state
    let mut show_model_popup = false;
    let mut popup_query = String::new();
    let mut popup_cursor: usize = 0;

    // AI Suggestions
    let mut suggestion_ghost = String::new();
    let (suggest_tx, mut suggest_rx) = tokio::sync::mpsc::channel::<String>(100);
    let (suggest_resp_tx, mut suggest_resp_rx) = tokio::sync::mpsc::channel::<String>(100);
    
    let engine_clone = llm_engine.clone();
    tokio::spawn(async move {
        let mut last_req = String::new();
        loop {
            let mut req = match suggest_rx.recv().await {
                Some(r) => r,
                None => break,
            };
            while let Ok(r) = suggest_rx.try_recv() { req = r; }
            if req == last_req { continue; }
            last_req = req.clone();
            
            tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;
            if !suggest_rx.is_empty() { continue; }

            if !req.is_empty() {
                let context = "User typing a pentest orchestrator prompt.";
                if let Ok(suggestion) = engine_clone.ai_suggest_completion(&req, context).await {
                    let _ = suggest_resp_tx.send(suggestion).await;
                }
            } else {
                let _ = suggest_resp_tx.send("".to_string()).await;
            }
        }
    });

    // Report Mode
    let mut active_report: Option<String> = None;
    let mut is_generating_report = false;
    let mut report_scroll: u16 = 0;
    let (report_tx, mut report_rx) = tokio::sync::mpsc::channel::<String>(2);

    let _commands = vec![
        "/agent", "/crew", "/se", "/evm", "/chain", "/target", "/tools",
        "/notes", "/nodes", "/report", "/memory", "/topology", "/prompt", "/config",
        "/clear", "/help", "/quit", "/model", "/models", "/modes"
    ];

    loop {
        let g = graph.clone();
        let pool = worker_pool.state.clone();
        
        terminal.draw(|f| {
            let size = f.size();
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(3),  // Header
                    Constraint::Length(8),  // Condensed Topology (Top)
                    Constraint::Min(5),      // Main Body (Logs + Agents)
                    Constraint::Length(3),  // Prompt
                ].as_ref())
                .split(size);

             // Integrated Header
            let (model_name, is_thinking) = {
                let s = llm_engine.state.try_read();
                if let Ok(s) = s {
                    (s.model.clone(), s.is_thinking)
                } else {
                    ("Loading...".to_string(), false)
                }
            };
            let target = target_shared.try_read().map(|t| t.clone()).unwrap_or("None".to_string());
            
            let header_chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Percentage(40),
                    Constraint::Percentage(30),
                    Constraint::Percentage(30),
                ])
                .split(chunks[0]);

            let mission_block = Paragraph::new(format!(" OBJECTIVE: {}", target))
                .style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
                .block(Block::default().borders(Borders::ALL).title(" Mission Control "))
                .wrap(Wrap { trim: true });
            
            let model_block = Paragraph::new(format!(" Model: {}", model_name))
                .style(Style::default().fg(Color::Magenta))
                .block(Block::default().borders(Borders::ALL).title(" Provider "))
                .wrap(Wrap { trim: true });

            let (state_text, state_style) = if is_thinking { 
                ("Thinking...".to_string(), Style::default().fg(Color::Yellow)) 
            } else { 
                ("Idle".to_string(), Style::default().fg(Color::Green)) 
            };

            let llm_metrics = {
                let s = llm_engine.state.try_read();
                if let Ok(s) = s {
                    format!(" Latency: {}ms\n Tokens: P:{} | C:{}", s.last_latency_ms, s.prompt_tokens, s.completion_tokens)
                } else {
                    " Loading metrics...".to_string()
                }
            };

            let state_block = Paragraph::new(format!(" Status: {}\n{}", state_text, llm_metrics))
                .style(state_style)
                .block(Block::default().borders(Borders::ALL).title(" LLM Telemetry "));

            f.render_widget(mission_block, header_chunks[0]);
            f.render_widget(model_block, header_chunks[1]);
            f.render_widget(state_block, header_chunks[2]);

            // Render Topology
            let topology_text = if let Ok(g_read) = g.try_read() {
                g_read.to_ascii_topology(chunks[1].width)
            } else {
                "Loading topology...".to_string()
            };

            let topology = Paragraph::new(topology_text)
                .style(Style::default().fg(Color::Green))
                .block(Block::default().borders(Borders::ALL).title(" Network Topology Map "));
            f.render_widget(topology, chunks[1]);

            // Main Body: Logs (Left) + Agents (Right)
            let body_chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Percentage(75),
                    Constraint::Percentage(25),
                ])
                .split(chunks[2]);

            let telemetry_title = if is_nav_mode { " [NAV MODE] Execution Telemetry (Arrows to Scroll) " } else { " Execution Telemetry (Esc for Nav) " };
            let log_para = Paragraph::new(logs.join("\n"))
                .block(Block::default().borders(Borders::ALL).title(telemetry_title))
                .style(Style::default().fg(Color::White))
                .wrap(Wrap { trim: true })
                .scroll((telemetry_scroll, 0));
            f.render_widget(log_para, body_chunks[0]);

            // Agent Panel
            let mut agent_list_items = Vec::new();
            let mut sorted_workers = Vec::new();
            if let Ok(p) = pool.try_read() {
                let mut workers: Vec<_> = p.workers.values().collect();
                workers.sort_by(|a, b| a.id.cmp(&b.id));
                for (i, w) in workers.iter().enumerate() {
                    let style = if i == agent_cursor && is_nav_mode {
                        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::White)
                    };
                    let emoji = match w.status.as_str() {
                        "Scanning" | "Injecting" => "🔄",
                        "Finished" => "✅",
                        "Searching" | "Browsing" | "Browsing..." => "🔍",
                        "Executing" => "⚡",
                        "Error" => "⚠️",
                        _ => "💤",
                    };
                    
                    let status_color = match w.status.as_str() {
                        "Scanning" | "Injecting" => Color::Yellow,
                        "Finished" => Color::Green,
                        "Error" => Color::Red,
                        _ => Color::Gray,
                    };

                    let pulse = if w.status == "Scanning" || w.status == "Injecting" || w.status == "Searching" || w.status == "Executing" || w.status == "Browsing" {
                        let tick = Local::now().timestamp_subsec_millis() / 500;
                        if tick % 2 == 0 { "*" } else { " " }
                    } else { "" };

                    agent_list_items.push(ListItem::new(format!(" {} {} {} {}", emoji, w.id, w.status, pulse)).style(style.fg(status_color)));
                    sorted_workers.push(w.id.clone());
                }
            }

            let agent_list = List::new(agent_list_items)
                .block(Block::default().borders(Borders::ALL).title(" Active Agents "));
            f.render_widget(agent_list, body_chunks[1]);

            // Interaction Bar (Footer)
            let prompt_border_style = if is_nav_mode { Style::default().fg(Color::DarkGray) } else { Style::default().fg(Color::Yellow) };
            
            let mut prompt_spans = vec![
                Span::styled(format!("> {}", input), Style::default().fg(Color::Yellow)),
            ];
            if !suggestion_ghost.is_empty() && !is_nav_mode {
                prompt_spans.push(Span::styled(suggestion_ghost.clone(), Style::default().fg(Color::DarkGray)));
            }

            let input_para = Paragraph::new(Line::from(prompt_spans))
                .block(Block::default().borders(Borders::ALL).title(" Agent Prompt (Tab to complete, /commands) ").border_style(prompt_border_style))
                .wrap(Wrap { trim: true });
            f.render_widget(input_para, chunks[3]);

            // Modals
            if let Some(agent_id) = &selected_agent_id {
                let area = centered_rect(80, 80, size);
                f.render_widget(Clear, area);
                if let Ok(p) = pool.try_read() {
                    if let Some(w) = p.workers.get(agent_id) {
                        let mut detail_lines = vec![
                            format!("AGENT ID: {}", w.id),
                            format!("COMMAND : {}", w.command),
                            format!("STATUS  : {}", w.status),
                            "-----------------------------------".to_string(),
                            "DISCOVERED INTELLIGENCE (LOOT):".to_string(),
                        ];
                        if w.loot.is_empty() {
                            detail_lines.push("  (No unique loot discovered yet)".to_string());
                        } else {
                            for l in &w.loot { detail_lines.push(format!("  [+] {}", l)); }
                        }
                        detail_lines.push("-----------------------------------".to_string());
                        detail_lines.push("TERMINAL STREAMING OUTPUT:".to_string());
                        detail_lines.extend(w.logs.iter().cloned());

                        let detail_para = Paragraph::new(detail_lines.join("\n"))
                            .block(Block::default().borders(Borders::ALL).title(format!(" Intelligence Report: {} (Arrows to Scroll, ESC to close) ", w.id)))
                            .wrap(Wrap { trim: true })
                            .scroll((agent_detail_scroll, 0));
                        f.render_widget(detail_para, area);
                    }
                }
            }

            if show_model_popup {
                let area = centered_rect(60, 50, size);
                f.render_widget(Clear, area);
                if let Ok(s) = llm_engine.state.try_read() {
                    let filter = popup_query.to_lowercase();
                    let free_only = filter.contains(":free");
                    let clean_query = filter.replace(":free", "").trim().to_string();

                    let filtered_models: Vec<_> = s.available_models.iter()
                        .filter(|m| {
                            let matches_query = m.id.to_lowercase().contains(&clean_query) || m.name.to_lowercase().contains(&clean_query);
                            let is_free = if let Some(p) = &m.pricing { 
                                p.prompt.parse::<f64>().unwrap_or(1.0) == 0.0 && p.completion.parse::<f64>().unwrap_or(1.0) == 0.0
                            } else { false };
                            matches_query && (!free_only || is_free)
                        })
                        .collect();

                    let items: Vec<ListItem> = filtered_models.iter().enumerate().map(|(i, m)| {
                        let style = if i == popup_cursor { Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD) } else { Style::default().fg(Color::White) };
                        ListItem::new(format!("{} ({})", m.name, m.id)).style(style)
                    }).collect();

                    let list = List::new(items)
                        .block(Block::default().borders(Borders::ALL).title(format!(" Select Model (Filter: '{}', ESC to cancel) ", popup_query)))
                        .highlight_symbol(">> ");
                    f.render_widget(list, area);
                }
            }

            if is_generating_report {
                let area = centered_rect(50, 20, size);
                f.render_widget(Clear, area);
                let loading = Paragraph::new("Synthesizing Intelligence...\nCalling Executive Summary Agent...")
                    .style(Style::default().fg(Color::Yellow))
                    .block(Block::default().borders(Borders::ALL).title(" Generating Report "));
                f.render_widget(loading, area);
            } else if let Some(report) = &active_report {
                let area = centered_rect(90, 90, size);
                f.render_widget(Clear, area);
                let report_para = Paragraph::new(report.as_str())
                    .style(Style::default().fg(Color::White))
                    .block(Block::default().borders(Borders::ALL).title(" Executive Intelligence Report (Arrows to scroll, ESC to close) "))
                    .wrap(Wrap { trim: true })
                    .scroll((report_scroll, 0));
                f.render_widget(report_para, area);
            }
        })?;

        tokio::select! {
            Some(msg) = event_rx.recv() => {
                logs.push(msg);
                if logs.len() > 2000 { logs.remove(0); }
                if !is_nav_mode {
                    telemetry_scroll = logs.len().saturating_sub(10) as u16; // Auto-scroll
                }
            }
            Some(sug) = suggest_resp_rx.recv() => {
                suggestion_ghost = sug;
            }
            Some(rep) = report_rx.recv() => {
                is_generating_report = false;
                active_report = Some(rep);
                report_scroll = 0;
            }
            Some(Ok(evt)) = reader.next() => {
                match evt {
                    Event::Key(key) => {
                        match key.code {
                            KeyCode::Esc => {
                                if active_report.is_some() { active_report = None; }
                                else if selected_agent_id.is_some() { selected_agent_id = None; }
                                else if show_model_popup { show_model_popup = false; }
                                else { is_nav_mode = !is_nav_mode; }
                            }
                            KeyCode::Up => {
                                if active_report.is_some() {
                                    report_scroll = report_scroll.saturating_sub(1);
                                } else if selected_agent_id.is_some() {
                                    agent_detail_scroll = agent_detail_scroll.saturating_sub(1);
                                } else if show_model_popup {
                                    if popup_cursor > 0 { popup_cursor -= 1; }
                                } else if is_nav_mode {
                                    if agent_cursor > 0 { agent_cursor -= 1; }
                                    else { telemetry_scroll = telemetry_scroll.saturating_sub(1); }
                                }
                            }
                            KeyCode::Down => {
                                if active_report.is_some() {
                                    report_scroll += 1;
                                } else if selected_agent_id.is_some() {
                                    agent_detail_scroll += 1;
                                } else if show_model_popup {
                                    // ... check count ...
                                    if let Ok(s) = llm_engine.state.try_read() {
                                        let filter = popup_query.to_lowercase();
                                        let free_only = filter.contains(":free");
                                        let clean_query = filter.replace(":free", "").trim().to_string();
                                        let count = s.available_models.iter().filter(|m| {
                                            let matches = m.id.to_lowercase().contains(&clean_query) || m.name.to_lowercase().contains(&clean_query);
                                            let is_free = if let Some(p) = &m.pricing { p.prompt.parse::<f64>().unwrap_or(1.0) == 0.0 } else { false };
                                            matches && (!free_only || is_free)
                                        }).count();
                                        if popup_cursor < count.saturating_sub(1) { popup_cursor += 1; }
                                    }
                                } else if is_nav_mode {
                                    if let Ok(p) = pool.try_read() {
                                        if agent_cursor < p.workers.len().saturating_sub(1) { agent_cursor += 1; }
                                        else { telemetry_scroll += 1; }
                                    }
                                }
                            }
                            KeyCode::Char(c) => {
                                if show_model_popup { popup_query.push(c); popup_cursor = 0; }
                                else if !is_nav_mode { 
                                    input.push(c); 
                                    suggestion_ghost.clear();
                                    let _ = suggest_tx.try_send(input.clone());
                                }
                            },
                            KeyCode::Backspace => { 
                                if show_model_popup { popup_query.pop(); popup_cursor = 0; }
                                else if !is_nav_mode { 
                                    input.pop(); 
                                    suggestion_ghost.clear();
                                    let _ = suggest_tx.try_send(input.clone());
                                }
                            },
                            KeyCode::Tab => {
                                if !is_nav_mode && !suggestion_ghost.is_empty() {
                                    input.push_str(&suggestion_ghost);
                                    suggestion_ghost.clear();
                                    let _ = suggest_tx.try_send(input.clone());
                                }
                            },
                            KeyCode::Enter => {
                                if is_nav_mode {
                                    if let Ok(p) = pool.try_read() {
                                        let mut workers: Vec<_> = p.workers.values().collect();
                                        workers.sort_by(|a, b| a.id.cmp(&b.id));
                                        if let Some(w) = workers.get(agent_cursor) {
                                            selected_agent_id = Some(w.id.clone());
                                            agent_detail_scroll = 0;
                                        }
                                    }
                                } else if show_model_popup {
                                    if let Ok(s) = llm_engine.state.try_read() {
                                        let filter = popup_query.to_lowercase();
                                        let free_only = filter.contains(":free");
                                        let clean_query = filter.replace(":free", "").trim().to_string();
                                        let filtered: Vec<_> = s.available_models.iter().filter(|m| {
                                            let matches = m.id.to_lowercase().contains(&clean_query) || m.name.to_lowercase().contains(&clean_query);
                                            let is_free = if let Some(p) = &m.pricing { p.prompt.parse::<f64>().unwrap_or(1.0) == 0.0 } else { false };
                                            matches && (!free_only || is_free)
                                        }).collect();
                                        if let Some(m) = filtered.get(popup_cursor) {
                                            let id = m.id.clone();
                                            drop(s);
                                            let mut sw = llm_engine.state.write().await;
                                            sw.model = id.clone();
                                            
                                            // Persist the choice
                                            let mut config = crate::config::AppConfig::load();
                                            config.selected_model = id;
                                            let _ = config.save();
                                            
                                            show_model_popup = false;
                                        }
                                    }
                                } else if !input.is_empty() {
                                    let clean_input = input.trim().to_lowercase();
                                    if clean_input == "/quit" || clean_input == "/exit" {
                                        break;
                                    } else if clean_input == "/model" || clean_input == "/models" {
                                        show_model_popup = true; popup_query.clear(); popup_cursor = 0;
                                    } else if clean_input == "/report" {
                                        is_generating_report = true;
                                        suggestion_ghost.clear();
                                        
                                        let llm = llm_engine.clone();
                                        let graph_ref = graph.clone();
                                        let rep_tx = report_tx.clone();
                                        let target = target_shared.try_read().map(|t| t.clone()).unwrap_or("Unknown".into());
                                        
                                        tokio::spawn(async move {
                                            let insights = graph_ref.read().await.get_strategic_insights().join("\n");
                                            let prompt = format!(
                                                "You are an expert offensive security reporting engine. \
                                                 Generate a comprehensive Markdown penetration test report for the target: {}.\n\n\
                                                 INTELLIGENCE TOPOLOGY GRAPH:\n{}\n\n\
                                                 Make sure to include an Executive Summary, Discovered Scope/Attack Surface, High-Level Vulnerabilities (if any), and Recommendations.", 
                                                target, insights
                                            );
                                            let msgs = vec![json!({"role": "system", "content": prompt})];
                                            if let Ok(res) = llm.generate_with_history(msgs).await {
                                                let _ = rep_tx.send(res).await;
                                            } else {
                                                let _ = rep_tx.send("Failed to generate report.".into()).await;
                                            }
                                        });
                                    } else {
                                        let _ = cmd_tx.try_send(input.clone());
                                    }
                                    input.clear();
                                    suggestion_ghost.clear();
                                }
                            }
                            KeyCode::Char('q') if input.trim().is_empty() && !show_model_popup && !active_report.is_some() => break,
                            _ => {}
                        }
                    }
                    Event::Mouse(mouse) => {
                        if let MouseEventKind::Down(MouseButton::Left) = mouse.kind {
                           // Simple click-to-nav detection could be added here if needed
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableMouseCapture)?;
    terminal.show_cursor()?;
    Ok(())
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}
