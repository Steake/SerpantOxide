#[cfg(target_os = "macos")]
mod macos {
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use gpui::{
        App, Application, Context, Entity, SharedString, Timer, Window, WindowOptions, div,
        prelude::*, px, rgb,
    };

    use crate::runtime::{RuntimeCommand, RuntimeService, RuntimeSnapshot};

    pub fn run(runtime: RuntimeService, tokio_handle: tokio::runtime::Handle) -> Result<(), String> {
        let snapshot_store = Arc::new(Mutex::new(tokio_handle.block_on(runtime.snapshot())));
        let runtime_for_updates = runtime.clone();
        let snapshot_updater = snapshot_store.clone();
        tokio_handle.spawn(async move {
            loop {
                let snapshot = runtime_for_updates.snapshot().await;
                if let Ok(mut slot) = snapshot_updater.lock() {
                    *slot = snapshot;
                }
                tokio::time::sleep(Duration::from_millis(350)).await;
            }
        });

        Application::new().run(move |cx: &mut App| {
            cx.activate(true);
            let runtime = runtime.clone();
            let handle = tokio_handle.clone();
            let snapshot_store = snapshot_store.clone();
            cx.open_window(
                WindowOptions::default(),
                move |window, cx| {
                    let view: Entity<MacFrontend> = cx.new(|_| MacFrontend {
                        active_section: AppSection::Mission,
                        runtime: runtime.clone(),
                        tokio_handle: handle.clone(),
                        snapshot: snapshot_store.clone(),
                    });
                    let entity_id = view.entity_id();
                    window
                        .spawn(cx, async move |cx| {
                            loop {
                                Timer::after(Duration::from_millis(350)).await;
                                if cx.update(|_, cx| cx.notify(entity_id)).is_err() {
                                    break;
                                }
                            }
                        })
                        .detach();
                    view
                },
            )
            .map_err(|error| error.to_string())
            .unwrap();
        });

        Ok(())
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum AppSection {
        Mission,
        Agents,
        Topology,
        Notes,
        Reports,
        Settings,
    }

    impl AppSection {
        fn title(self) -> &'static str {
            match self {
                Self::Mission => "Mission",
                Self::Agents => "Agents",
                Self::Topology => "Topology",
                Self::Notes => "Notes",
                Self::Reports => "Reports",
                Self::Settings => "Settings",
            }
        }
    }

    struct MacFrontend {
        active_section: AppSection,
        runtime: RuntimeService,
        tokio_handle: tokio::runtime::Handle,
        snapshot: Arc<Mutex<RuntimeSnapshot>>,
    }

    impl MacFrontend {
        fn select_section(&mut self, section: AppSection, cx: &mut Context<Self>) {
            self.active_section = section;
            cx.notify();
        }
    }

    impl Render for MacFrontend {
        fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let snapshot = self
                .snapshot
                .lock()
                .map(|guard| guard.clone())
                .unwrap_or_default();

            let checklist = if snapshot.completed_checklist.is_empty()
                && snapshot.remaining_checklist.is_empty()
            {
                "No published plan yet.".to_string()
            } else {
                let mut lines = Vec::new();
                if !snapshot.remaining_checklist.is_empty() {
                    lines.push("In Progress:".to_string());
                    lines.extend(
                        snapshot
                            .remaining_checklist
                            .iter()
                            .map(|item| format!("[ ] {}", item)),
                    );
                }
                if !snapshot.completed_checklist.is_empty() {
                    lines.push(String::new());
                    lines.push("Completed:".to_string());
                    lines.extend(
                        snapshot
                            .completed_checklist
                            .iter()
                            .map(|item| format!("[x] {}", item)),
                    );
                }
                lines.join("\n")
            };

