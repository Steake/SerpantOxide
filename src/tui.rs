use crossterm::{
    event::{
        DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
        Event, EventStream, KeyCode, MouseButton, MouseEventKind,
    },
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use futures::StreamExt;
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap},
};
use std::io;
use std::sync::Arc;

use crate::events::UiEvent;
use crate::graph::{ShadowGraph, TopologyRelationship};
use crate::pool::WorkerPool;
use crate::runtime::{RuntimeCommand, parse_operator_input};
use chrono::Local;
use tokio::sync::RwLock;

#[derive(Clone, Debug, Default)]
struct ToolModalState {
    agent_id: String,
    tool_id: usize,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum TopologyFocus {
    #[default]
    Hosts,
    Detail,
    Relations,
    Findings,
}

#[derive(Clone, Debug, Default)]
struct UiHitState {
    telemetry_area: Rect,
    topology_area: Rect,
    agent_list_area: Rect,
    agent_detail_area: Option<Rect>,
    agent_log_area: Option<Rect>,
    agent_tools_area: Option<Rect>,
    topology_modal_area: Option<Rect>,
    topology_hosts_area: Option<Rect>,
    topology_detail_area: Option<Rect>,
    topology_relation_area: Option<Rect>,
    topology_findings_area: Option<Rect>,
    report_area: Option<Rect>,
    tool_modal_area: Option<Rect>,
    model_popup_area: Option<Rect>,
    agent_row_ids: Vec<String>,
    agent_tool_ids: Vec<usize>,
}

pub async fn run_tui(
    mut event_rx: tokio::sync::broadcast::Receiver<UiEvent>,
    cmd_tx: tokio::sync::mpsc::Sender<RuntimeCommand>,
    graph: Arc<RwLock<ShadowGraph>>,
    llm_engine: Arc<crate::llm::NativeLLMEngine>,
    target_shared: Arc<RwLock<String>>,
    worker_pool: WorkerPool,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(
        stdout,
        EnterAlternateScreen,
        EnableMouseCapture,
        EnableBracketedPaste
    )?;
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
    let mut completed_checklist: Vec<String> = Vec::new();
    let mut remaining_checklist: Vec<String> = Vec::new();

    // Existing Popup state
    let mut show_model_popup = false;
    let mut popup_query = String::new();
    let mut popup_cursor: usize = 0;

    // AI Suggestions
    let mut suggestion_ghost = String::new();
    let (suggest_tx, mut suggest_rx) = tokio::sync::mpsc::channel::<String>(100);
    let (suggest_resp_tx, mut suggest_resp_rx) =
        tokio::sync::mpsc::channel::<(String, String)>(100);
    let mut _last_crew_summary: Option<String> = None;

    let engine_clone = llm_engine.clone();
    tokio::spawn(async move {
        let mut last_req = String::new();
        loop {
            let mut req = match suggest_rx.recv().await {
                Some(r) => r,
                None => break,
            };
            while let Ok(r) = suggest_rx.try_recv() {
                req = r;
            }
            if req == last_req {
                continue;
            }
            last_req = req.clone();

            tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;
            if !suggest_rx.is_empty() {
                continue;
            }

            if !req.is_empty() {
                let context = "User typing a pentest orchestrator prompt.";
                if let Ok(suggestion) = engine_clone.ai_suggest_completion(&req, context).await {
                    let _ = suggest_resp_tx.send((req.clone(), suggestion)).await;
                } else {
                    let _ = suggest_resp_tx.send((req.clone(), "".to_string())).await;
                }
            } else {
                let _ = suggest_resp_tx.send((req.clone(), "".to_string())).await;
            }
        }
    });

    // Report Mode
    let mut active_report: Option<String> = None;
    let mut is_generating_report = false;
    let mut report_scroll: u16 = 0;
    let (_report_tx, mut report_rx) = tokio::sync::mpsc::channel::<String>(2);
    let mut selected_tool: Option<ToolModalState> = None;
    let mut ui_hits = UiHitState::default();
    let mut show_topology_modal = false;
    let mut topology_fullscreen = false;
    let mut topology_host_cursor: usize = 0;
    let mut topology_detail_scroll: u16 = 0;
    let mut topology_relationship_scroll: u16 = 0;
    let mut topology_findings_scroll: u16 = 0;
    let mut topology_focus = TopologyFocus::Hosts;
    let mut topology_peer_cursor: usize = 0;

    let commands = vec![
        "/agent",
        "/crew",
        "/se",
        "/evm",
        "/chain",
        "/target",
        "/tools",
        "/notes",
        "/nodes",
        "/cancel",
        "/retry",
        "/report",
        "/memory",
        "/topology",
        "/prompt",
        "/config",
        "/clear",
        "/help",
        "/quit",
        "/model",
        "/models",
        "/modes",
    ];

    loop {
        let g = graph.clone();
        let pool = worker_pool.state.clone();

        terminal.draw(|f| {
            let size = f.size();
            let prompt_height = prompt_height(&input, &suggestion_ghost, size.width);
            f.render_widget(Block::default().style(Style::default().bg(ui_bg())), size);
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints(
                    [
                        Constraint::Length(3), // Header
                        Constraint::Length(14), // Topology intelligence
                        Constraint::Min(5),    // Main Body (Logs + Agents)
                        Constraint::Length(prompt_height), // Prompt
                    ]
                    .as_ref(),
                )
                .split(size);

            // Integrated Header
            let (model_name, llm_status, is_thinking, last_latency_ms, prompt_tokens, completion_tokens) = {
                let s = llm_engine.state.try_read();
                if let Ok(s) = s {
                    (
                        s.model.clone(),
                        s.status.clone(),
                        s.is_thinking,
                        s.last_latency_ms,
                        s.prompt_tokens,
                        s.completion_tokens,
                    )
                } else {
                    ("Loading...".to_string(), "Loading...".to_string(), false, 0, 0, 0)
                }
            };
            let target = target_shared
                .try_read()
                .map(|t| t.clone())
                .unwrap_or("None".to_string());

            let header_chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Percentage(40),
                    Constraint::Percentage(30),
                    Constraint::Percentage(30),
                ])
                .split(chunks[0]);

            let mission_block = Paragraph::new(Line::from(vec![
                Span::styled("TARGET ".to_string(), Style::default().fg(ui_muted())),
                Span::styled(
                    target.clone(),
                    Style::default()
                        .fg(ui_accent())
                        .add_modifier(Modifier::BOLD),
                ),
            ]))
                .style(
                    Style::default()
                        .fg(ui_text())
                        .bg(ui_panel()),
                )
                .block(emphasis_block("Mission Control"))
                .wrap(Wrap { trim: true });

            let model_block = Paragraph::new(Line::from(vec![
                Span::styled("MODEL ".to_string(), Style::default().fg(ui_muted())),
                Span::styled(
                    model_name,
                    Style::default()
                        .fg(ui_info())
                        .add_modifier(Modifier::BOLD),
                ),
            ]))
                .style(Style::default().fg(ui_text()).bg(ui_panel()))
                .block(panel_block("Provider"))
                .wrap(Wrap { trim: true });

            let lower_status = llm_status.to_lowercase();
            let (state_text, state_style) = if is_thinking {
                (llm_status, Style::default().fg(ui_warning()))
            } else if lower_status.contains("rate limited")
                || lower_status.contains("provider error")
                || lower_status.contains("failed")
                || lower_status.contains("unavailable")
                || lower_status.contains("denied")
                || lower_status.contains("rejected")
            {
                (llm_status, Style::default().fg(ui_danger()))
            } else {
                (llm_status, Style::default().fg(ui_success()))
            };

            let llm_metrics = format!(
                " Latency: {}ms\n Tokens: P:{} | C:{}",
                last_latency_ms, prompt_tokens, completion_tokens
            );

            let state_block = Paragraph::new(format!(" Status: {}\n{}", state_text, llm_metrics))
                .style(state_style.bg(ui_panel()))
                .block(panel_block("LLM Telemetry"));

            f.render_widget(mission_block, header_chunks[0]);
            f.render_widget(model_block, header_chunks[1]);
            f.render_widget(state_block, header_chunks[2]);

            // Render Topology
            let topology_text = if let Ok(g_read) = g.try_read() {
                g_read.to_ascii_topology(chunks[1].width, chunks[1].height)
            } else {
                "Loading topology...".to_string()
            };

            let topology = Paragraph::new(render_code_lines(&topology_text))
                .style(Style::default().fg(ui_text()).bg(ui_panel()))
                .block(emphasis_block("Topology Intelligence  Click or /topology"));
            f.render_widget(topology, chunks[1]);
            ui_hits.topology_area = chunks[1];

            // Main Body: Logs (Left) + Crew Panel (Right)
            let body_chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(75), Constraint::Percentage(25)])
                .split(chunks[2]);

