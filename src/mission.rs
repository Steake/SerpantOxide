use crate::graph::TopologySnapshot;

const DEFAULT_PRESET_ID: &str = "auto";

struct MissionPresetSpec {
    id: &'static str,
    title: &'static str,
    summary: &'static str,
}

const PRESETS: &[MissionPresetSpec] = &[
    MissionPresetSpec {
        id: "auto",
        title: "Auto",
        summary: "Infer the best immediate mission outcome from current discoveries.",
    },
    MissionPresetSpec {
        id: "recon",
        title: "Recon",
        summary: "Expand attack surface coverage and identify the most promising foothold paths.",
    },
    MissionPresetSpec {
        id: "service-foothold",
        title: "Service Foothold",
        summary: "Turn exposed network services into a plausible access path.",
    },
    MissionPresetSpec {
        id: "web-foothold",
        title: "Web Foothold",
        summary: "Convert discovered web surface into validated exploit or authenticated access paths.",
    },
    MissionPresetSpec {
        id: "credential-access",
        title: "Credential Access",
        summary: "Exploit credential discoveries for authenticated access and reuse opportunities.",
    },
    MissionPresetSpec {
        id: "exploit-path",
        title: "Exploit Path",
        summary: "Validate the highest-confidence vulnerability path and drive it to a concrete outcome.",
    },
    MissionPresetSpec {
        id: "report",
        title: "Report",
        summary: "Consolidate findings, gaps, and operator-ready recommendations.",
    },
];

#[derive(Clone, Debug, Default)]
pub struct DiscoverySignals {
    pub topology: TopologySnapshot,
    pub note_categories: Vec<(String, usize)>,
}

impl DiscoverySignals {
    pub fn new(topology: TopologySnapshot, note_categories: Vec<(String, usize)>) -> Self {
        Self {
            topology,
            note_categories,
        }
    }

    pub fn host_count(&self) -> usize {
        self.topology.host_count
    }

    pub fn service_count(&self) -> usize {
        self.topology.service_count
    }

    pub fn web_count(&self) -> usize {
        self.topology.web_count
    }

    pub fn vulnerability_count(&self) -> usize {
        self.topology.vulnerability_count
    }

    pub fn credential_count(&self) -> usize {
        self.topology.credential_count
    }

    pub fn total_note_count(&self) -> usize {
        self.note_categories.iter().map(|(_, count)| *count).sum()
    }

