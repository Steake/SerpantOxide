# Serpantoxide

Serpantoxide is the Rust command nerve-centre for autonomous security operations. It exists because orchestration is too important to leave in the custody of ad hoc glue, runtime whim, and the usual theatrical promises about agentic systems. The point here is not to sound futuristic. The point is to make the machinery behave.

This repository contains a working offensive-security console with a crew orchestrator, iterative worker agents, browser control, note persistence, topology intelligence, and two operator surfaces: a production TUI and an experimental macOS GPUI shell. It is not a philosophy seminar disguised as a framework. It is an instrument.

## What It Actually Does

- Runs a Ratatui-based terminal interface with live telemetry, worker logs, topology views, and inspection panes.
- Exposes an experimental macOS GPUI frontend with the same shared runtime.
- Uses an LLM-driven orchestrator to plan, spawn, monitor, and synthesize worker activity.
- Executes worker agents as stepwise tool-calling loops with replanning and final summaries.
- Provides native tools for `terminal`, `browser`, `web_search`, `notes`, `nmap`, `sqlmap`, `osint`, `hosting`, `image_gen`, and `evm_chain`.
- Persists findings in `loot/notes.json` and builds higher-level hints through a lightweight graph model.
- Falls back to deterministic mock behavior when the LLM provider key is absent, which is less glamorous than calling it magic and considerably more accurate.

## Quick Start

### Requirements

- Rust toolchain with `cargo`
- Chromium or a compatible browser runtime for `chromiumoxide`
- Optional native binaries:
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
cargo run
```

### Frontends

```bash
# Default interface
cargo run

# Experimental macOS shell
cargo run -- --gpui

# Force the TUI explicitly
cargo run -- --tui
```

### Package The macOS App

```bash
scripts/package_macos_app.sh
scripts/package_macos_app.sh --target x86_64-apple-darwin --zip
```

## Runtime Commands

```text
/agent <task>        Run a focused autonomous assessment
/crew <task>         Run multi-agent crew mode
/target <host>       Set the active target
/tools               Show worker capabilities
/notes [category]    Show stored findings
/memory              Show graph-derived intelligence
/topology            Open the interactive topology explorer
/prompt              Show the crew prompt
/report              Generate a markdown report
/modes               Show mode and prefix help
/quit                Exit
```

## Architecture In Four Parts

1. `main.rs` selects the frontend and boots the runtime.
2. `runtime.rs` provides the shared command, event, and snapshot layer.
3. `orchestrator.rs` thinks in campaigns and delegates work.
4. `worker_agent.rs` does the grubby business of using tools, marking progress, and coming back with something worth reading.

That division is deliberate. The orchestrator is there to think in terms larger than a shell command, and the workers are there to stop those thoughts from remaining ornamental.

## Tool Surface

Workers can call:

- `terminal`
- `browser`
- `web_search`
- `notes`
- `nmap`
- `sqlmap`
- `osint`
- `hosting`
- `image_gen`
- `evm_chain`

Forced intent prefixes are also supported:

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

Serpantoxide stores local runtime state in `.serpantoxide_config` and falls back to `LLM_MODEL` when that file is absent. OpenRouter is used when `OPENROUTER_API_KEY` is present; otherwise the runtime drops into deterministic mock behavior. This is not deception. It is merely the difference between a live provider and a rehearsal room.

Common environment variables:

```bash
OPENROUTER_API_KEY=...
TAVILY_API_KEY=...
GOOGLE_API_KEY=...
ETHERSCAN_API_KEY=...
EVM_RPC_URL=...
LLM_MODEL=openai/gpt-4o
```

## Repository Layout

```text
src/
  main.rs
  runtime.rs
  tui.rs
  gpui_app.rs
  orchestrator.rs
  pool.rs
  worker_agent.rs
  llm.rs
  browser.rs
  notes.rs
  graph.rs
  terminal.rs
  nmap.rs
  sqlmap.rs
  osint.rs
  hosting.rs
  image_gen.rs
  evm_chain.rs
  prompts.rs
  events.rs
  config.rs
```

## Documentation

- [ARCHITECTURE.md](./ARCHITECTURE.md)
- [docs/OPERATIONS.md](./docs/OPERATIONS.md)
- [docs/TOOL_REFERENCE.md](./docs/TOOL_REFERENCE.md)
- [docs/DEVELOPMENT.md](./docs/DEVELOPMENT.md)
- [docs/GIT_SPLIT.md](./docs/GIT_SPLIT.md)

## Verification

```bash
cargo fmt
cargo check
cargo test
```

## Legal And Moral Clarity

Use this only against targets you are authorised to assess. A fast tool does not become an ethical tool by the simple expedient of being well-written.
