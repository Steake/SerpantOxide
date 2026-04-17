pub fn help_text() -> &'static str {
    r#"Commands:
  /agent <task>        Run a focused autonomous assessment
  /crew <task>         Run multi-agent crew mode
  /target <host>       Set the active target
  /tools               Show native worker capabilities
  /notes [category]    Show saved findings
  /memory              Show graph intelligence summary
  /prompt              Show the crew system prompt
  /report              Generate a markdown report
  /models              Open model picker
  /modes               Show available execution modes
  /clear               Clear telemetry
  /quit                Exit Serpantoxide
"#
}

pub fn modes_text() -> &'static str {
    r#"Modes:
  Assist: raw command and report workflow
  Agent: single autonomous planner focused on one objective
  Crew: multi-agent orchestration with checklist + worker monitoring

Worker task prefixes:
  NMAP: <host>
  SQLMAP: <url>
  BROWSER: <url>
  SEARCH: <query>
  TERMINAL: <command>
  OSINT: <tool and target>
  HOSTING: <action and path>
  IMAGE: <prompt>
  EVM: <action and address/query>
"#
}

pub fn worker_capabilities_text() -> &'static str {
    r#"Crew worker capabilities:
  Available tools inside each worker:
  - terminal: local command execution
  - browser: navigation and page inspection
  - web_search: target-specific intelligence lookup
  - notes: persistent shared findings
  - nmap: fast host/service discovery
  - sqlmap: automated SQLi validation
  - osint: holehe, sherlock, theHarvester
  - hosting: lightweight local content hosting
  - image_gen: native image generation to local loot
  - evm_chain: EVM RPC and explorer analysis
  - finish: complete, skip, or fail plan steps

Use explicit targets and prefer parallel, independent subtasks.
Direct tool prefixes like NMAP:, OSINT:, or EVM: are still accepted for forced execution."#
}

pub fn build_crew_prompt(
    target: &str,
    task: &str,
    insights: &[String],
    current_plan: &[String],
) -> String {
    let insights_block = if insights.is_empty() {
        "- No prior findings yet.".to_string()
    } else {
        insights
            .iter()
            .map(|item| format!("- {}", item))
            .collect::<Vec<_>>()
            .join("\n")
    };

    let plan_block = if current_plan.is_empty() {
        "- No checklist published yet.".to_string()
    } else {
        current_plan
            .iter()
            .map(|item| format!("- {}", item))
            .collect::<Vec<_>>()
            .join("\n")
    };

    format!(
        r#"You are the lead of an authorized penetration testing crew. Coordinate multiple workers with maximum parallelism.

Target:
{target}

Operator Objective:
{task}

Shared Intelligence:
{insights_block}

Current Checklist:
{plan_block}

Crew tools:
- spawn_agent(task, priority?, depends_on?): Launch a worker. Provide a concrete natural-language task. Prefixes like NMAP:, SQLMAP:, BROWSER:, SEARCH:, TERMINAL: are optional when you need to force a specific tool path.
- wait_for_agents(agent_ids?): Wait for one or more workers and collect results.
- get_agent_status(agent_id): Inspect one worker.
- cancel_agent(agent_id): Stop one worker.
- formulate_strategy(problem, candidates, selected_id, rationale, feasible?): Record a strategic decision.
- update_plan(completed_tasks, remaining_tasks): Update the visible checklist in the UI.
- finish(context?): Wait for all workers and synthesize a final report.

Critical constraints:
- Do not ask the user questions.
- Maximize parallel execution. Spawn independent workers together before waiting.
- Use SEARCH only for target-specific intelligence, not generic tutorials.
- Prefer action-oriented tasks with concrete targets.
- When objectives are met, call finish.
"#
    )
}

pub fn build_worker_prompt(task: &str, plan_lines: &[String]) -> String {
    let plan_block = if plan_lines.is_empty() {
        "- No plan generated yet.".to_string()
    } else {
        plan_lines.join("\n")
    };

    format!(
        r#"You are Serpantoxide WorkerAgent, an autonomous penetration testing specialist.

Task:
{task}

Workflow:
1. Follow the current plan.
2. Use tools to make progress.
3. After each completed step, immediately call finish(action="complete", step_id=N, result="what you accomplished").
4. If a step is not applicable, call finish(action="skip", step_id=N, reason="why").
5. If a step invalidates the plan, call finish(action="fail", step_id=N, reason="why").

Rules:
- Do not ask the operator questions.
- Use notes(action="create", ...) for findings that matter to the wider crew.
- Prefer concrete actions over narration.
- Web search is only for target-specific intelligence, not general tutorials.

Current plan:
{plan_block}

Available tools:
- terminal(command, timeout?, working_dir?, inputs?, privileged?)
- browser(action, url?, selector?, text?, javascript?, wait_for?, timeout?)
- web_search(query)
- notes(action, key?, value?, category?, target?, source?, username?, password?, protocol?, port?, cve?, url?, evidence_path?)
- nmap(target)
- sqlmap(url)
- osint(tool, target)
- hosting(action, content_path?)
- image_gen(prompt, model?, output_file?)
- evm_chain(action, address?, rpc_url?, network?, selector?, slot?, data?, topics?, from_block?, to_block?, block_number?, tx_hash?, offset?)
- finish(action, step_id, result?, reason?)
"#
    )
}