            let telemetry_title = if is_nav_mode {
                " [NAV MODE] Execution Telemetry (Arrows to Scroll) "
            } else {
                " Execution Telemetry (Esc for Nav, Mouse Wheel to Scroll) "
            };
            let log_lines = render_log_lines(&logs);
            let log_para = Paragraph::new(log_lines.clone())
                .block(panel_block(telemetry_title))
                .style(Style::default().fg(ui_text()).bg(ui_panel()))
                .wrap(Wrap { trim: true })
                .scroll((bottom_anchor_scroll(&log_lines, body_chunks[0], telemetry_scroll), 0));
            f.render_widget(log_para, body_chunks[0]);
            ui_hits.telemetry_area = body_chunks[0];

            let crew_chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(10), Constraint::Min(8)])
                .split(body_chunks[1]);

            let mut checklist_lines = Vec::new();
            if completed_checklist.is_empty() && remaining_checklist.is_empty() {
                checklist_lines.push("No published plan yet.".to_string());
            } else {
                if !remaining_checklist.is_empty() {
                    checklist_lines.push("In Progress:".to_string());
                    for item in &remaining_checklist {
                        checklist_lines.push(format!("[ ] {}", item));
                    }
                }
                if !completed_checklist.is_empty() {
                    if !checklist_lines.is_empty() {
                        checklist_lines.push(String::new());
                    }
                    checklist_lines.push("Completed:".to_string());
                    for item in &completed_checklist {
                        checklist_lines.push(format!("[x] {}", item));
                    }
                }
            }

            let checklist = Paragraph::new(render_markdown_lines(&checklist_lines.join("\n")))
                .block(panel_block("Mission Checklist"))
                .style(Style::default().fg(ui_text()).bg(ui_panel()))
                .wrap(Wrap { trim: true });
            f.render_widget(checklist, crew_chunks[0]);

            // Agent Panel
            let mut agent_list_items = Vec::new();
            ui_hits.agent_row_ids.clear();
            if let Ok(p) = pool.try_read() {
                let mut workers: Vec<_> = p.workers.values().collect();
                workers.sort_by(|a, b| a.id.cmp(&b.id));
                for (i, w) in workers.iter().enumerate() {
                    ui_hits.agent_row_ids.push(w.id.clone());
                    let style = if i == agent_cursor && is_nav_mode {
                        Style::default()
                            .fg(ui_accent())
                            .bg(ui_selected_bg())
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(ui_text()).bg(ui_panel())
                    };
                    let glyph = match w.status.as_str() {
                        "Scanning" | "Injecting" => "~",
                        "Finished" => "+",
                        "Searching" | "Browsing" | "Browsing..." => "?",
                        "Executing" => "*",
                        "Error" => "!",
                        _ => "-",
                    };

                    let status_color = match w.status.as_str() {
                        "Scanning" | "Injecting" => ui_warning(),
                        "Finished" => ui_success(),
                        "Error" => ui_danger(),
                        "Searching" | "Browsing" | "Browsing..." | "Executing" => ui_info(),
                        _ => ui_muted(),
                    };

                    let pulse = if w.status == "Scanning"
                        || w.status == "Injecting"
                        || w.status == "Searching"
                        || w.status == "Executing"
                        || w.status == "Browsing"
                    {
                        let tick = Local::now().timestamp_subsec_millis() / 500;
                        if tick % 2 == 0 { "*" } else { " " }
                    } else {
                        ""
                    };

                    agent_list_items.push(
                        ListItem::new(format!(" {} {} {} {}", glyph, w.id, w.status, pulse))
                            .style(style.fg(status_color)),
                    );
                }
            }

            let agent_list =
                List::new(agent_list_items).block(panel_block("Active Agents  Click or Enter to Inspect"));
            f.render_widget(agent_list, crew_chunks[1]);
            ui_hits.agent_list_area = crew_chunks[1];

            // Interaction Bar (Footer)
            let prompt_border_style = if is_nav_mode {
                Style::default().fg(ui_muted())
            } else {
                Style::default().fg(ui_accent())
            };

            let input_para = Paragraph::new(build_prompt_lines(&input, &suggestion_ghost, is_nav_mode))
                .block(
                    panel_block("Agent Prompt  Enter to run, Tab to complete, paste multiline ok")
                        .border_style(prompt_border_style),
                )
                .style(Style::default().bg(ui_panel()))
                .wrap(Wrap { trim: true });
            f.render_widget(input_para, chunks[3]);

            // Modals
            if let Some(agent_id) = &selected_agent_id {
                let area = centered_rect(84, 84, size);
                f.render_widget(Clear, area);
                ui_hits.agent_detail_area = Some(area);
                ui_hits.agent_tools_area = None;
                ui_hits.agent_log_area = None;
                if let Ok(p) = pool.try_read() {
                    if let Some(w) = p.workers.get(agent_id) {
                        let frame = emphasis_block(&format!(
                            "Intelligence Report: {}  Click tools, wheel to scroll, ESC to close",
                            w.id
                        ));
                        let inner = frame.inner(area);
                        f.render_widget(frame, area);

                        let detail_chunks = Layout::default()
                            .direction(Direction::Vertical)
                            .constraints([
                                Constraint::Length(7),
                                Constraint::Length(8),
                                Constraint::Min(8),
                            ])
                            .split(inner);

                        let latest_loot = if w.loot.is_empty() {
                            "(No unique loot discovered yet)".to_string()
                        } else {
                            w.loot
                                .iter()
                                .rev()
                                .take(3)
                                .cloned()
                                .collect::<Vec<_>>()
                                .into_iter()
                                .rev()
                                .map(|item| format!("+ {}", item))
                                .collect::<Vec<_>>()
                                .join("\n")
                        };

                        let info_para = Paragraph::new(render_markdown_lines(&format!(
                            "**ID**: {}\n**STATUS**: {}\n**COMMAND**: `{}`\n**TOOLS**: {}\n**LOOT**: {}\n{}\n",
                            w.id,
                            w.status,
                            w.command,
                            w.tool_history.len(),
                            w.loot.len(),
                            latest_loot
                        )))
                        .block(panel_block("Agent Overview"))
                        .style(Style::default().bg(ui_panel()))
                        .wrap(Wrap { trim: false });
                        f.render_widget(info_para, detail_chunks[0]);

                        ui_hits.agent_tool_ids = w.tool_history.iter().map(|tool| tool.id).collect();
                        let tool_items = if w.tool_history.is_empty() {
                            vec![ListItem::new(" No tool executions yet").style(
                                Style::default().fg(ui_muted()),
                            )]
                        } else {
                            w.tool_history
                                .iter()
                                .map(|tool| {
                                    let state = if tool.result.is_some() { "done" } else { "live" };
                                    let color = if tool.result.is_some() {
                                        ui_success()
                                    } else {
                                        ui_warning()
                                    };
                                    ListItem::new(format!(
                                        " [{}] {} ({})",
                                        tool.id, tool.name, state
                                    ))
                                    .style(Style::default().fg(color))
                                })
                                .collect::<Vec<_>>()
                        };
                        let tool_list = List::new(tool_items).block(panel_block("Tool Timeline"));
                        f.render_widget(tool_list, detail_chunks[1]);
                        ui_hits.agent_tools_area = Some(detail_chunks[1]);

                        let worker_log_lines = render_log_lines(&w.logs);
                        let logs_para = Paragraph::new(worker_log_lines.clone())
                            .block(panel_block("Live Output"))
                            .style(Style::default().bg(ui_panel()))
                            .wrap(Wrap { trim: true })
                            .scroll((
                                bottom_anchor_scroll(
                                    &worker_log_lines,
                                    detail_chunks[2],
                                    agent_detail_scroll,
                                ),
                                0,
                            ));
                        f.render_widget(logs_para, detail_chunks[2]);
                        ui_hits.agent_log_area = Some(detail_chunks[2]);
                    }
                }
            } else {
                ui_hits.agent_detail_area = None;
                ui_hits.agent_tools_area = None;
                ui_hits.agent_log_area = None;
                ui_hits.agent_tool_ids.clear();
            }

            if show_model_popup {
                let area = centered_rect(60, 50, size);
                f.render_widget(Clear, area);
                ui_hits.model_popup_area = Some(area);
                if let Ok(s) = llm_engine.state.try_read() {
                    let filter = popup_query.to_lowercase();
                    let free_only = filter.contains(":free");
                    let clean_query = filter.replace(":free", "").trim().to_string();

                    let filtered_models: Vec<_> = s
                        .available_models
                        .iter()
                        .filter(|m| {
                            let matches_query = m.id.to_lowercase().contains(&clean_query)
                                || m.name.to_lowercase().contains(&clean_query);
                            let is_free = if let Some(p) = &m.pricing {
                                p.prompt.parse::<f64>().unwrap_or(1.0) == 0.0
                                    && p.completion.parse::<f64>().unwrap_or(1.0) == 0.0
                            } else {
                                false
                            };
                            matches_query && (!free_only || is_free)
                        })
                        .collect();

                    let items: Vec<ListItem> = filtered_models
                        .iter()
                        .enumerate()
                        .map(|(i, m)| {
                            let style = if i == popup_cursor {
                                Style::default()
                                    .fg(ui_accent())
                                    .bg(ui_selected_bg())
                                    .add_modifier(Modifier::BOLD)
                            } else {
                                Style::default().fg(ui_text()).bg(ui_panel())
                            };
                            ListItem::new(format!(
                                "{} ({}) {}",
                                m.name,
                                m.id,
                                describe_model_pricing(m)
                            ))
                            .style(style)
                        })
                        .collect();

                    let list = List::new(items)
                        .block(panel_block(&format!(
                            "Select Model  Filter: '{}'  ESC to cancel",
                            popup_query
                        )))
                        .highlight_symbol(">> ");
                    f.render_widget(list, area);
                }
            } else {
                ui_hits.model_popup_area = None;
            }

            if is_generating_report {
                let area = centered_rect(50, 20, size);
                f.render_widget(Clear, area);
                let loading = Paragraph::new(
                    "Synthesizing Intelligence...\nCalling Executive Summary Agent...",
                    )
                    .style(Style::default().fg(ui_warning()).bg(ui_panel()))
                    .block(emphasis_block("Generating Report"));
                f.render_widget(loading, area);
                ui_hits.report_area = Some(area);
            } else if let Some(report) = &active_report {
                let area = centered_rect(90, 90, size);
                f.render_widget(Clear, area);
                let report_para =
                    Paragraph::new(render_markdown_lines(report.as_str()))
                        .style(Style::default().fg(ui_text()).bg(ui_panel()))
                        .block(emphasis_block(
                            "Executive Intelligence Report  Arrows to scroll, ESC to close",
                        ))
                        .wrap(Wrap { trim: true })
                        .scroll((report_scroll, 0));
                f.render_widget(report_para, area);
                ui_hits.report_area = Some(area);
            } else {
                ui_hits.report_area = None;
            }

            if let Some(tool_modal) = &selected_tool {
                let area = centered_rect(78, 70, size);
                f.render_widget(Clear, area);
                ui_hits.tool_modal_area = Some(area);
                if let Ok(p) = pool.try_read() {
                    if let Some(worker) = p.workers.get(&tool_modal.agent_id) {
                        if let Some(tool) = worker
                            .tool_history
                            .iter()
                            .find(|tool| tool.id == tool_modal.tool_id)
                        {
                            let tool_chunks = Layout::default()
                                .direction(Direction::Vertical)
                                .constraints([
                                    Constraint::Length(6),
                                    Constraint::Min(10),
                                ])
                                .split(area);

                            let args_para = Paragraph::new(render_code_lines(tool.args.as_str()))
                                .style(Style::default().bg(ui_panel()))
                                .block(panel_block(&format!(
                                    "Tool Detail: {} [{}]",
                                    tool.name, worker.id
                                )))
                                .wrap(Wrap { trim: false });
                            f.render_widget(args_para, tool_chunks[0]);

                            let result_para = Paragraph::new(
                                render_markdown_lines(
                                    tool.result
                                        .as_deref()
                                        .unwrap_or("Tool execution still streaming...")
                                )
                            )
                            .style(Style::default().bg(ui_panel()))
                            .block(panel_block("Output / Result  ESC to close"))
                            .wrap(Wrap { trim: false });
                            f.render_widget(result_para, tool_chunks[1]);
                        }
                    }
                }
            } else {
                ui_hits.tool_modal_area = None;
            }

            if show_topology_modal {
                let area = if topology_fullscreen {
                    size
                } else {
                    centered_rect(92, 88, size)
                };
                f.render_widget(Clear, area);
                ui_hits.topology_modal_area = Some(area);

                if let Ok(g_read) = g.try_read() {
                    let snapshot = g_read.snapshot();
                    let frame =
                        emphasis_block("Topology Explorer  Click hosts, wheel to scroll, ESC to close");
                    let inner = frame.inner(area);
                    f.render_widget(frame, area);

                    let outer_chunks = Layout::default()
                        .direction(Direction::Vertical)
                        .constraints([Constraint::Length(4), Constraint::Min(10)])
                        .split(inner);

                    let summary_text = format!(
                        "Hosts: {}  Services: {}  Web: {}  Vulns: {}  Cred links: {}  Mode: {}\nTarget focus: {}",
                        snapshot.host_count,
                        snapshot.service_count,
                        snapshot.web_count,
                        snapshot.vulnerability_count,
                        snapshot.credential_count,
                        if topology_fullscreen {
                            "fullscreen"
                        } else {
                            "modal"
                        },
                        snapshot
                            .hosts
                            .get(topology_host_cursor)
                            .map(|host| host.label.clone())
                            .or_else(|| snapshot.web_findings.first().map(|finding| finding.label.clone()))
                            .unwrap_or_else(|| "No mapped assets yet".to_string())
                    );
                    let summary = Paragraph::new(render_markdown_lines(&summary_text))
                        .block(panel_block("Discovery Summary"))
                        .style(Style::default().bg(ui_panel()))
                        .wrap(Wrap { trim: true });
                    f.render_widget(summary, outer_chunks[0]);

                    let body_chunks = Layout::default()
                        .direction(Direction::Horizontal)
                        .constraints([
                            Constraint::Length(32),
                            Constraint::Min(30),
                            Constraint::Length(30),
                        ])
                        .split(outer_chunks[1]);

                    let host_items = if snapshot.hosts.is_empty() {
                        vec![ListItem::new(" No hosts mapped yet").style(
                            Style::default().fg(ui_muted()),
                        )]
                    } else {
                        snapshot
                            .hosts
                            .iter()
                            .enumerate()
                            .map(|(idx, host)| {
                                let style = if idx == topology_host_cursor {
                                    Style::default()
                                        .fg(ui_accent())
                                        .bg(ui_selected_bg())
                                        .add_modifier(Modifier::BOLD)
                                } else {
                                    Style::default().fg(ui_text()).bg(ui_panel())
                                };
                                ListItem::new(format!(
                                    " {} [{} svc, {} cred]",
                                    host.label,
                                    host.services.len(),
                                    host.credentials
                                ))
                                .style(style)
                            })
                            .collect::<Vec<_>>()
                    };
                    let host_list = List::new(host_items).block(
                        panel_block("Hosts")
                            .border_style(focus_style(topology_focus == TopologyFocus::Hosts)),
                    );
                    f.render_widget(host_list, body_chunks[0]);
                    ui_hits.topology_hosts_area = Some(body_chunks[0]);

                    let selected_host = snapshot.hosts.get(topology_host_cursor);
                    let selected_label = selected_host.map(|host| host.label.clone());
                    let related_relationships = selected_label
                        .as_ref()
                        .map(|selected_label| {
                            snapshot
                                .relationships
                                .iter()
                                .filter(|rel| rel.source == *selected_label || rel.target == *selected_label)
                                .cloned()
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default();
                    let active_peer_index = if related_relationships.is_empty() {
                        0
                    } else {
                        topology_peer_cursor.min(related_relationships.len().saturating_sub(1))
                    };
                    let active_relationship = related_relationships.get(active_peer_index);
                    let detail_text = if let Some(host) = selected_host {
                        let mut lines = vec![
                            format!("Host: {}", host.label),
                            format!("Services exposed: {}", host.services.len()),
                            format!("Credential links: {}", host.credentials),
                            String::new(),
                            "Service inventory:".to_string(),
                        ];
                        if host.services.is_empty() {
                            lines.push("  (none yet)".to_string());
                        } else {
                            for svc in &host.services {
                                lines.push(format!("  - {}", svc));
                            }
                        }
                        lines.push(String::new());
                        lines.push("Peer link detail:".to_string());
                        if let Some(rel) = active_relationship {
                            let peer = if rel.source == host.label {
                                &rel.target
                            } else {
                                &rel.source
                            };
                            lines.push(format!("  -> {}", peer));
                            for reason in &rel.reasons {
                                lines.push(format!("     {}", reason));
                            }
                        } else {
                            lines.push("  (no derived peer link selected)".to_string());
                        }
                        lines.join("\n")
                    } else {
                        let mut lines = vec!["No host selected yet.".to_string()];
                        if !snapshot.web_findings.is_empty() {
                            lines.push(String::new());
                            lines.push("Known web assets:".to_string());
                            for finding in snapshot.web_findings.iter().take(5) {
                                lines.push(format!("  - {}", finding.label));
                            }
                        }
                        lines.join("\n")
                    };
                    let detail = Paragraph::new(render_markdown_lines(&detail_text))
                        .block(
                            panel_block("Selected Host Detail")
                                .border_style(focus_style(topology_focus == TopologyFocus::Detail)),
                        )
                        .style(Style::default().bg(ui_panel()))
                        .wrap(Wrap { trim: true })
                        .scroll((topology_detail_scroll, 0));
                    f.render_widget(detail, body_chunks[1]);
                    ui_hits.topology_detail_area = Some(body_chunks[1]);

                    let side_chunks = Layout::default()
                        .direction(Direction::Vertical)
                        .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
                        .split(body_chunks[2]);

                    let relationship_canvas = if let Some(selected_label) = selected_label.as_ref() {
                        render_topology_canvas(
                            selected_label,
                            &related_relationships,
                            active_peer_index,
                            side_chunks[0].width.saturating_sub(2),
                            side_chunks[0].height.saturating_sub(2),
                        )
                    } else {
                        "Select a host to inspect its peer relationships.".to_string()
                    };

                    let relationships = Paragraph::new(render_code_lines(&relationship_canvas))
                        .block(
                            panel_block("Host Graph Canvas")
                                .border_style(focus_style(topology_focus == TopologyFocus::Relations)),
                        )
                        .style(Style::default().bg(ui_panel()))
                        .wrap(Wrap { trim: true })
                        .scroll((topology_relationship_scroll, 0));
                    f.render_widget(relationships, side_chunks[0]);
                    ui_hits.topology_relation_area = Some(side_chunks[0]);

                    let mut right_lines = Vec::new();
                    right_lines.push("Web Findings".to_string());
                    if snapshot.web_findings.is_empty() {
                        right_lines.push("  (none yet)".to_string());
                    } else {
                        for finding in &snapshot.web_findings {
                            right_lines.push(format!("  - {}", finding.label));
                            if finding.vulnerabilities.is_empty() {
                                right_lines.push("      no confirmed vulns".to_string());
                            } else {
                                for vuln in &finding.vulnerabilities {
                                    right_lines.push(format!("      ! {}", vuln));
                                }
                            }
                        }
                    }
                    right_lines.push(String::new());
                    right_lines.push("Access Paths".to_string());
                    if snapshot.credential_links.is_empty() {
                        right_lines.push("  (none yet)".to_string());
                    } else {
                        for link in &snapshot.credential_links {
                            right_lines.push(format!("  - {}", link));
                        }
                    }

                    let side = Paragraph::new(render_markdown_lines(&right_lines.join("\n")))
                        .block(
                            panel_block("Findings / Access")
                                .border_style(focus_style(topology_focus == TopologyFocus::Findings)),
                        )
                        .style(Style::default().bg(ui_panel()))
                        .wrap(Wrap { trim: true });
                    let side = side.scroll((topology_findings_scroll, 0));
                    f.render_widget(side, side_chunks[1]);
                    ui_hits.topology_findings_area = Some(side_chunks[1]);
                }
            } else {
                ui_hits.topology_modal_area = None;
                ui_hits.topology_hosts_area = None;
                ui_hits.topology_detail_area = None;
                ui_hits.topology_relation_area = None;
                ui_hits.topology_findings_area = None;
            }
        })?;

        tokio::select! {
            recv = event_rx.recv() => {
                if let Ok(event) = recv {
                    match event {
                        UiEvent::Log { message } => logs.push(message),
                        UiEvent::Checklist { completed, remaining } => {
                            completed_checklist = completed;
                            remaining_checklist = remaining;
                            logs.push("Checklist updated.".to_string());
                        }
                        UiEvent::CrewComplete { summary } => {
                            logs.push("Crew complete.".to_string());
                            _last_crew_summary = Some(summary);
                            logs.push("Crew summary ready. Use /report to open it.".to_string());
                        }
                        UiEvent::ReportReady { report } => {
                            is_generating_report = false;
                            active_report = Some(report);
                            report_scroll = 0;
                        }
                        UiEvent::WorkerSpawn { worker_id, task } => {
                            logs.push(format!("Spawned {} -> {}", worker_id, task));
                        }
                        UiEvent::WorkerStatus { worker_id, status } => {
                            logs.push(format!("[{}] status -> {}", worker_id, status));
                        }
                        UiEvent::WorkerOutput { worker_id, message } => {
                            logs.push(format!("[{}] {}", worker_id, message));
                        }
                        UiEvent::WorkerTool {
                            worker_id,
                            tool_name,
                            result,
                            ..
                        } => {
                            if result.is_some() {
                                logs.push(format!("[{}] tool completed -> {}", worker_id, tool_name));
                            } else {
                                logs.push(format!("[{}] tool started -> {}", worker_id, tool_name));
                            }
                        }
                        UiEvent::TargetUpdated { target } => {
                            logs.push(format!("Target set to: {}", target));
                        }
                        UiEvent::ModelChanged { model_id } => {
                            logs.push(format!("Model set to: {}", model_id));
                        }
                        UiEvent::LogsCleared => {
                            logs.clear();
                            completed_checklist.clear();
                            remaining_checklist.clear();
                        }
                        UiEvent::ShutdownRequested => break,
                        UiEvent::ModelsUpdated { .. }
                        | UiEvent::TelemetryUpdated { .. }
                        | UiEvent::TopologyUpdated { .. }
                        | UiEvent::NotesUpdated { .. } => {}
                    }
                }
                if logs.len() > 2000 { logs.remove(0); }
                if !is_nav_mode {
                    telemetry_scroll = 0;
                }
            }
            Some((for_input, sug)) = suggest_resp_rx.recv() => {
                if for_input == input && is_useful_ghost_text(&input, &sug) {
                    suggestion_ghost = sug;
                } else if for_input == input {
                    suggestion_ghost.clear();
                }
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
                                if show_topology_modal { show_topology_modal = false; topology_fullscreen = false; }
                                else if selected_tool.is_some() { selected_tool = None; }
                                else if active_report.is_some() { active_report = None; }
                                else if selected_agent_id.is_some() { selected_agent_id = None; selected_tool = None; }
                                else if show_model_popup { show_model_popup = false; }
                                else { is_nav_mode = !is_nav_mode; }
                            }
                            KeyCode::BackTab | KeyCode::Left if show_topology_modal => {
                                topology_focus = previous_topology_focus(topology_focus);
                            }
                            KeyCode::Right if show_topology_modal => {
                                topology_focus = next_topology_focus(topology_focus);
                            }
                            KeyCode::Up => {
                                if show_topology_modal {
                                    match topology_focus {
                                        TopologyFocus::Hosts => {
                                            if topology_host_cursor > 0 {
                                                topology_host_cursor -= 1;
                                                topology_detail_scroll = 0;
                                                topology_relationship_scroll = 0;
                                                topology_peer_cursor = 0;
                                            }
                                        }
                                        TopologyFocus::Detail => {
                                            topology_detail_scroll = topology_detail_scroll.saturating_sub(1);
                                        }
                                        TopologyFocus::Relations => {
                                            if topology_peer_cursor > 0 {
                                                topology_peer_cursor -= 1;
                                            } else {
                                                topology_relationship_scroll = topology_relationship_scroll.saturating_sub(1);
                                            }
                                        }
                                        TopologyFocus::Findings => {
                                            topology_findings_scroll = topology_findings_scroll.saturating_sub(1);
                                        }
                                    }
                                } else if active_report.is_some() {
                                    report_scroll = report_scroll.saturating_sub(1);
                                } else if selected_agent_id.is_some() {
                                    agent_detail_scroll = agent_detail_scroll.saturating_add(1);
                                } else if show_model_popup {
                                    if popup_cursor > 0 { popup_cursor -= 1; }
                                } else if is_nav_mode {
                                    if agent_cursor > 0 { agent_cursor -= 1; }
                                    else { telemetry_scroll = telemetry_scroll.saturating_add(1); }
                                }
                            }
                            KeyCode::Down => {
                                if show_topology_modal {
                                    match topology_focus {
                                        TopologyFocus::Hosts => {
                                            if let Ok(g_read) = g.try_read() {
                                                let snapshot = g_read.snapshot();
                                                if topology_host_cursor < snapshot.hosts.len().saturating_sub(1) {
                                                    topology_host_cursor += 1;
                                                    topology_detail_scroll = 0;
                                                    topology_relationship_scroll = 0;
                                                    topology_peer_cursor = 0;
                                                }
                                            }
                                        }
                                        TopologyFocus::Detail => {
                                            topology_detail_scroll = topology_detail_scroll.saturating_add(1);
                                        }
                                        TopologyFocus::Relations => {
                                            if let Ok(g_read) = g.try_read() {
                                                let snapshot = g_read.snapshot();
                                                if let Some(host) = snapshot.hosts.get(topology_host_cursor) {
                                                    let related_count = snapshot
                                                        .relationships
                                                        .iter()
                                                        .filter(|rel| rel.source == host.label || rel.target == host.label)
                                                        .count();
                                                    if topology_peer_cursor < related_count.saturating_sub(1) {
                                                        topology_peer_cursor += 1;
                                                    } else {
                                                        topology_relationship_scroll = topology_relationship_scroll.saturating_add(1);
                                                    }
                                                }
                                            }
                                        }
                                        TopologyFocus::Findings => {
                                            topology_findings_scroll = topology_findings_scroll.saturating_add(1);
                                        }
                                    }
                                } else if active_report.is_some() {
                                    report_scroll += 1;
                                } else if selected_agent_id.is_some() {
                                    agent_detail_scroll = agent_detail_scroll.saturating_sub(1);
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
                                        else { telemetry_scroll = telemetry_scroll.saturating_sub(1); }
                                    }
                                }
                            }
                            KeyCode::Char('q') if input.trim().is_empty() && !show_topology_modal && !show_model_popup && active_report.is_none() && selected_agent_id.is_none() => break,
                            KeyCode::Char(c) => {
                                if show_topology_modal {
                                    if c == 'f' {
                                        topology_fullscreen = !topology_fullscreen;
                                    }
                                } else if show_model_popup { popup_query.push(c); popup_cursor = 0; }
                                else if !is_nav_mode {
                                    input.push(c);
                                    update_prompt_suggestion(
                                        &input,
                                        &commands,
                                        &mut suggestion_ghost,
                                        &suggest_tx,
                                    );
                                }
                            },
                            KeyCode::Backspace => {
                                if show_topology_modal {
                                } else if show_model_popup { popup_query.pop(); popup_cursor = 0; }
                                else if !is_nav_mode {
                                    input.pop();
                                    update_prompt_suggestion(
                                        &input,
                                        &commands,
                                        &mut suggestion_ghost,
                                        &suggest_tx,
                                    );
                                }
                            },
                            KeyCode::Tab => {
                                if show_topology_modal {
                                    topology_focus = next_topology_focus(topology_focus);
                                } else if !is_nav_mode && !suggestion_ghost.is_empty() {
                                    input.push_str(&suggestion_ghost);
                                    suggestion_ghost.clear();
                                    update_prompt_suggestion(
                                        &input,
                                        &commands,
                                        &mut suggestion_ghost,
                                        &suggest_tx,
                                    );
                                }
                            },
                            KeyCode::Enter => {
                                if show_topology_modal {
                                    topology_fullscreen = !topology_fullscreen;
                                } else if is_nav_mode {
                                    if let Ok(p) = pool.try_read() {
                                        let mut workers: Vec<_> = p.workers.values().collect();
                                        workers.sort_by(|a, b| a.id.cmp(&b.id));
                                        if let Some(w) = workers.get(agent_cursor) {
                                            selected_agent_id = Some(w.id.clone());
                                            selected_tool = None;
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
                                            let _ = cmd_tx
                                                .send(RuntimeCommand::SelectModel { model_id: id })
                                                .await;
                                            show_model_popup = false;
                                        }
                                    }
                                } else if !input.is_empty() {
                                    let clean_input = input.trim().to_lowercase();
                                    if clean_input == "/quit" || clean_input == "/exit" || clean_input == "/q" {
                                        let _ = cmd_tx.send(RuntimeCommand::Shutdown).await;
                                        break;
                                    } else if clean_input == "/model" || clean_input == "/models" {
                                        show_model_popup = true; popup_query.clear(); popup_cursor = 0;
                                    } else if clean_input == "/topology" {
                                        suggestion_ghost.clear();
                                        show_topology_modal = true;
                                        topology_fullscreen = true;
                                        topology_focus = TopologyFocus::Hosts;
                                        topology_detail_scroll = 0;
                                        topology_relationship_scroll = 0;
                                        topology_findings_scroll = 0;
                                        topology_peer_cursor = 0;
                                    } else if clean_input == "/report" {
                                        suggestion_ghost.clear();
                                        is_generating_report = true;
                                        let _ = cmd_tx.send(RuntimeCommand::GenerateReport).await;
                                    } else {
                                        match parse_operator_input(&input) {
                                            Ok(command) => {
                                                let _ = cmd_tx.send(command).await;
                                            }
                                            Err(error) => logs.push(error),
                                        }
                                    }
                                    input.clear();
                                    suggestion_ghost.clear();
                                }
                            }
                            _ => {}
                        }
                    }
                    Event::Paste(pasted) => {
                        if show_model_popup {
                            popup_query.push_str(&normalize_pasted_text(&pasted));
                            popup_cursor = 0;
                        } else if !is_nav_mode && !show_topology_modal {
                            let pasted = normalize_pasted_text(&pasted);
                            if !pasted.is_empty() {
                                input.push_str(&pasted);
                                update_prompt_suggestion(
                                    &input,
                                    &commands,
                                    &mut suggestion_ghost,
                                    &suggest_tx,
                                );
                            }
                        }
                    }
                    Event::Mouse(mouse) => {
                        match mouse.kind {
                            MouseEventKind::Down(MouseButton::Left) => {
                                let x = mouse.column;
                                let y = mouse.row;

                                if show_topology_modal {
                                    if let Some(area) = ui_hits.topology_modal_area {
                                        if !rect_contains(area, x, y) {
                                            show_topology_modal = false;
                                            topology_fullscreen = false;
                                            continue;
                                        }
                                    }

                                    if let Some(area) = ui_hits.topology_hosts_area {
                                        if let Some(index) =
                                            list_index_from_click(area, x, y, if let Ok(g_read) = g.try_read() {
                                                g_read.snapshot().hosts.len()
                                            } else {
                                                0
                                            })
                                        {
                                            topology_host_cursor = index;
                                            topology_detail_scroll = 0;
                                            topology_relationship_scroll = 0;
                                            topology_peer_cursor = 0;
                                            topology_focus = TopologyFocus::Hosts;
                                            continue;
                                        }
                                    }
                                    if let Some(area) = ui_hits.topology_detail_area {
                                        if rect_contains(area, x, y) {
                                            topology_focus = TopologyFocus::Detail;
                                            continue;
                                        }
                                    }
                                    if let Some(area) = ui_hits.topology_relation_area {
                                        if rect_contains(area, x, y) {
                                            topology_focus = TopologyFocus::Relations;
                                            continue;
                                        }
                                    }
                                    if let Some(area) = ui_hits.topology_findings_area {
                                        if rect_contains(area, x, y) {
                                            topology_focus = TopologyFocus::Findings;
                                            continue;
                                        }
                                    }
                                    continue;
                                }

                                if selected_tool.is_some() {
                                    if let Some(area) = ui_hits.tool_modal_area {
                                        if !rect_contains(area, x, y) {
                                            selected_tool = None;
                                        }
                                    }
                                    continue;
                                }

                                if show_model_popup {
                                    continue;
                                }

                                if active_report.is_some() {
                                    if let Some(area) = ui_hits.report_area {
                                        if !rect_contains(area, x, y) {
                                            active_report = None;
                                        }
                                    }
                                    continue;
                                }

                                if rect_contains(ui_hits.topology_area, x, y) {
                                    show_topology_modal = true;
                                    topology_fullscreen = false;
                                    topology_focus = TopologyFocus::Hosts;
                                    topology_detail_scroll = 0;
                                    topology_relationship_scroll = 0;
                                    topology_findings_scroll = 0;
                                    topology_peer_cursor = 0;
                                    continue;
                                }

                                if let Some(agent_id) = selected_agent_id.clone() {
                                    if let Some(area) = ui_hits.agent_detail_area {
                                        if !rect_contains(area, x, y) {
                                            selected_agent_id = None;
                                            selected_tool = None;
                                            continue;
                                        }
                                    }

                                    if let Some(area) = ui_hits.agent_tools_area {
                                        if let Some(index) = list_index_from_click(
                                            area,
                                            x,
                                            y,
                                            ui_hits.agent_tool_ids.len(),
                                        ) {
                                            if let Some(tool_id) = ui_hits.agent_tool_ids.get(index) {
                                                selected_tool = Some(ToolModalState {
                                                    agent_id,
                                                    tool_id: *tool_id,
                                                });
                                            }
                                            continue;
                                        }
                                    }
                                    continue;
                                }

                                if let Some(index) = list_index_from_click(
                                    ui_hits.agent_list_area,
                                    x,
                                    y,
                                    ui_hits.agent_row_ids.len(),
                                ) {
                                    if let Some(agent_id) = ui_hits.agent_row_ids.get(index) {
                                        agent_cursor = index;
                                        selected_agent_id = Some(agent_id.clone());
                                        selected_tool = None;
                                        agent_detail_scroll = 0;
                                    }
                                }
                            }
                            MouseEventKind::ScrollDown => {
                                let x = mouse.column;
                                let y = mouse.row;
                                if show_topology_modal {
                                    if let Some(area) = ui_hits.topology_detail_area {
                                        if rect_contains(area, x, y) {
                                            topology_focus = TopologyFocus::Detail;
                                            topology_detail_scroll = topology_detail_scroll.saturating_add(1);
                                        }
                                    }
                                    if let Some(area) = ui_hits.topology_relation_area {
                                        if rect_contains(area, x, y) {
                                            topology_focus = TopologyFocus::Relations;
                                            topology_relationship_scroll = topology_relationship_scroll.saturating_add(1);
                                        }
                                    }
                                    if let Some(area) = ui_hits.topology_findings_area {
                                        if rect_contains(area, x, y) {
                                            topology_focus = TopologyFocus::Findings;
                                            topology_findings_scroll = topology_findings_scroll.saturating_add(1);
                                        }
                                    }
                                } else if active_report.is_some() {
                                    if let Some(area) = ui_hits.report_area {
                                        if rect_contains(area, x, y) {
                                            report_scroll = report_scroll.saturating_add(1);
                                        }
                                    }
                                } else if selected_agent_id.is_some() {
                                    if let Some(area) = ui_hits.agent_log_area {
                                        if rect_contains(area, x, y) {
                                            agent_detail_scroll = agent_detail_scroll.saturating_sub(1);
                                        }
                                    }
                                } else if rect_contains(ui_hits.telemetry_area, x, y) {
                                    telemetry_scroll = telemetry_scroll.saturating_sub(1);
                                }
                            }
                            MouseEventKind::ScrollUp => {
                                let x = mouse.column;
                                let y = mouse.row;
                                if show_topology_modal {
                                    if let Some(area) = ui_hits.topology_detail_area {
                                        if rect_contains(area, x, y) {
                                            topology_focus = TopologyFocus::Detail;
                                            topology_detail_scroll = topology_detail_scroll.saturating_sub(1);
                                        }
                                    }
                                    if let Some(area) = ui_hits.topology_relation_area {
                                        if rect_contains(area, x, y) {
                                            topology_focus = TopologyFocus::Relations;
                                            topology_relationship_scroll = topology_relationship_scroll.saturating_sub(1);
                                        }
                                    }
                                    if let Some(area) = ui_hits.topology_findings_area {
                                        if rect_contains(area, x, y) {
                                            topology_focus = TopologyFocus::Findings;
                                            topology_findings_scroll = topology_findings_scroll.saturating_sub(1);
                                        }
                                    }
                                } else if active_report.is_some() {
                                    if let Some(area) = ui_hits.report_area {
                                        if rect_contains(area, x, y) {
                                            report_scroll = report_scroll.saturating_sub(1);
                                        }
                                    }
                                } else if selected_agent_id.is_some() {
                                    if let Some(area) = ui_hits.agent_log_area {
                                        if rect_contains(area, x, y) {
                                            agent_detail_scroll = agent_detail_scroll.saturating_add(1);
                                        }
                                    }
                                } else if rect_contains(ui_hits.telemetry_area, x, y) {
                                    telemetry_scroll = telemetry_scroll.saturating_add(1);
                                }
                            }
                            _ => {}
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture,
        DisableBracketedPaste
    )?;
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

fn rect_contains(rect: Rect, x: u16, y: u16) -> bool {
    x >= rect.x
        && x < rect.x.saturating_add(rect.width)
        && y >= rect.y
        && y < rect.y.saturating_add(rect.height)
}

fn bottom_anchor_scroll(lines: &[Line<'_>], area: Rect, bottom_offset: u16) -> u16 {
    let inner_width = area.width.saturating_sub(2).max(1);
    let inner_height = area.height.saturating_sub(2);
    let total_lines = estimated_wrapped_line_count(lines, inner_width as usize);
    let max_top_scroll = total_lines.saturating_sub(inner_height as usize) as u16;
    max_top_scroll.saturating_sub(bottom_offset)
}

fn estimated_wrapped_line_count(lines: &[Line<'_>], width: usize) -> usize {
    if width == 0 {
        return lines.len();
    }

    lines
        .iter()
        .map(|line| {
            let char_count = line
                .spans
                .iter()
                .map(|span| span.content.chars().count())
                .sum::<usize>();
            std::cmp::max(1, char_count.div_ceil(width))
        })
        .sum()
}

fn list_index_from_click(area: Rect, x: u16, y: u16, item_count: usize) -> Option<usize> {
    if !rect_contains(area, x, y) || item_count == 0 || area.height <= 2 {
        return None;
    }

    let inner_y = y.checked_sub(area.y + 1)?;
    let inner_height = area.height.saturating_sub(2) as usize;
    let index = inner_y as usize;
    if index < item_count && index < inner_height {
        Some(index)
    } else {
        None
    }
}

fn render_topology_canvas(
    selected_host: &str,
    relationships: &[TopologyRelationship],
    active_index: usize,
    width: u16,
    height: u16,
) -> String {
    let width = width.max(20) as usize;
    let height = height.max(8) as usize;
    let mut grid = vec![vec![' '; width]; height];

    let selected_text = format!("[{}]", truncate_canvas_label(selected_host, 16));
    let selected_x = width.saturating_sub(selected_text.len()) / 2;
    let selected_y = height / 2;
    let selected_center_x = selected_x + selected_text.len() / 2;

    let peer_labels = relationships
        .iter()
        .map(|rel| {
            if rel.source == selected_host {
                rel.target.clone()
            } else {
                rel.source.clone()
            }
        })
        .collect::<Vec<_>>();

    let slots = [
        (width / 2, 1usize),
        (width.saturating_sub(12), height / 4),
        (width.saturating_sub(12), (height * 3) / 4),
        (width / 2, height.saturating_sub(2)),
        (2usize, (height * 3) / 4),
        (2usize, height / 4),
        (width.saturating_sub(12), height / 2),
        (2usize, height / 2),
    ];

    for (idx, peer) in peer_labels.iter().take(slots.len()).enumerate() {
        let peer_text = if idx == active_index {
            format!(">{{{}}}<", truncate_canvas_label(peer, 10))
        } else {
            format!("({})", truncate_canvas_label(peer, 10))
        };
        let (slot_x, slot_y) = slots[idx];
        let peer_x = slot_x.min(width.saturating_sub(peer_text.len() + 1));
        let peer_y = slot_y.min(height.saturating_sub(1));
        let peer_center_x = peer_x + peer_text.len() / 2;

        draw_orthogonal_link(
            &mut grid,
            peer_center_x,
            peer_y,
            selected_center_x,
            selected_y,
        );
        draw_text(&mut grid, peer_x, peer_y, &peer_text);
    }

    draw_text(&mut grid, selected_x, selected_y, &selected_text);

    if relationships.is_empty() {
        draw_text(&mut grid, 2, 1, "No peer relationships derived yet");
    } else if relationships.len() > slots.len() {
        let more = format!("+{} more peers", relationships.len() - slots.len());
        let y = height.saturating_sub(1);
        draw_text(&mut grid, 2, y, &more);
    }

    grid.into_iter()
        .map(|row| {
            let line = row.into_iter().collect::<String>();
            line.trim_end().to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn truncate_canvas_label(label: &str, max_len: usize) -> String {
    if label.chars().count() <= max_len {
        return label.to_string();
    }
    let mut truncated = label
        .chars()
        .take(max_len.saturating_sub(1))
        .collect::<String>();
    truncated.push('~');
    truncated
}

fn draw_text(grid: &mut [Vec<char>], x: usize, y: usize, text: &str) {
    if y >= grid.len() {
        return;
    }
    for (offset, ch) in text.chars().enumerate() {
        let px = x + offset;
        if px < grid[y].len() {
            grid[y][px] = ch;
        }
    }
}

fn draw_orthogonal_link(
    grid: &mut [Vec<char>],
    from_x: usize,
    from_y: usize,
    to_x: usize,
    to_y: usize,
) {
    let bend_x = to_x;
    draw_horizontal(grid, from_x, bend_x, from_y);
    draw_vertical(grid, from_y, to_y, bend_x);
}

fn draw_horizontal(grid: &mut [Vec<char>], start_x: usize, end_x: usize, y: usize) {
    if y >= grid.len() {
        return;
    }
    let (lo, hi) = if start_x <= end_x {
        (start_x, end_x)
    } else {
        (end_x, start_x)
    };
    for x in lo..=hi {
        if x < grid[y].len() {
            let current = grid[y][x];
            grid[y][x] = match current {
                '|' | '+' => '+',
                _ => '-',
            };
        }
    }
}

fn draw_vertical(grid: &mut [Vec<char>], start_y: usize, end_y: usize, x: usize) {
    let (lo, hi) = if start_y <= end_y {
        (start_y, end_y)
    } else {
        (end_y, start_y)
    };
    for y in lo..=hi {
        if y < grid.len() && x < grid[y].len() {
            let current = grid[y][x];
            grid[y][x] = match current {
                '-' | '+' => '+',
                _ => '|',
            };
        }
    }
}

fn focus_style(active: bool) -> Style {
    if active {
        Style::default()
            .fg(ui_accent())
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(ui_border())
    }
}

fn next_topology_focus(focus: TopologyFocus) -> TopologyFocus {
    match focus {
        TopologyFocus::Hosts => TopologyFocus::Detail,
        TopologyFocus::Detail => TopologyFocus::Relations,
        TopologyFocus::Relations => TopologyFocus::Findings,
        TopologyFocus::Findings => TopologyFocus::Hosts,
    }
}

fn previous_topology_focus(focus: TopologyFocus) -> TopologyFocus {
    match focus {
        TopologyFocus::Hosts => TopologyFocus::Findings,
        TopologyFocus::Detail => TopologyFocus::Hosts,
        TopologyFocus::Relations => TopologyFocus::Detail,
        TopologyFocus::Findings => TopologyFocus::Relations,
    }
}

fn ui_bg() -> Color {
    Color::Black
}

fn ui_panel() -> Color {
    Color::Black
}

fn ui_selected_bg() -> Color {
    Color::DarkGray
}

fn ui_border() -> Color {
    Color::DarkGray
}

fn ui_text() -> Color {
    Color::White
}

fn ui_muted() -> Color {
    Color::Gray
}

fn ui_accent() -> Color {
    Color::Cyan
}

fn ui_info() -> Color {
    Color::Blue
}

fn ui_warning() -> Color {
    Color::Yellow
}

fn ui_success() -> Color {
    Color::Green
}

fn ui_danger() -> Color {
    Color::Red
}

fn ui_code() -> Color {
    Color::Magenta
}

fn panel_block(title: &str) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ui_border()))
        .style(Style::default().bg(ui_panel()))
        .title(Line::from(vec![Span::styled(
            format!(" {} ", title),
            Style::default()
                .fg(ui_info())
                .bg(ui_panel())
                .add_modifier(Modifier::BOLD),
        )]))
}

fn emphasis_block(title: &str) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ui_accent()))
        .style(Style::default().bg(ui_panel()))
        .title(Line::from(vec![Span::styled(
            format!(" {} ", title),
            Style::default()
                .fg(ui_accent())
                .bg(ui_panel())
                .add_modifier(Modifier::BOLD),
        )]))
}

