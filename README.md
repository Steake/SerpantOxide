# Serpantoxide

**A Rust command centre for autonomous security operations.**

Most "agentic" tooling is a confidence trick performed with logs. Serpantoxide takes the less glamorous view that orchestration should be explicit, typed, inspectable, and difficult to romanticise. It is a Rust runtime for running a crew of security workers with a proper operator surface, shared state, browser control, topology intelligence, and just enough suspicion about its own abstractions to remain useful.

In plain English: this is a working offensive-security console. It plans, delegates, watches, revises, records, and reports. It has a production TUI, an experimental macOS GPUI shell, and very little interest in pretending that "AI" is a substitute for instrumentation.

## Why It Exists

Because the usual arrangement is intolerable.

- Python prototypes become accidental constitutions.
- Agent frameworks multiply concepts the way damp basements multiply fungus.
- Tool calls vanish into stringly typed fog.
- Interfaces soothe the operator precisely when they ought to confess uncertainty.

Serpantoxide is the opposite design instinct: fewer illusions, harder edges, better visibility.

## What You Get

- A live terminal UI with telemetry, worker logs, topology views, inspection panes, and report generation.
- An experimental native macOS shell built on the same runtime.
- A crew orchestrator that can plan, spawn, monitor, and synthesise multiple workers.
- Worker agents that operate as iterative tool-calling loops rather than decorative one-shot prompts.
- Native tools for `terminal`, `browser`, `web_search`, `notes`, `nmap`, `sqlmap`, `osint`, `hosting`, `image_gen`, and `evm_chain`.
- Persistent findings in `loot/notes.json`.
- A lightweight graph model that turns findings into something closer to intelligence.
- Deterministic mock mode when provider keys are absent, because pretending otherwise would be vulgar.

## The Shape Of The Thing

Serpantoxide is arranged around four deliberate layers:

1. `main.rs` boots the selected frontend.
2. `runtime.rs` provides the shared command, event, and snapshot model.
3. `orchestrator.rs` thinks in campaigns.
4. `worker_agent.rs` does the grubby work and returns with evidence.

That separation matters. The orchestrator is there to think at a level above a shell command. The workers are there to prevent those thoughts from floating away into rhetoric.

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
- Optional environment variables:
  - `OPENROUTER_API_KEY`
  - `TAVILY_API_KEY`
  - `GOOGLE_API_KEY`
  - `ETHERSCAN_API_KEY`
  - `EVM_RPC_URL`
  - `LLM_MODEL`

### Run It

```bash
cargo run
```

### Frontends

```bash
# Default TUI
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

And if subtlety is getting in the way, forced intent prefixes are available:

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

Serpantoxide stores local runtime state in `.serpantoxide_config`. If `OPENROUTER_API_KEY` is present, it uses OpenRouter. If it is absent, the runtime drops into deterministic mock behaviour. This is not a scam. It is simply the difference between a live provider and a rehearsal.

Typical configuration:

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

## Final Point Of Order

Use this only against systems you are authorised to assess. Good tooling does not suspend ethics. It merely removes the excuse of incompetence.