    pub fn discovery_summary(&self) -> String {
        let mut segments = vec![
            format!("hosts: {}", self.host_count()),
            format!("services: {}", self.service_count()),
            format!("web: {}", self.web_count()),
            format!("vulns: {}", self.vulnerability_count()),
            format!("creds: {}", self.credential_count()),
        ];

        if !self.note_categories.is_empty() {
            segments.push(format!(
                "notes: {}",
                self.note_categories
                    .iter()
                    .map(|(name, count)| format!("{} ({})", name, count))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }

        segments.join(" | ")
    }

    pub fn heuristic_basis(&self) -> Vec<String> {
        let mut basis = Vec::new();

        if let Some(host) = self.topology.hosts.first() {
            basis.push(format!(
                "Most exposed host so far: {} ({} mapped services).",
                host.label,
                host.services.len()
            ));
        }

        if let Some(web) = self.topology.web_findings.first() {
            if web.vulnerabilities.is_empty() {
                basis.push(format!("Observed web surface: {}.", web.label));
            } else {
                basis.push(format!(
                    "Highest-value web asset: {} ({} vulnerability indicators).",
                    web.label,
                    web.vulnerabilities.len()
                ));
            }
        }

        if self.credential_count() > 0 {
            basis.push(format!(
                "Credential-linked access paths detected: {}.",
                self.credential_count()
            ));
        }

        if self.vulnerability_count() > 0 {
            basis.push(format!(
                "Confirmed vulnerability indicators detected: {}.",
                self.vulnerability_count()
            ));
        }

        if basis.is_empty() && self.total_note_count() > 0 {
            basis.push(format!(
                "Shared note store already contains {} findings.",
                self.total_note_count()
            ));
        }

        if basis.is_empty() {
            basis.push(
                "No strong discoveries yet, so broad reconnaissance remains the best next move."
                    .to_string(),
            );
        }

        basis
    }

    fn has_note_category(&self, needle: &str) -> bool {
        let needle = needle.to_ascii_lowercase();
        self.note_categories
            .iter()
            .any(|(name, count)| *count > 0 && name.to_ascii_lowercase().contains(&needle))
    }

    fn has_credentials_signal(&self) -> bool {
        self.credential_count() > 0
            || self.has_note_category("credential")
            || self.has_note_category("password")
            || self.has_note_category("auth")
    }

    fn has_vulnerability_signal(&self) -> bool {
        self.vulnerability_count() > 0
            || self.has_note_category("vuln")
            || self.has_note_category("cve")
            || self.has_note_category("finding")
    }

    fn has_web_signal(&self) -> bool {
        self.web_count() > 0 || self.has_note_category("web") || self.has_note_category("http")
    }
}

#[derive(Clone, Debug)]
pub struct MissionProfile {
    pub requested_preset: String,
    pub resolved_preset: String,
    pub preset_title: String,
    pub preset_summary: String,
    pub operator_task: String,
    pub desired_outcome: String,
    pub discovery_summary: String,
    pub heuristic_basis: Vec<String>,
    pub continuation_priorities: Vec<String>,
    pub suggested_follow_ups: Vec<String>,
}

impl MissionProfile {
    pub fn runtime_summary(&self) -> String {
        if self.requested_preset == self.resolved_preset {
            format!(
                "{} [{}] -> {}",
                self.preset_title, self.resolved_preset, self.desired_outcome
            )
        } else {
            format!(
                "{} [{} inferred from {}] -> {}",
                self.preset_title,
                self.resolved_preset,
                self.requested_preset,
                self.desired_outcome
            )
        }
    }

    pub fn execution_brief(&self, target: &str) -> String {
        let basis = self
            .heuristic_basis
            .iter()
            .map(|item| format!("- {}", item))
            .collect::<Vec<_>>()
            .join("\n");
        let priorities = self
            .continuation_priorities
            .iter()
            .enumerate()
            .map(|(idx, item)| format!("{}. {}", idx + 1, item))
            .collect::<Vec<_>>()
            .join("\n");

        format!(
            "Target: {target}\n\
             Mission preset: {} ({})\n\
             Operator objective: {}\n\
             Desired outcome: {}\n\
             Discovery summary: {}\n\
             Heuristic basis:\n{}\n\
             Continuation priorities:\n{}\n\
             Self-directed rules:\n\
             - Continue toward the desired outcome when discoveries reveal a stronger path.\n\
             - Re-prioritize if new evidence beats the current line of effort.\n\
             - Finish only when the desired outcome is satisfied or no materially better next step remains.",
            self.preset_title,
            self.resolved_preset,
            self.operator_task,
            self.desired_outcome,
            self.discovery_summary,
            basis,
            priorities
        )
    }

    pub fn continuation_nudge(&self, worker_summary: &[String]) -> String {
        let worker_block = if worker_summary.is_empty() {
            "- No worker status updates yet.".to_string()
        } else {
            worker_summary
                .iter()
                .map(|item| format!("- {}", item))
                .collect::<Vec<_>>()
                .join("\n")
        };
        let follow_up_block = if self.suggested_follow_ups.is_empty() {
            "- No deterministic follow-up suggestions available yet.".to_string()
        } else {
            self.suggested_follow_ups
                .iter()
                .map(|item| format!("- {}", item))
                .collect::<Vec<_>>()
                .join("\n")
        };

        format!(
            "Continue autonomously toward this desired outcome: {}\n\
             Current discoveries: {}\n\
             Worker state:\n{}\n\
             Strong follow-up candidates:\n{}\n\
             If work remains, update the checklist and continue. If workers are still active, wait for them. Finish only when no materially better next step remains.",
            self.desired_outcome, self.discovery_summary, worker_block, follow_up_block
        )
    }
}

pub fn default_preset_id() -> &'static str {
    DEFAULT_PRESET_ID
}

pub fn normalize_preset_id(input: &str) -> Option<String> {
    let normalized = input
        .trim()
        .to_ascii_lowercase()
        .replace('_', "-")
        .replace(' ', "-");

    let canonical = match normalized.as_str() {
        "" => DEFAULT_PRESET_ID,
        "auto" => "auto",
        "recon" | "reconnaissance" | "discovery" | "surface" | "surface-map" => "recon",
        "service" | "services" | "network" | "network-foothold" | "service-foothold" => {
            "service-foothold"
        }
        "web" | "app" | "http" | "web-foothold" | "web-app" => "web-foothold",
        "credential" | "credentials" | "cred" | "creds" | "credential-access" => {
            "credential-access"
        }
        "exploit" | "exploit-path" | "vuln" | "vulnerability" => "exploit-path",
        "report" | "reporting" => "report",
        _ => return None,
    };

    Some(canonical.to_string())
}

pub fn preset_catalog_lines(current_preset: &str) -> Vec<String> {
    let current =
        normalize_preset_id(current_preset).unwrap_or_else(|| DEFAULT_PRESET_ID.to_string());
    let mut lines = vec![format!("Current preset: {}", current)];
    lines.extend(PRESETS.iter().map(|preset| {
        let marker = if preset.id == current { "*" } else { " " };
        format!("{} {:<18} {}", marker, preset.id, preset.summary)
    }));
    lines
}

pub fn looks_like_continuation_request(task: &str) -> bool {
    let trimmed = task.trim().to_ascii_lowercase();
    trimmed.is_empty()
        || trimmed == "continue"
        || trimmed == "continue mission"
        || trimmed == "continue autonomously"
        || trimmed == "keep going"
        || trimmed == "go deeper"
        || trimmed == "next"
        || trimmed == "follow up"
        || trimmed == "follow-up"
        || trimmed == "same goal"
        || trimmed == "full autonomous assessment"
}

pub fn resolve_mission(
    requested_preset: &str,
    target: &str,
    task: &str,
    signals: &DiscoverySignals,
) -> MissionProfile {
    let requested =
        normalize_preset_id(requested_preset).unwrap_or_else(|| DEFAULT_PRESET_ID.to_string());
    let resolved = infer_preset(&requested, task, signals);
    let preset = preset_spec(&resolved);
    let operator_task = normalize_operator_task(task, target, &resolved, signals);
    let desired_outcome = desired_outcome(&resolved, target, signals);
    let heuristic_basis = build_heuristic_basis(&resolved, signals, task);
    let continuation_priorities = continuation_priorities(&resolved, target, signals);
    let suggested_follow_ups = suggested_follow_ups(&resolved, target, signals);

    MissionProfile {
        requested_preset: requested,
        resolved_preset: resolved,
        preset_title: preset.title.to_string(),
        preset_summary: preset.summary.to_string(),
        operator_task,
        desired_outcome,
        discovery_summary: signals.discovery_summary(),
        heuristic_basis,
        continuation_priorities,
        suggested_follow_ups,
    }
}

fn preset_spec(id: &str) -> &'static MissionPresetSpec {
    PRESETS
        .iter()
        .find(|preset| preset.id == id)
        .unwrap_or(&PRESETS[0])
}