fn render_log_lines(logs: &[String]) -> Vec<Line<'static>> {
    logs.iter()
        .map(|line| render_log_line(line))
        .collect::<Vec<_>>()
}

fn render_log_line(line: &str) -> Line<'static> {
    let trimmed = line.trim();
    let lower = trimmed.to_lowercase();

    let style = if lower.contains("error")
        || trimmed.contains('❌')
        || lower.contains("failed")
        || lower.contains("cancelled")
    {
        Style::default().fg(ui_danger())
    } else if lower.contains("finished")
        || lower.contains("complete")
        || trimmed.contains('✅')
        || lower.contains("ready for next command")
    {
        Style::default().fg(ui_success())
    } else if lower.contains("tool started")
        || lower.contains("thinking:")
        || lower.contains("queued")
        || lower.contains("running")
    {
        Style::default().fg(ui_warning())
    } else if lower.contains("tool completed")
        || lower.contains("spawned ")
        || lower.contains("status ->")
    {
        Style::default().fg(ui_info())
    } else {
        Style::default().fg(ui_text())
    };

    if let Some(end) = trimmed.find(']') {
        if trimmed.starts_with('[') {
            return Line::from(vec![
                Span::styled(
                    trimmed[..=end].to_string(),
                    Style::default()
                        .fg(ui_accent())
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" "),
                Span::styled(trimmed[end + 1..].trim_start().to_string(), style),
            ]);
        }
    }

    Line::from(vec![Span::styled(trimmed.to_string(), style)])
}

