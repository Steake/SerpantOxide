# Development Guide

Every codebase eventually reveals what it thinks of its maintainers. Serpantoxide, to its credit, is still readable enough to be extended without requiring a séance.

## Development Principles

Three principles govern sane work in this codebase:

- keep orchestration logic separate from tool execution
- prefer explicit state transitions over magical inference
- make UI state observable rather than implicit

If a change violates one of these, it should face a presumption of guilt.

## Code Map

### Entry and runtime composition

- `src/main.rs`
- `src/config.rs`
- `src/startup_trace.rs`
- `src/runtime.rs`

### UI and events

- `src/tui.rs`
- `src/gpui_app.rs`
- `src/events.rs`

### LLM and prompts

- `src/llm.rs`
- `src/prompts.rs`
- `src/mission.rs`

### Orchestration

- `src/orchestrator.rs`
- `src/pool.rs`
- `src/worker_agent.rs`

### Persistence and intelligence

- `src/notes.rs`
- `src/graph.rs`

### Tool modules

- `src/browser.rs`
- `src/terminal.rs`
- `src/web_search.rs`
- `src/nmap.rs`
- `src/sqlmap.rs`
- `src/osint.rs`
- `src/hosting.rs`
- `src/image_gen.rs`
- `src/evm_chain.rs`

## How To Add a New Worker Tool

### 1. Create the module

Add a focused module under `src/`. Keep the interface narrow and return `Result<String, String>` unless there is a compelling reason to introduce richer structures.

### 2. Register the tool

Update `worker_agent.rs`:

- add the tool schema in `worker_tools()`
- add a match arm in `execute_tool()`
- add a dedicated execution helper if the behavior is non-trivial

### 3. Document the tool to the model

Update `prompts.rs`:

- add the tool to `worker_capabilities_text()`
- add the tool signature to `build_worker_prompt()`

If the orchestrator may intentionally force that tool path, also add a prefix conversion in `pool.rs`.

### 4. Decide on graph or note side effects

Not every tool deserves graph ingestion. Some merely produce output. Others create strategic state and should update notes or the graph.

### 5. Verify

```bash
cargo fmt
cargo check
```

## How To Change the UI

Both frontends are now driven by:

- command input from the user
- typed `RuntimeCommand` values
- typed `UiEvent` values emitted by orchestrator and workers
- shared `RuntimeSnapshot` reads assembled by `runtime.rs`

If you need richer UI behavior, do not simply add more log lines and pretend structure has emerged from noise. Extend `UiEvent` in `events.rs`, emit those events at the source, update `runtime.rs` snapshot assembly as needed, and then teach the frontend to render them properly.

Current examples of this principle in action:

- worker lifecycle and streaming output use structured worker events rather than inferred log parsing
- tool timelines in the agent detail pane are driven by explicit tool-history state
- the topology explorer reads graph snapshots directly instead of trying to rehydrate meaning from the compact topology strip

The TUI remains the default CLI frontend. The GPUI shell is experimental and should be treated as a native shell under construction, not as the canonical operator workflow.

## How To Change Prompts

Prompt text lives in `src/prompts.rs`. This is both convenient and dangerous.

Convenient, because:

- there is one obvious place to edit
- prompt surfaces are easy to compare

Dangerous, because:

- prompt changes are code changes
- prompt regressions can hide inside otherwise innocent diffs

Prompt changes should therefore be treated as behavioral changes, not copy edits.

## Mock Mode Development

When `OPENROUTER_API_KEY` is absent, `llm.rs` provides deterministic mock responses. Use this mode for:

- UI layout work
- event wiring
- worker loop validation
- local refactors where live model behavior is not the variable under examination

Do not use mock mode to draw conclusions about real-agent quality. It tells you whether the plumbing works, not whether the brain does.

## Testing and Verification

At present, the principal local verification path is compilation and runtime exercise:

```bash
cargo fmt
cargo check
cargo run
cargo run -- --gpui
```

There is room for a more formal test suite, particularly around:

- worker tool execution helpers
- graph extraction and ingestion
- topology snapshot and relationship derivation
- text-canvas topology rendering
- orchestration tool handling
- event serialization

For macOS packaging work, also verify:

```bash
bash -n scripts/package_macos_app.sh
scripts/package_macos_app.sh --help
```

That work remains to be done.

## Known Design Tensions

### The graph is intentionally small

It is useful, but not yet sophisticated. Resist the urge to turn it into a grand ontological machine before the operational need is proven. The current topology explorer works because the graph derives just enough structure to support host lists, peer links, and findings; it does not require a doctoral thesis in graph theory.

### Tool wrappers are opportunistic

They implement the useful path first. If you expand them, do so because the workflow demands it, not because the upstream tool’s manual was long and impressive.

### Prompt logic and state logic are adjacent

This is workable at current scale. It may become strained if policy, evaluation, or model-specific prompting grows more elaborate.

## Documentation Maintenance

If you change architecture materially, update:

- `README.md`
- `docs/OPERATOR_RUNTIME_SPEC.md`
- `docs/TOOL_REFERENCE.md`
- `docs/OPERATIONS.md`
- `docs/macos-gpui-frontend-spec.md` when the native-shell behavior or rollout assumptions change

The cardinal sin of technical documentation is not incompleteness. It is falsehood. An outdated architecture document is a lie written in Markdown.