fn infer_preset(requested: &str, task: &str, signals: &DiscoverySignals) -> String {
    if requested != DEFAULT_PRESET_ID {
        return requested.to_string();
    }

    let lower = task.to_ascii_lowercase();
    if lower.contains("report") || lower.contains("summary") || lower.contains("writeup") {
        return "report".to_string();
    }
    if lower.contains("credential")
        || lower.contains("password")
        || lower.contains("login")
        || lower.contains("ssh")
        || lower.contains("auth")
    {
        return "credential-access".to_string();
    }
    if lower.contains("web")
        || lower.contains("browser")
        || lower.contains("http")
        || lower.contains("https")
        || lower.contains("sqlmap")
        || lower.contains("xss")
        || lower.contains("sqli")
        || lower.contains("form")
    {
        return "web-foothold".to_string();
    }
    if lower.contains("exploit")
        || lower.contains("foothold")
        || lower.contains("shell")
        || lower.contains("rce")
        || lower.contains("vuln")
    {
        return "exploit-path".to_string();
    }
    if lower.contains("service") || lower.contains("port") || lower.contains("nmap") {
        return "service-foothold".to_string();
    }

    if signals.has_credentials_signal() {
        "credential-access".to_string()
    } else if signals.has_vulnerability_signal() {
        "exploit-path".to_string()
    } else if signals.has_web_signal() {
        "web-foothold".to_string()
    } else if signals.service_count() > 0 {
        "service-foothold".to_string()
    } else {
        "recon".to_string()
    }
}