fn render_code_lines(text: &str) -> Vec<Line<'static>> {
    if text.trim().is_empty() {
        return vec![Line::from(vec![Span::styled(
            "(empty)".to_string(),
            Style::default().fg(ui_muted()),
        )])];
    }

    text.lines()
        .map(|line| {
            Line::from(vec![Span::styled(
                line.to_string(),
                Style::default().fg(ui_code()),
            )])
        })
        .collect()
}

fn render_markdown_lines(text: &str) -> Vec<Line<'static>> {
    if text.trim().is_empty() {
        return vec![Line::from(vec![Span::styled(
            "(empty)".to_string(),
            Style::default().fg(ui_muted()),
        )])];
    }

    let mut lines = Vec::new();
    let mut in_code_block = false;
    for raw_line in text.lines() {
        let line = raw_line.trim_end();
        let trimmed = line.trim_start();

        if let Some(label) = trimmed.strip_prefix("```") {
            if in_code_block {
                lines.push(Line::from(vec![Span::styled(
                    "```".to_string(),
                    Style::default().fg(ui_muted()),
                )]));
                push_blank_markdown_line(&mut lines);
                in_code_block = false;
            } else {
                in_code_block = true;
                let fence = if label.trim().is_empty() {
                    "```".to_string()
                } else {
                    format!("```{}", label.trim())
                };
                lines.push(Line::from(vec![Span::styled(
                    fence,
                    Style::default().fg(ui_muted()),
                )]));
            }
            continue;
        }

        if in_code_block {
            lines.push(Line::from(vec![Span::styled(
                line.to_string(),
                Style::default().fg(ui_code()),
            )]));
            continue;
        }

        if trimmed.is_empty() {
            push_blank_markdown_line(&mut lines);
            continue;
        }

        if is_markdown_rule(trimmed) {
            lines.push(Line::from(vec![Span::styled(
                "─".repeat(36),
                Style::default().fg(ui_border()),
            )]));
            continue;
        }

        let (quote_depth, content_after_quotes) = strip_blockquote_prefix(trimmed);
        let mut prefix_spans = blockquote_prefix_spans(quote_depth);
        let mut content = content_after_quotes;
        let mut base_style = if quote_depth > 0 {
            Style::default()
                .fg(ui_muted())
                .add_modifier(Modifier::ITALIC)
        } else {
            Style::default().fg(ui_text())
        };
        let mut add_blank_after = false;

        if let Some((level, rest)) = parse_heading(content) {
            content = rest;
            base_style = markdown_heading_style(level);
            add_blank_after = level <= 2;
        } else if let Some(rest) = parse_task_list(content, true) {
            content = rest;
            prefix_spans.push(Span::styled(
                "[x] ".to_string(),
                Style::default()
                    .fg(ui_success())
                    .add_modifier(Modifier::BOLD),
            ));
        } else if let Some(rest) = parse_task_list(content, false) {
            content = rest;
            prefix_spans.push(Span::styled(
                "[ ] ".to_string(),
                Style::default()
                    .fg(ui_warning())
                    .add_modifier(Modifier::BOLD),
            ));
        } else if let Some(rest) = content
            .strip_prefix("- ")
            .or_else(|| content.strip_prefix("* "))
        {
            content = rest;
            prefix_spans.push(Span::styled(
                "• ".to_string(),
                Style::default()
                    .fg(ui_accent())
                    .add_modifier(Modifier::BOLD),
            ));
        } else if let Some((number, rest)) = ordered_list_parts(content) {
            content = rest;
            prefix_spans.push(Span::styled(
                format!("{}. ", number),
                Style::default().fg(ui_info()).add_modifier(Modifier::BOLD),
            ));
        }

        let mut spans = prefix_spans;
        spans.extend(parse_inline_markdown(content, base_style));
        lines.push(Line::from(spans));

        if add_blank_after {
            push_blank_markdown_line(&mut lines);
        }
    }

    while matches!(lines.last(), Some(line) if line.spans.is_empty()) {
        lines.pop();
    }

    if lines.is_empty() {
        vec![Line::from(vec![Span::styled(
            "(empty)".to_string(),
            Style::default().fg(ui_muted()),
        )])]
    } else {
        lines
    }
}