            let workers = if snapshot.workers.is_empty() {
                "No workers yet.".to_string()
            } else {
                snapshot
                    .workers
                    .iter()
                    .map(|worker| {
                        format!(
                            "{} [{}]\n  task: {}\n  tools: {}\n  loot: {}",
                            worker.id,
                            worker.status,
                            worker.task,
                            worker.tool_history.len(),
                            worker.loot.len()
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n\n")
            };

            let topology = format!(
                "Hosts: {}\nServices: {}\nWeb: {}\nVulnerabilities: {}\nCredentials: {}\n\n{}\n\n{}",
                snapshot.topology.host_count,
                snapshot.topology.service_count,
                snapshot.topology.web_count,
                snapshot.topology.vulnerability_count,
                snapshot.topology.credential_count,
                if snapshot.topology.hosts.is_empty() {
                    "No hosts mapped yet.".to_string()
                } else {
                    snapshot
                        .topology
                        .hosts
                        .iter()
                        .map(|host| format!("{} -> {}", host.label, host.services.join(", ")))
                        .collect::<Vec<_>>()
                        .join("\n")
                },
                if snapshot.topology.relationships.is_empty() {
                    "No relationships yet.".to_string()
                } else {
                    snapshot
                        .topology
                        .relationships
                        .iter()
                        .map(|edge| format!("{} -> {} ({})", edge.source, edge.target, edge.reasons.join(", ")))
                        .collect::<Vec<_>>()
                        .join("\n")
                }
            );

            let notes = if snapshot.note_categories.is_empty() {
                "No notes yet.".to_string()
            } else {
                snapshot
                    .note_categories
                    .iter()
                    .map(|category| format!("{} ({})", category.name, category.count))
                    .collect::<Vec<_>>()
                    .join("\n")
            };

            let report = snapshot
                .latest_report
                .clone()
                .or_else(|| snapshot.last_crew_summary.clone())
                .unwrap_or_else(|| "No report or crew summary yet.".to_string());

            let settings = format!(
                "Target: {}\nModel: {}\nStatus: {}\nAvailable models: {}\nShutdown requested: {}",
                snapshot.target,
                snapshot.llm.model,
                snapshot.llm.status,
                snapshot.llm.available_models.len(),
                snapshot.shutdown_requested
            );

            let activity = if snapshot.activity_log.is_empty() {
                "No runtime activity yet.".to_string()
            } else {
                snapshot
                    .activity_log
                    .iter()
                    .rev()
                    .take(50)
                    .cloned()
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect::<Vec<_>>()
                    .join("\n")
            };

            let active_section = self.active_section;
            let section_content = match active_section {
                AppSection::Mission => mission_section(&snapshot, &checklist, &activity, &workers),
                AppSection::Agents => section_stack()
                    .child(panel("Agents", &workers))
                    .child(panel("Recent Activity", &activity)),
                AppSection::Topology => section_stack().child(panel("Topology", &topology)),
                AppSection::Notes => section_stack().child(panel("Notes", &notes)),
                AppSection::Reports => section_stack().child(panel("Reports", &report)),
                AppSection::Settings => section_stack().child(panel("Settings", &settings)),
            };

            div()
                .flex()
                .bg(rgb(0xf3efe6))
                .text_color(rgb(0x201a14))
                .text_base()
                .line_height(px(20.))
                .size_full()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .w(px(160.))
                        .flex_none()
                        .p_4()
                        .gap_2()
                        .bg(rgb(0xe0d0b6))
                        .child(sidebar_title("Serpantoxide"))
                        .child(sidebar_item("Mission", active_section == AppSection::Mission, cx, |this, cx| {
                            this.select_section(AppSection::Mission, cx);
                        }))
                        .child(sidebar_item("Agents", active_section == AppSection::Agents, cx, |this, cx| {
                            this.select_section(AppSection::Agents, cx);
                        }))
                        .child(sidebar_item("Topology", active_section == AppSection::Topology, cx, |this, cx| {
                            this.select_section(AppSection::Topology, cx);
                        }))
                        .child(sidebar_item("Notes", active_section == AppSection::Notes, cx, |this, cx| {
                            this.select_section(AppSection::Notes, cx);
                        }))
                        .child(sidebar_item("Reports", active_section == AppSection::Reports, cx, |this, cx| {
                            this.select_section(AppSection::Reports, cx);
                        }))
                        .child(sidebar_item("Settings", active_section == AppSection::Settings, cx, |this, cx| {
                            this.select_section(AppSection::Settings, cx);
                        })),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_w(px(0.))
                        .child(
                            div()
                                .flex()
                                .flex_wrap()
                                .items_start()
                                .gap_3()
                                .px_5()
                                .py_4()
                                .bg(rgb(0xfbf7ef))
                                .border_b_1()
                                .border_color(rgb(0xd7c6aa))
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .flex_1()
                                        .min_w(px(320.))
                                        .gap_1()
                                        .child(
                                            div()
                                                .text_xl()
                                                .font_weight(gpui::FontWeight::BOLD)
                                                .child(format!(
                                                    "macOS Mission Control · {}",
                                                    active_section.title()
                                                )),
                                        )
                                        .child(
                                            div()
                                                .text_sm()
                                                .text_color(rgb(0x5f5649))
                                                .line_clamp(2)
                                                .child(format!(
                                                    "Target: {}  |  Model: {}  |  Status: {}",
                                                    snapshot.target, snapshot.llm.model, snapshot.llm.status
                                                )),
                                        ),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .flex_wrap()
                                        .gap_2()
                                        .flex_none()
                                        .child(action_button("Run Agent", {
                                            let this = self.runtime.clone();
                                            let handle = self.tokio_handle.clone();
                                            move |_, _| {
                                                let runtime = this.clone();
                                                handle.spawn(async move {
                                                    let _ = runtime
                                                        .send_command(RuntimeCommand::RunAgent {
                                                            task: "Enumerate the active target and summarize the initial attack surface.".to_string(),
                                                        })
                                                        .await;
                                                });
                                            }
                                        }))
                                        .child(action_button("Run Crew", {
                                            let this = self.runtime.clone();
                                            let handle = self.tokio_handle.clone();
                                            move |_, _| {
                                                let runtime = this.clone();
                                                handle.spawn(async move {
                                                    let _ = runtime
                                                        .send_command(RuntimeCommand::RunCrew {
                                                            task: "Full autonomous assessment".to_string(),
                                                        })
                                                        .await;
                                                });
                                            }
                                        }))
                                        .child(action_button("Generate Report", {
                                            let this = self.runtime.clone();
                                            let handle = self.tokio_handle.clone();
                                            move |_, _| {
                                                let runtime = this.clone();
                                                handle.spawn(async move {
                                                    let _ = runtime
                                                        .send_command(RuntimeCommand::GenerateReport)
                                                        .await;
                                                });
                                            }
                                        })),
                                ),
                        )
                        .child(
                            div()
                                .flex_1()
                                .id("main-scroll")
                                .overflow_y_scroll()
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap_3()
                                        .p_5()
                                        .min_h_full()
                                        .child(section_content),
                                ),
                        ),
                )
        }
    }

    fn sidebar_title(title: &str) -> gpui::Div {
        div()
            .pb_3()
            .text_color(rgb(0x2b2419))
            .text_lg()
            .font_weight(gpui::FontWeight::BOLD)
            .child(title.to_string())
    }

    fn sidebar_item(
        label: &str,
        active: bool,
        cx: &mut Context<MacFrontend>,
        on_click: impl Fn(&mut MacFrontend, &mut Context<MacFrontend>) + 'static,
    ) -> impl IntoElement {
        div()
            .id(SharedString::from(label.to_string()))
            .px_3()
            .py_2()
            .rounded_sm()
            .cursor_pointer()
            .bg(if active { rgb(0x203a2f) } else { rgb(0xf7f1e6) })
            .text_color(if active { rgb(0xfaf8f2) } else { rgb(0x2b2419) })
            .text_sm()
            .font_weight(if active {
                gpui::FontWeight::BOLD
            } else {
                gpui::FontWeight::NORMAL
            })
            .hover(|this| this.bg(rgb(0xede3d0)))
            .child(label.to_string())
            .on_click(cx.listener(move |this, _, _, cx| on_click(this, cx)))
    }

    fn action_button(
        label: &str,
        on_click: impl Fn(&mut Window, &mut App) + 'static,
    ) -> impl IntoElement {
        div()
            .id(SharedString::from(format!("action-{}", label)))
            .px_3()
            .py_2()
            .rounded_sm()
            .cursor_pointer()
            .bg(rgb(0x203a2f))
            .text_color(rgb(0xfaf8f2))
            .text_sm()
            .font_weight(gpui::FontWeight::BOLD)
            .hover(|this| this.bg(rgb(0x2f5848)))
            .child(label.to_string())
            .on_click(move |_, window, cx| on_click(window, cx))
    }

    fn metric_card(title: &str, value: &str) -> gpui::Div {
        div()
            .flex()
            .flex_col()
            .flex_1()
            .gap_1()
            .min_w(px(180.))
            .p_3()
            .bg(rgb(0xfbf7ef))
            .border_1()
            .border_color(rgb(0xd8ccb9))
            .rounded_sm()
            .text_color(rgb(0x201a14))
            .child(
                div()
                    .text_sm()
                    .text_color(rgb(0x776a58))
                    .line_height(px(18.))
                    .truncate()
                    .child(title.to_string()),
            )
            .child(
                div()
                    .text_lg()
                    .line_height(px(24.))
                    .font_weight(gpui::FontWeight::BOLD)
                    .line_clamp(2)
                    .child(value.to_string()),
            )
    }

    fn panel(title: &str, body: &str) -> gpui::Div {
        div()
            .flex()
            .flex_col()
            .gap_3()
            .p_4()
            .bg(rgb(0xfbf7ef))
            .border_1()
            .border_color(rgb(0xd8ccb9))
            .rounded_sm()
            .text_color(rgb(0x201a14))
            .child(
                div()
                    .text_lg()
                    .line_height(px(24.))
                    .font_weight(gpui::FontWeight::BOLD)
                    .child(title.to_string()),
            )
            .child(
                div()
                    .text_sm()
                    .line_height(px(20.))
                    .text_color(rgb(0x2b2419))
                    .child(body.to_string()),
            )
    }

    fn mission_section(
        snapshot: &RuntimeSnapshot,
        checklist: &str,
        activity: &str,
        workers: &str,
    ) -> gpui::Div {
        section_stack()
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_3()
                    .child(metric_card("Target", &snapshot.target))
                    .child(metric_card("Model", &snapshot.llm.model))
                    .child(metric_card(
                        "Latency",
                        &format!("{} ms", snapshot.llm.last_latency_ms),
                    ))
                    .child(metric_card(
                        "Tokens",
                        &format!(
                            "P:{} / C:{}",
                            snapshot.llm.prompt_tokens, snapshot.llm.completion_tokens
                        ),
                    )),
            )
            .child(panel("Mission", checklist))
            .child(panel("Active Agents", workers))
            .child(panel("Activity", activity))
    }

    fn section_stack() -> gpui::Div {
        div().flex().flex_col().gap_3()
    }
}

#[cfg(target_os = "macos")]
pub use macos::run;

#[cfg(not(target_os = "macos"))]
pub fn run(
    _runtime: crate::runtime::RuntimeService,
    _tokio_handle: tokio::runtime::Handle,
) -> Result<(), String> {
    Err("GPUI frontend is only available on macOS".to_string())
}