fn normalize_operator_task(
    task: &str,
    target: &str,
    resolved_preset: &str,
    signals: &DiscoverySignals,
) -> String {
    if looks_like_continuation_request(task) {
        return match resolved_preset {
            "credential-access" => format!(
                "Continue autonomously by converting credential discoveries into validated access against {}.",
                display_target(target)
            ),
            "exploit-path" => format!(
                "Continue autonomously by validating the strongest discovered exploit path against {}.",
                display_target(target)
            ),
            "web-foothold" => format!(
                "Continue autonomously by pushing the discovered web surface on {} toward authenticated or exploitable footholds.",
                display_target(target)
            ),
            "service-foothold" => format!(
                "Continue autonomously by turning the exposed services on {} into the most plausible foothold path.",
                display_target(target)
            ),
            "report" => format!(
                "Continue autonomously by consolidating findings and remaining gaps for {}.",
                display_target(target)
            ),
            _ => format!(
                "Continue autonomously from the current discoveries and expand coverage on {}.",
                display_target(target)
            ),
        };
    }

    if task.trim().is_empty() {
        format!(
            "Drive the current mission on {} using the {} preset.",
            display_target(target),
            resolved_preset
        )
    } else if signals.host_count() == 0 && signals.web_count() == 0 {
        task.trim().to_string()
    } else {
        format!(
            "{} Keep steering toward the strongest next step exposed by the current discoveries.",
            task.trim()
        )
    }
}

fn desired_outcome(resolved_preset: &str, target: &str, signals: &DiscoverySignals) -> String {
    match resolved_preset {
        "credential-access" => format!(
            "Demonstrate whether discovered credentials can unlock authenticated access, service reuse, or a stronger pivot on {}.",
            display_target(target)
        ),
        "exploit-path" => format!(
            "Validate the highest-confidence vulnerability chain on {} and either convert it into a foothold path or clearly rule it out.",
            display_target(target)
        ),
        "web-foothold" => format!(
            "Turn the most promising web asset on {} into a confirmed exploit path, auth bypass, or clearly bounded dead end.",
            display_target(target)
        ),
        "service-foothold" => format!(
            "Prioritize the exposed services on {} and drive them toward a realistic foothold or strong negative conclusion.",
            display_target(target)
        ),
        "report" => format!(
            "Produce a tighter operator-ready account of what was found, what remains uncertain, and what should happen next for {}.",
            display_target(target)
        ),
        _ => {
            if signals.has_web_signal() || signals.service_count() > 0 {
                format!(
                    "Expand coverage on {} until the highest-value foothold path is clear enough to pursue next.",
                    display_target(target)
                )
            } else {
                format!(
                    "Map the reachable attack surface on {} and identify the strongest next-stage objective.",
                    display_target(target)
                )
            }
        }
    }
}