fn markdown_heading_style(level: usize) -> Style {
    match level {
        1 => Style::default()
            .fg(ui_warning())
            .add_modifier(Modifier::BOLD),
        2 => Style::default().fg(ui_info()).add_modifier(Modifier::BOLD),
        3 => Style::default()
            .fg(ui_accent())
            .add_modifier(Modifier::BOLD),
        _ => Style::default().fg(ui_text()).add_modifier(Modifier::BOLD),
    }
}

fn parse_inline_markdown(text: &str, base: Style) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut cursor = 0;

    while cursor < text.len() {
        let remaining = &text[cursor..];

        if let Some(after_tick) = remaining.strip_prefix('`') {
            if let Some(end) = after_tick.find('`') {
                spans.push(Span::styled(
                    after_tick[..end].to_string(),
                    Style::default().fg(ui_code()).add_modifier(Modifier::BOLD),
                ));
                cursor += 1 + end + 1;
                continue;
            }
        }

        if let Some((marker, style)) = inline_marker_at_start(remaining, base) {
            if let Some(end) = remaining[marker.len()..].find(marker) {
                let inner = &remaining[marker.len()..marker.len() + end];
                spans.extend(parse_inline_markdown(inner, style));
                cursor += marker.len() + end + marker.len();
                continue;
            }
        }

        if let Some(after_open) = remaining.strip_prefix('[') {
            if let Some(close) = after_open.find("](") {
                let label = &after_open[..close];
                let url_and_more = &after_open[close + 2..];
                if let Some(end) = url_and_more.find(')') {
                    spans.push(Span::styled(
                        label.to_string(),
                        base.fg(ui_info()).add_modifier(Modifier::UNDERLINED),
                    ));
                    cursor += 1 + close + 2 + end + 1;
                    continue;
                }
            }
        }

        let ch = remaining.chars().next().unwrap_or_default();
        spans.push(Span::styled(ch.to_string(), base));
        cursor += ch.len_utf8();
    }

    if spans.is_empty() {
        spans.push(Span::styled(String::new(), base));
    }

    spans
}

