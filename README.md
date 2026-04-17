<div align="center">

<img src="../assets/pentestagent-logo.png" alt="PentestAgent Logo" width="180" />

# Serpantoxide
### The Rust Nerve Center for PentestAgent

</div>

Serpantoxide exists for an unfashionable but necessary reason: orchestration ought not to be fragile, sluggish, or dependent on a tower of runtime excuses. The Rust binary takes the operational heart of PentestAgent, namely multi-agent delegation, terminal tooling, browser automation, note persistence, graph intelligence, and the terminal UI, and places it under a regime of explicit state, bounded concurrency, and sternly typed control flow.

This is not an abstract research toy. It is a working offensive-security console with a crew orchestrator, autonomous worker agents, native browser control, and a telemetry-rich TUI. Where the Python system proved the thesis, Serpantoxide attempts the less glamorous labor of enforcement.

## What It Does

- Runs a Ratatui-based terminal interface with model telemetry, topology, logs, checklist state, and worker inspection.
- Uses an LLM-driven crew orchestrator to spawn, wait on, cancel, and synthesize worker activity.
- Runs worker agents as iterative tool-calling loops with planning, replanning, step completion, and final summaries.
- Provides native worker tools for `terminal`, `browser`, `web_search`, `notes`, `nmap`, `sqlmap`, `osint`, `hosting`, `image_gen`, and `evm_chain`.
- Persists findings to `loot/notes.json` and derives strategic hints through a lightweight graph model.
- Falls back to deterministic mock behavior when the OpenRouter key is absent, which is a practical concession to development rather than an act of mystical foresight.

## Quick Start

### Requirements

- Rust toolchain with `cargo`
- Chromium or compatible browser environment for `chromiumoxide`
- Optional native binaries for tool execution:
  - `nmap`
  - `sqlmap`
  - `holehe`
  - `sherlock`
  - `theHarvester`
- Optional API keys:
  - `OPENROUTER_API_KEY`
  - `TAVILY_API_KEY`
  - `GOOGLE_API_KEY`
  - `ETHERSCAN_API_KEY`
  - `EVM_RPC_URL`

### Run

```bash
cd Serpantoxide
cargo run
```

### Useful Runtime Commands

```text
/agent <task>        Run a focused autonomous assessment
/crew <task>         Run multi-agent crew mode
/target <host>       Set the active target
/tools               Show worker capabilities
/notes [category]    Show stored findings
/memory              Show graph-derived intelligence
/prompt              Show the crew prompt
/report              Generate a markdown report
/modes               Show mode and prefix help
/quit                Exit
```

## Runtime Model

Serpantoxide has three operational layers:

1. `main.rs` boots engines, starts the TUI, and wires shared state.
2. `orchestrator.rs` acts as mission control. It decides whether to spawn workers, wait for them, revise the checklist, or end the run.
3. `worker_agent.rs` performs the grubby work. Each worker plans, uses tools, marks steps complete or failed, and produces a summary.

The design is not subtle:

- The orchestrator thinks in campaigns.
- The workers think in concrete steps.
- The TUI tells you whether either of them has lost the plot.

## Native Tool Surface

Workers can call the following tools:

- `terminal`: shell execution with working directory, stdin input, and optional privilege escalation.
- `browser`: navigate, inspect content, enumerate links/forms, click, type, take screenshots, and execute JavaScript.
- `web_search`: Tavily-backed target research.
- `notes`: shared durable findings.
- `nmap`: fast host and service discovery.
- `sqlmap`: injection verification.
- `osint`: `holehe`, `sherlock`, `theHarvester`.
- `hosting`: lightweight local HTTP exposure for generated or staged artifacts.
- `image_gen`: image generation via Google’s image-capable models.
- `evm_chain`: RPC and explorer-backed EVM analysis.

There is also a forced-prefix path for explicit worker intent:

```text
NMAP: <host>
SQLMAP: <url>
BROWSER: <url>
SEARCH: <query>
TERMINAL: <command>
OSINT: <tool and target>
HOSTING: <action and path>
IMAGE: <prompt>
EVM: <action and address/query>
```

## Configuration

Serpantoxide loads its selected model from `.serpantoxide_config` and falls back to `LLM_MODEL` when that file is absent. The LLM engine itself uses OpenRouter when `OPENROUTER_API_KEY` is present and enters deterministic mock mode otherwise.

Common environment variables:

```bash
OPENROUTER_API_KEY=...
TAVILY_API_KEY=...
GOOGLE_API_KEY=...
ETHERSCAN_API_KEY=...
EVM_RPC_URL=...
LLM_MODEL=openai/gpt-4o
```

## Project Layout

```text
Serpantoxide/
  src/
    main.rs           Boot sequence and command loop
    tui.rs            Ratatui interface
    orchestrator.rs   Crew orchestration loop
    pool.rs           Worker lifecycle and dependency handling
    worker_agent.rs   Autonomous worker agent
    llm.rs            OpenRouter integration and mock path
    browser.rs        Native browser automation
    notes.rs          Persistent note store
    graph.rs          Shadow graph and strategic hints
    terminal.rs       Shell execution
    nmap.rs           Nmap integration
    sqlmap.rs         Sqlmap integration
    osint.rs          OSINT tool wrappers
    hosting.rs        Local file hosting
    image_gen.rs      Image generation
    evm_chain.rs      EVM chain tooling
    prompts.rs        Orchestrator and worker prompt text
    events.rs         UI event envelopes
    config.rs         Local configuration
```

## Documentation Set

- [ARCHITECTURE.md](./ARCHITECTURE.md): component structure, control flow, and design boundaries.
- [docs/OPERATIONS.md](./docs/OPERATIONS.md): installation, commands, outputs, and runtime expectations.
- [docs/TOOL_REFERENCE.md](./docs/TOOL_REFERENCE.md): worker tools and their contracts.
- [docs/DEVELOPMENT.md](./docs/DEVELOPMENT.md): extension points, coding map, and verification workflow.

## Verification

```bash
cargo fmt
cargo check
```

If `OPENROUTER_API_KEY` is missing, the program still runs, though in mock mode. This is not fraud. It is simply the difference between a laboratory and a field deployment.

## Legal and Operational Reality

Use this system only against targets you are authorized to assess. Tools do not confer permission. They merely accelerate consequences.