fn build_heuristic_basis(
    resolved_preset: &str,
    signals: &DiscoverySignals,
    task: &str,
) -> Vec<String> {
    let mut basis = signals.heuristic_basis();

    if !task.trim().is_empty() && normalize_preset_id(task).is_some() {
        basis.push("The operator task explicitly named a preset-like intent.".to_string());
    }

    match resolved_preset {
        "credential-access" if signals.has_credentials_signal() => basis.push(
            "Credential evidence exists, so authenticated access validation outranks more speculative recon.".to_string(),
        ),
        "exploit-path" if signals.has_vulnerability_signal() => basis.push(
            "Vulnerability signals already exist, so confirmation and exploitation triage should continue before broadening scope.".to_string(),
        ),
        "web-foothold" if signals.has_web_signal() => basis.push(
            "Web surface is already visible, so deeper browser-driven enumeration is likely higher value than restarting host discovery.".to_string(),
        ),
        "service-foothold" if signals.service_count() > 0 => basis.push(
            "Mapped services provide enough structure to prioritize deeper service-level follow-up instead of repeating first-pass scanning.".to_string(),
        ),
        _ => {}
    }

    basis
}

fn continuation_priorities(
    resolved_preset: &str,
    target: &str,
    _signals: &DiscoverySignals,
) -> Vec<String> {
    match resolved_preset {
        "credential-access" => vec![
            "Read shared notes and validate the most promising credential-to-service combinations first.".to_string(),
            "If authenticated access succeeds, enumerate what that access materially enables next.".to_string(),
            "Record both successful and failed reuse attempts so the crew stops re-testing dead paths.".to_string(),
        ],
        "exploit-path" => vec![
            "Focus on the highest-confidence vulnerability indicator before branching into weaker leads.".to_string(),
            "Correlate each exploit attempt with concrete evidence, affected endpoints, and resulting access.".to_string(),
            "If the exploit path collapses, pivot immediately to the next strongest discovery rather than ending the mission.".to_string(),
        ],
        "web-foothold" => vec![
            "Enumerate forms, auth flows, and high-value endpoints on the strongest web asset first.".to_string(),
            "Use confirmed web findings to decide whether to deepen browser work, sqlmap work, or authenticated follow-up.".to_string(),
            "Keep going until the web path is either converted into a foothold or bounded as a dead end.".to_string(),
        ],
        "service-foothold" => vec![
            "Prioritize the most exposed host and the most interesting mapped services first.".to_string(),
            "Use service fingerprints and auth clues to narrow the best foothold candidate instead of scanning everything evenly.".to_string(),
            "Promote any web-capable or credential-bearing service into a deeper follow-up path immediately.".to_string(),
        ],
        "report" => vec![
            "Close the highest-impact evidence gaps before generating the final narrative.".to_string(),
            "Prefer concise, operator-ready findings over exploratory branches with low payoff.".to_string(),
            "Call out what is confirmed, what is only suspected, and what should be done next.".to_string(),
        ],
        _ => vec![
            format!(
                "Continue discovery on {} until a stronger foothold path clearly outranks first-pass recon.",
                display_target(target)
            ),
            "When new evidence appears, shift immediately from broad enumeration to targeted follow-up.".to_string(),
            "Do not stop after the first pass if mapped services, web assets, or credentials suggest a deeper next step.".to_string(),
        ],
    }
}