fn inline_marker_at_start<'a>(text: &'a str, base: Style) -> Option<(&'a str, Style)> {
    if text.starts_with("**") {
        Some(("**", base.add_modifier(Modifier::BOLD)))
    } else if text.starts_with("__") {
        Some(("__", base.add_modifier(Modifier::BOLD)))
    } else if text.starts_with("~~") {
        Some(("~~", base.add_modifier(Modifier::CROSSED_OUT)))
    } else if text.starts_with('*') {
        Some(("*", base.add_modifier(Modifier::ITALIC)))
    } else if text.starts_with('_') {
        Some(("_", base.add_modifier(Modifier::ITALIC)))
    } else {
        None
    }
}

fn strip_blockquote_prefix(line: &str) -> (usize, &str) {
    let mut depth = 0usize;
    let mut rest = line.trim_start();
    while let Some(after) = rest.strip_prefix('>') {
        depth += 1;
        rest = after.trim_start();
    }
    (depth, rest)
}

fn blockquote_prefix_spans(depth: usize) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    for _ in 0..depth {
        spans.push(Span::styled(
            "▌ ".to_string(),
            Style::default().fg(ui_muted()),
        ));
    }
    spans
}

fn push_blank_markdown_line(lines: &mut Vec<Line<'static>>) {
    if !matches!(lines.last(), Some(line) if line.spans.is_empty()) {
        lines.push(Line::default());
    }
}

