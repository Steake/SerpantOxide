use petgraph::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum NodeType {
    Host,
    Service,
    Credential,
    Vulnerability,
    Finding,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeData {
    pub label: String,
    pub node_type: NodeType,
    pub metadata: HashMap<String, String>,
}

pub struct ShadowGraph {
    graph: DiGraph<NodeData, String>,
    node_indices: HashMap<String, NodeIndex>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TopologyHost {
    pub label: String,
    pub services: Vec<String>,
    pub credentials: usize,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TopologyWebFinding {
    pub label: String,
    pub vulnerabilities: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TopologySnapshot {
    pub host_count: usize,
    pub service_count: usize,
    pub web_count: usize,
    pub vulnerability_count: usize,
    pub credential_count: usize,
    pub hosts: Vec<TopologyHost>,
    pub web_findings: Vec<TopologyWebFinding>,
    pub credential_links: Vec<String>,
    pub relationships: Vec<TopologyRelationship>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TopologyRelationship {
    pub source: String,
    pub target: String,
    pub reasons: Vec<String>,
}

impl ShadowGraph {
    pub fn new() -> Self {
        Self {
            graph: DiGraph::new(),
            node_indices: HashMap::new(),
        }
    }

    pub fn add_node(&mut self, id: &str, label: &str, node_type: NodeType) -> NodeIndex {
        if let Some(&idx) = self.node_indices.get(id) {
            return idx;
        }

        let idx = self.graph.add_node(NodeData {
            label: label.to_string(),
            node_type,
            metadata: HashMap::new(),
        });
        self.node_indices.insert(id.to_string(), idx);
        idx
    }

    pub fn add_edge(&mut self, source_id: &str, target_id: &str, edge_type: &str) {
        if let (Some(&src), Some(&target)) = (
            self.node_indices.get(source_id),
            self.node_indices.get(target_id),
        ) {
            self.graph.add_edge(src, target, edge_type.to_string());
        }
    }

    pub fn extract_from_note(&mut self, category: &str, content: &str) {
        use regex::Regex;
        let ip_regex = Regex::new(r"\b(?:\d{1,3}\.){3}\d{1,3}\b").unwrap();
        let url_regex = Regex::new(r#"https?://[^\s)>"]+"#).unwrap();
        let domain_regex = Regex::new(r"\b(?:[a-zA-Z0-9-]+\.)+[a-zA-Z]{2,}\b").unwrap();

        let ips: Vec<String> = ip_regex
            .find_iter(content)
            .map(|m| m.as_str().to_string())
            .collect();

        for ip in &ips {
            self.add_node(&format!("host:{}", ip), ip, NodeType::Host);
        }

        for url in url_regex.find_iter(content).map(|m| m.as_str()) {
            self.add_node(&format!("web:{}", url), url, NodeType::Finding);
        }

        for domain in domain_regex.find_iter(content).map(|m| m.as_str()) {
            if !domain.parse::<std::net::IpAddr>().is_ok() {
                self.add_node(&format!("host:{}", domain), domain, NodeType::Host);
            }
        }

        if category == "credential" && !ips.is_empty() {
            let cred_id = format!("cred:{}", content.chars().take(10).collect::<String>());
            self.add_node(&cred_id, "Credential Found", NodeType::Credential);
            for ip in ips {
                self.add_edge(&cred_id, &format!("host:{}", ip), "AUTH_ACCESS");
            }
        }
    }

    pub fn get_strategic_insights(&self) -> Vec<String> {
        let mut insights = Vec::new();

        for idx in self.graph.node_indices() {
            let data = &self.graph[idx];
            if data.node_type == NodeType::Credential {
                let neighbors: Vec<_> = self
                    .graph
                    .neighbors(idx)
                    .map(|n| self.graph[n].label.clone())
                    .collect();
                if !neighbors.is_empty() {
                    insights.push(format!(
                        "Valid credentials found for: {}",
                        neighbors.join(", ")
                    ));
                }
            } else if data.node_type == NodeType::Vulnerability {
                insights.push(format!("Confirmed Vulnerability: {}", data.label));
            }
        }

        if insights.is_empty() {
            insights.push(
                "No strategic insights available yet. Continue scanning targets.".to_string(),
            );
        }

        insights
    }

    pub fn ingest_nmap(&mut self, host: &str, ports: Vec<(String, String)>) {
        let host_id = format!("host:{}", host);
        let host_idx = self.add_node(&host_id, host, NodeType::Host);
        self.graph[host_idx]
            .metadata
            .insert("kind".to_string(), "network_host".to_string());

        for (port_str, service) in ports {
            let svc_id = format!("service:{}:{}", host, port_str);
            let label = format!("{} ({})", port_str, service);
            let svc_idx = self.add_node(&svc_id, &label, NodeType::Service);
            self.graph[svc_idx]
                .metadata
                .insert("host".to_string(), host.to_string());
            self.graph[svc_idx]
                .metadata
                .insert("port".to_string(), port_str.clone());
            self.graph[svc_idx]
                .metadata
                .insert("service".to_string(), service.clone());
            self.add_edge(&host_id, &svc_id, "HAS_SERVICE");
        }
    }

    pub fn ingest_sqlmap(&mut self, url: &str, vulnerabilities: Vec<String>) {
        let url_id = format!("url:{}", url);
        let url_idx = self.add_node(&url_id, url, NodeType::Finding);
        self.graph[url_idx]
            .metadata
            .insert("kind".to_string(), "web_endpoint".to_string());
        self.graph[url_idx]
            .metadata
            .insert("url".to_string(), url.to_string());

        for vuln in vulnerabilities {
            let vuln_id = format!("vuln:{}:{}", url, vuln.chars().take(10).collect::<String>());
            let vuln_idx = self.add_node(&vuln_id, &vuln, NodeType::Vulnerability);
            self.graph[vuln_idx]
                .metadata
                .insert("scope".to_string(), url.to_string());
            self.add_edge(&url_id, &vuln_id, "VULNERABLE_TO");
        }
    }

    pub fn snapshot(&self) -> TopologySnapshot {
        let hosts = self.collect_hosts();
        let web_findings = self.collect_web_findings();
        let credential_links = self.collect_credential_links();
        let relationships = self.collect_host_relationships(&hosts);

        TopologySnapshot {
            host_count: hosts.len(),
            service_count: self.count_nodes(NodeType::Service),
            web_count: web_findings.len(),
            vulnerability_count: self.count_nodes(NodeType::Vulnerability),
            credential_count: credential_links.len(),
            hosts,
            web_findings,
            credential_links,
            relationships,
        }
    }

    pub fn to_ascii_topology(&self, width: u16, height: u16) -> String {
        let mut lines = Vec::new();
        let snapshot = self.snapshot();
        let hosts = &snapshot.hosts;
        let web_findings = &snapshot.web_findings;
        let credential_links = &snapshot.credential_links;
        let vulnerability_count = snapshot.vulnerability_count;
        let service_count = snapshot.service_count;
        let host_count = snapshot.host_count;
        let web_count = snapshot.web_count;

        if host_count == 0 && web_count == 0 && credential_links.is_empty() {
            return "Awaiting reconnaissance. No hosts, services, or web assets have been mapped yet."
                .to_string();
        }

        lines.push(self.fit_line(
            &format!(
                "Discovery: hosts {} | services {} | web {} | vulns {} | creds {}",
                host_count,
                service_count,
                web_count,
                vulnerability_count,
                credential_links.len()
            ),
            width,
        ));

        let hottest_host = hosts
            .iter()
            .max_by_key(|host| host.services.len())
            .map(|host| {
                format!(
                    "Top host: {} ({} services)",
                    host.label,
                    host.services.len()
                )
            });
        let hottest_web = web_findings
            .iter()
            .max_by_key(|finding| finding.vulnerabilities.len())
            .map(|finding| {
                format!(
                    "Top web asset: {} ({} vulns)",
                    finding.label,
                    finding.vulnerabilities.len()
                )
            });

        if let Some(summary) = hottest_host.or(hottest_web) {
            lines.push(self.fit_line(&summary, width));
        }

        if !hosts.is_empty() {
            lines.push("".to_string());
            lines.push("Network Surface".to_string());
            for host in hosts.iter().take(4) {
                let service_preview = if host.services.is_empty() {
                    "no open services".to_string()
                } else {
                    host.services
                        .iter()
                        .take(4)
                        .cloned()
                        .collect::<Vec<_>>()
                        .join(", ")
                };
                lines.push(self.fit_line(
                    &format!(
                        "├─ {} [{} svc{}]",
                        host.label,
                        host.services.len(),
                        if host.credentials > 0 {
                            format!(", {} cred", host.credentials)
                        } else {
                            String::new()
                        }
                    ),
                    width,
                ));
                lines.push(self.fit_line(&format!("│  {}", service_preview), width));
            }
            if hosts.len() > 4 {
                lines.push(self.fit_line(
                    &format!("└─ {} more hosts not shown", hosts.len() - 4),
                    width,
                ));
            }
        }

        if !web_findings.is_empty() {
            lines.push("".to_string());
            lines.push("Web Findings".to_string());
            for finding in web_findings.iter().take(3) {
                lines.push(self.fit_line(&format!("├─ {}", finding.label), width));
                if finding.vulnerabilities.is_empty() {
                    lines.push(self.fit_line("│  no confirmed vulnerabilities yet", width));
                } else {
                    for vuln in finding.vulnerabilities.iter().take(2) {
                        lines.push(self.fit_line(&format!("│  ! {}", vuln), width));
                    }
                    if finding.vulnerabilities.len() > 2 {
                        lines.push(self.fit_line(
                            &format!(
                                "│  … {} more vulnerability indicators",
                                finding.vulnerabilities.len() - 2
                            ),
                            width,
                        ));
                    }
                }
            }
            if web_findings.len() > 3 {
                lines.push(self.fit_line(
                    &format!("└─ {} more web assets not shown", web_findings.len() - 3),
                    width,
                ));
            }
        }

        if !credential_links.is_empty() {
            lines.push("".to_string());
            lines.push("Access Paths".to_string());
            for link in credential_links.iter().take(3) {
                lines.push(self.fit_line(&format!("├─ {}", link), width));
            }
            if credential_links.len() > 3 {
                lines.push(self.fit_line(
                    &format!(
                        "└─ {} more credential links not shown",
                        credential_links.len() - 3
                    ),
                    width,
                ));
            }
        }

        self.limit_height(lines, height)
    }

    fn collect_hosts(&self) -> Vec<TopologyHost> {
        let mut hosts = self
            .graph
            .node_indices()
            .filter(|&i| self.graph[i].node_type == NodeType::Host)
            .map(|host_idx| {
                let label = self.graph[host_idx].label.clone();
                let mut services = self
                    .graph
                    .neighbors(host_idx)
                    .filter(|&n| self.graph[n].node_type == NodeType::Service)
                    .map(|n| self.graph[n].label.clone())
                    .collect::<Vec<_>>();
                services.sort();
                services.dedup();

                let credentials = self
                    .graph
                    .neighbors_directed(host_idx, Incoming)
                    .filter(|&n| self.graph[n].node_type == NodeType::Credential)
                    .count();

                TopologyHost {
                    label,
                    services,
                    credentials,
                }
            })
            .collect::<Vec<_>>();

        hosts.sort_by(|a, b| {
            b.services
                .len()
                .cmp(&a.services.len())
                .then_with(|| a.label.cmp(&b.label))
        });
        hosts
    }

    fn collect_web_findings(&self) -> Vec<TopologyWebFinding> {
        let mut findings = self
            .graph
            .node_indices()
            .filter(|&i| self.graph[i].node_type == NodeType::Finding)
            .map(|finding_idx| {
                let label = self.graph[finding_idx].label.clone();
                let mut vulnerabilities = self
                    .graph
                    .neighbors(finding_idx)
                    .filter(|&n| self.graph[n].node_type == NodeType::Vulnerability)
                    .map(|n| self.graph[n].label.clone())
                    .collect::<Vec<_>>();
                vulnerabilities.sort();
                vulnerabilities.dedup();
                TopologyWebFinding {
                    label,
                    vulnerabilities,
                }
            })
            .collect::<Vec<_>>();

        findings.sort_by(|a, b| {
            b.vulnerabilities
                .len()
                .cmp(&a.vulnerabilities.len())
                .then_with(|| a.label.cmp(&b.label))
        });
        findings
    }

    fn collect_credential_links(&self) -> Vec<String> {
        let mut links = HashSet::new();
        for cred_idx in self
            .graph
            .node_indices()
            .filter(|&i| self.graph[i].node_type == NodeType::Credential)
        {
            let targets = self
                .graph
                .neighbors(cred_idx)
                .map(|n| self.graph[n].label.clone())
                .collect::<Vec<_>>();

            if targets.is_empty() {
                links.insert(self.graph[cred_idx].label.clone());
            } else {
                links.insert(format!(
                    "{} -> {}",
                    self.graph[cred_idx].label,
                    targets.join(", ")
                ));
            }
        }

        let mut sorted = links.into_iter().collect::<Vec<_>>();
        sorted.sort();
        sorted
    }

    fn collect_host_relationships(&self, hosts: &[TopologyHost]) -> Vec<TopologyRelationship> {
        let mut relationships = Vec::new();

        for (idx, host) in hosts.iter().enumerate() {
            for peer in hosts.iter().skip(idx + 1) {
                let mut reasons = Vec::new();

                if same_ipv4_subnet(&host.label, &peer.label) {
                    reasons.push("same /24 subnet".to_string());
                }

                let shared_services = shared_service_names(&host.services, &peer.services);
                if !shared_services.is_empty() {
                    reasons.push(format!("shared services: {}", shared_services.join(", ")));
                }

                let shared_credentials = self.shared_credential_count(&host.label, &peer.label);
                if shared_credentials > 0 {
                    reasons.push(format!("shared credentials: {}", shared_credentials));
                }

                if !reasons.is_empty() {
                    relationships.push(TopologyRelationship {
                        source: host.label.clone(),
                        target: peer.label.clone(),
                        reasons,
                    });
                }
            }
        }

        relationships.sort_by(|a, b| {
            a.source
                .cmp(&b.source)
                .then_with(|| a.target.cmp(&b.target))
        });
        relationships
    }

    fn shared_credential_count(&self, host_a: &str, host_b: &str) -> usize {
        let host_a_id = format!("host:{}", host_a);
        let host_b_id = format!("host:{}", host_b);

        let Some(&host_a_idx) = self.node_indices.get(&host_a_id) else {
            return 0;
        };
        let Some(&host_b_idx) = self.node_indices.get(&host_b_id) else {
            return 0;
        };

        self.graph
            .node_indices()
            .filter(|&idx| self.graph[idx].node_type == NodeType::Credential)
            .filter(|&cred_idx| {
                let neighbors = self.graph.neighbors(cred_idx).collect::<HashSet<_>>();
                neighbors.contains(&host_a_idx) && neighbors.contains(&host_b_idx)
            })
            .count()
    }

    fn count_nodes(&self, node_type: NodeType) -> usize {
        self.graph
            .node_indices()
            .filter(|&idx| self.graph[idx].node_type == node_type)
            .count()
    }

    fn fit_line(&self, text: &str, width: u16) -> String {
        let max_width = width.saturating_sub(4) as usize;
        if max_width == 0 {
            return String::new();
        }
        if text.chars().count() <= max_width {
            return text.to_string();
        }

        let mut truncated = text
            .chars()
            .take(max_width.saturating_sub(1))
            .collect::<String>();
        truncated.push('…');
        truncated
    }

    fn limit_height(&self, mut lines: Vec<String>, height: u16) -> String {
        let max_lines = height.saturating_sub(2) as usize;
        if max_lines == 0 {
            return String::new();
        }

        if lines.len() > max_lines {
            let remaining = lines.len() - max_lines + 1;
            lines.truncate(max_lines.saturating_sub(1));
            lines.push(format!(
                "… {} more lines available via /topology later",
                remaining
            ));
        }

        lines.join("\n")
    }
}

fn same_ipv4_subnet(a: &str, b: &str) -> bool {
    let a_parts = a
        .split('.')
        .map(str::trim)
        .filter_map(|part| part.parse::<u8>().ok())
        .collect::<Vec<_>>();
    let b_parts = b
        .split('.')
        .map(str::trim)
        .filter_map(|part| part.parse::<u8>().ok())
        .collect::<Vec<_>>();

    a_parts.len() == 4 && b_parts.len() == 4 && a_parts[..3] == b_parts[..3]
}

fn shared_service_names(a: &[String], b: &[String]) -> Vec<String> {
    let a_names = a
        .iter()
        .map(|svc| canonical_service_name(svc))
        .collect::<HashSet<_>>();
    let mut shared = b
        .iter()
        .map(|svc| canonical_service_name(svc))
        .filter(|svc| a_names.contains(svc))
        .collect::<HashSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    shared.sort();
    shared
}

fn canonical_service_name(service_label: &str) -> String {
    if let Some((_, rhs)) = service_label.split_once('(') {
        rhs.trim_end_matches(')').trim().to_string()
    } else {
        service_label.to_string()
    }
}