fn suggested_follow_ups(
    resolved_preset: &str,
    target: &str,
    signals: &DiscoverySignals,
) -> Vec<String> {
    match resolved_preset {
        "credential-access" => vec![
            format!(
                "Read the shared notes, identify the best credential leads, and validate authenticated access opportunities on {}.",
                display_target(target)
            ),
            "If authentication works anywhere, enumerate what those credentials unlock and record the access path.".to_string(),
            "Summarize reusable credentials, blocked paths, and the next highest-value authenticated pivot.".to_string(),
        ],
        "exploit-path" => vec![
            "Validate the strongest discovered vulnerability path with concrete reproduction steps and evidence.".to_string(),
            "If exploitation fails, explain why and pivot to the next most credible finding instead of stopping.".to_string(),
            "Record the precise exploitability conclusion and what the crew should test next.".to_string(),
        ],
        "web-foothold" => vec![
            "Inspect the strongest discovered web asset for forms, auth flows, hidden endpoints, and clear exploit opportunities.".to_string(),
            "Correlate web findings with search intelligence and record any reproducible vulnerability path.".to_string(),
            "If the main web asset is exhausted, pivot to the next mapped web surface instead of ending the mission.".to_string(),
        ],
        "service-foothold" => {
            let highest_host = signals
                .topology
                .hosts
                .first()
                .map(|host| host.label.clone())
                .unwrap_or_else(|| display_target(target));
            vec![
                format!(
                    "Prioritize {} and investigate the most interesting mapped services for version, auth, and foothold opportunities.",
                    highest_host
                ),
                "Promote any discovered HTTP, HTTPS, SSH, RDP, or database service into a focused follow-up path.".to_string(),
                "Record which service family now looks like the best foothold candidate and why.".to_string(),
            ]
        }
        "report" => vec![
            "Collect the minimum additional evidence needed to close any major ambiguity in the current findings.".to_string(),
            "Group the strongest discoveries into attack surface, findings, and recommendations.".to_string(),
            "Highlight the single best next action if the operator resumes work later.".to_string(),
        ],
        _ => vec![
            format!(
                "Expand discovery on {} using nmap, browser inspection, and target-specific search until a higher-confidence foothold path emerges.",
                display_target(target)
            ),
            "If new web surface appears, pivot into deeper browser-driven inspection immediately.".to_string(),
            "If credentials or vulnerability indicators appear, switch from recon to validation instead of restarting enumeration.".to_string(),
        ],
    }
}

fn display_target(target: &str) -> String {
    if target.trim().is_empty() || target == "None" {
        "the active scope".to_string()
    } else {
        target.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{TopologyHost, TopologySnapshot, TopologyWebFinding};

    fn signals(
        host_count: usize,
        service_count: usize,
        web_count: usize,
        vulnerability_count: usize,
        credential_count: usize,
    ) -> DiscoverySignals {
        DiscoverySignals::new(
            TopologySnapshot {
                host_count,
                service_count,
                web_count,
                vulnerability_count,
                credential_count,
                hosts: vec![TopologyHost {
                    label: "10.10.10.10".to_string(),
                    services: vec!["80 (http)".to_string(), "22 (ssh)".to_string()],
                    credentials: credential_count,
                }],
                web_findings: if web_count > 0 {
                    vec![TopologyWebFinding {
                        label: "http://10.10.10.10".to_string(),
                        vulnerabilities: if vulnerability_count > 0 {
                            vec!["Possible SQL injection".to_string()]
                        } else {
                            Vec::new()
                        },
                    }]
                } else {
                    Vec::new()
                },
                credential_links: vec![],
                relationships: vec![],
            },
            Vec::new(),
        )
    }

    #[test]
    fn auto_prefers_credentials_when_present() {
        let mission = resolve_mission("auto", "10.10.10.10", "continue", &signals(1, 2, 1, 0, 1));
        assert_eq!(mission.resolved_preset, "credential-access");
    }

    #[test]
    fn auto_prefers_web_when_task_mentions_browser() {
        let mission = resolve_mission(
            "auto",
            "10.10.10.10",
            "inspect the web app and keep going",
            &signals(1, 2, 0, 0, 0),
        );
        assert_eq!(mission.resolved_preset, "web-foothold");
    }

    #[test]
    fn continuation_request_is_rewritten_into_outcome_driven_task() {
        let mission = resolve_mission("recon", "10.10.10.10", "continue", &signals(0, 0, 0, 0, 0));
        assert!(mission.operator_task.contains("Continue autonomously"));
        assert!(mission.desired_outcome.contains("10.10.10.10"));
    }

    #[test]
    fn preset_aliases_normalize() {
        assert_eq!(
            normalize_preset_id("creds").as_deref(),
            Some("credential-access")
        );
        assert_eq!(normalize_preset_id("web").as_deref(), Some("web-foothold"));
        assert_eq!(
            normalize_preset_id("network").as_deref(),
            Some("service-foothold")
        );
    }
}