fn parse_heading(line: &str) -> Option<(usize, &str)> {
    let mut count = 0usize;
    for ch in line.chars() {
        if ch == '#' {
            count += 1;
        } else {
            break;
        }
    }

    if (1..=6).contains(&count) && line.chars().nth(count) == Some(' ') {
        Some((count, line[count + 1..].trim_start()))
    } else {
        None
    }
}

fn parse_task_list<'a>(line: &'a str, checked: bool) -> Option<&'a str> {
    let prefixes = if checked {
        ["- [x] ", "- [X] ", "* [x] ", "* [X] "]
    } else {
        ["- [ ] ", "* [ ] ", "- [] ", "* [] "]
    };

    prefixes
        .into_iter()
        .find_map(|prefix| line.strip_prefix(prefix))
}

fn ordered_list_parts(line: &str) -> Option<(String, &str)> {
    let mut chars = line.char_indices().peekable();
    while let Some((_, ch)) = chars.peek() {
        if !ch.is_ascii_digit() {
            break;
        }
        chars.next();
    }

    let (idx, ch) = chars.next()?;
    if ch != '.' {
        return None;
    }

    let rest = line[idx + 1..].strip_prefix(' ')?;
    Some((line[..idx].to_string(), rest))
}

fn is_markdown_rule(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.len() >= 3
        && trimmed
            .chars()
            .all(|ch| ch == '-' || ch == '*' || ch == '_')
}

