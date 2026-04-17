use petgraph::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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

        let ips: Vec<String> = ip_regex
            .find_iter(content)
            .map(|m| m.as_str().to_string())
            .collect();

        for ip in &ips {
            self.add_node(&format!("host:{}", ip), ip, NodeType::Host);
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
        self.add_node(&host_id, host, NodeType::Host);

        for (port_str, service) in ports {
            let svc_id = format!("service:{}:{}", host, port_str);
            let label = format!("{} ({})", port_str, service);
            self.add_node(&svc_id, &label, NodeType::Service);
            self.add_edge(&host_id, &svc_id, "HAS_SERVICE");
        }
    }

    pub fn ingest_sqlmap(&mut self, url: &str, vulnerabilities: Vec<String>) {
        let url_id = format!("url:{}", url);
        self.add_node(&url_id, url, NodeType::Finding);

        for vuln in vulnerabilities {
            let vuln_id = format!("vuln:{}:{}", url, vuln.chars().take(10).collect::<String>());
            self.add_node(&vuln_id, &vuln, NodeType::Vulnerability);
            self.add_edge(&url_id, &vuln_id, "VULNERABLE_TO");
        }
    }

    pub fn to_ascii_topology(&self, width: u16) -> String {
        let mut lines = Vec::new();
        let hosts: Vec<_> = self
            .graph
            .node_indices()
            .filter(|&i| self.graph[i].node_type == NodeType::Host)
            .collect();

        if hosts.is_empty() {
            return "--- No Hosts Discovered ---".to_string();
        }

        let box_width = 20;
        let mut current_row = Vec::new();

        for host_idx in hosts {
            let data = &self.graph[host_idx];
            let label = data.label.clone();

            // Collect services
            let services: Vec<_> = self
                .graph
                .neighbors(host_idx)
                .filter(|&n| self.graph[n].node_type == NodeType::Service)
                .map(|n| self.graph[n].label.clone())
                .collect();

            let mut host_box = vec![
                format!("┌{:─^18}┐", ""),
                format!("│{:^18}│", label),
                format!("├{:─^18}┤", ""),
            ];

            if services.is_empty() {
                host_box.push(format!("│{:^18}│", "(no services)"));
            } else {
                for svc in services.iter().take(3) {
                    host_box.push(format!("│ {:<16} │", svc));
                }
            }
            host_box.push(format!("└{:─^18}┘", ""));

            current_row.push(host_box);

            if current_row.len() * (box_width + 2) > width as usize {
                // Render the collected row
                for i in 0..6 {
                    let mut line = String::new();
                    for box_lines in &current_row {
                        if i < box_lines.len() {
                            line.push_str(&box_lines[i]);
                            line.push_str("  ");
                        }
                    }
                    lines.push(line);
                }
                lines.push("".to_string());
                current_row.clear();
            }
        }

        // Render remaining row
        if !current_row.is_empty() {
            for i in 0..6 {
                let mut line = String::new();
                for box_lines in &current_row {
                    if i < box_lines.len() {
                        line.push_str(&box_lines[i]);
                        line.push_str("  ");
                    }
                }
                lines.push(line);
            }
        }

        lines.join("\n")
    }
}