fn local_command_completion(input: &str, commands: &[&str]) -> Option<String> {
    let trimmed = input.trim();
    if !trimmed.starts_with('/') || trimmed.split_whitespace().count() > 1 {
        return None;
    }

    let mut matches = commands
        .iter()
        .copied()
        .filter(|command| command.starts_with(trimmed))
        .collect::<Vec<_>>();
    matches.sort_unstable();

    if matches.len() != 1 {
        return None;
    }

    matches[0]
        .strip_prefix(trimmed)
        .filter(|suffix| !suffix.is_empty())
        .map(ToString::to_string)
}

fn update_prompt_suggestion(
    input: &str,
    commands: &[&str],
    suggestion_ghost: &mut String,
    suggest_tx: &tokio::sync::mpsc::Sender<String>,
) {
    *suggestion_ghost = local_command_completion(input, commands).unwrap_or_default();
    if suggestion_ghost.is_empty() && should_request_ai_completion(input) {
        let _ = suggest_tx.try_send(input.to_string());
    }
}

fn normalize_pasted_text(pasted: &str) -> String {
    let normalized = pasted.replace("\r\n", "\n").replace('\r', "\n");
    normalized.trim_end_matches('\n').to_string()
}

fn build_prompt_lines(
    input: &str,
    suggestion_ghost: &str,
    is_nav_mode: bool,
) -> Vec<Line<'static>> {
    let prompt_style = Style::default()
        .fg(ui_warning())
        .add_modifier(Modifier::BOLD);
    let ghost_style = Style::default()
        .fg(ui_muted())
        .add_modifier(Modifier::ITALIC);

    let mut lines = Vec::new();
    let mut input_lines = input.split('\n');
    let first_line = input_lines.next().unwrap_or_default();
    let first_text = if input.is_empty() {
        "> ".to_string()
    } else {
        format!("> {}", first_line)
    };
    let mut first_spans = vec![Span::styled(first_text, prompt_style)];

    if !suggestion_ghost.is_empty() && !is_nav_mode && !input.contains('\n') {
        first_spans.push(Span::styled(suggestion_ghost.to_string(), ghost_style));
    }
    lines.push(Line::from(first_spans));

    for line in input_lines {
        lines.push(Line::from(Span::styled(
            format!("  {}", line),
            prompt_style,
        )));
    }

    lines
}

fn prompt_height(input: &str, suggestion_ghost: &str, width: u16) -> u16 {
    let content_width = usize::from(width.saturating_sub(4).max(8));
    let input_prefix = if input.is_empty() { "> " } else { "> " };
    let input_lines = wrapped_line_count(&format!("{input_prefix}{input}"), content_width);
    let ghost_lines = if suggestion_ghost.is_empty() || input.contains('\n') {
        0
    } else {
        wrapped_line_count(suggestion_ghost, content_width)
    };
    (input_lines + ghost_lines + 2).clamp(3, 10)
}

fn wrapped_line_count(text: &str, width: usize) -> u16 {
    text.split('\n')
        .map(|line| {
            let line_len = line.chars().count().max(1);
            line_len.div_ceil(width) as u16
        })
        .sum::<u16>()
        .max(1)
}

fn should_request_ai_completion(input: &str) -> bool {
    let trimmed = input.trim();
    !trimmed.is_empty()
        && !trimmed.starts_with('/')
        && trimmed.chars().count() >= 12
        && trimmed.contains(' ')
        && !trimmed.contains('\n')
}

fn is_useful_ghost_text(current_input: &str, suggestion: &str) -> bool {
    let trimmed = suggestion.trim();
    if trimmed.is_empty() || current_input.trim().starts_with('/') {
        return false;
    }

    if suggestion.contains('\n')
        || suggestion.chars().count() > 24
        || suggestion.split_whitespace().count() > 4
        || suggestion.contains("Context:")
        || suggestion.contains("Provide ONLY")
        || suggestion.contains("The user's input")
        || suggestion.contains('{')
        || suggestion.contains('}')
        || suggestion.contains('[')
        || suggestion.contains(']')
        || suggestion.contains(':')
    {
        return false;
    }

    suggestion.chars().all(|ch| !ch.is_control())
}

fn describe_model_pricing(model: &crate::llm::OpenRouterModel) -> String {
    if let Some(pricing) = &model.pricing {
        let prompt_cost = pricing.prompt.parse::<f64>().unwrap_or(0.0);
        let completion_cost = pricing.completion.parse::<f64>().unwrap_or(0.0);
        if prompt_cost == 0.0 && completion_cost == 0.0 {
            "[free/rate-limited prone]".to_string()
        } else {
            "[paid]".to_string()
        }
    } else {
        "[pricing unknown]".to_string()
    }
}
