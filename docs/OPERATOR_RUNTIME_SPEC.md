# Operator Runtime Specification

This document describes the current operator-facing runtime contract for Serpantoxide as implemented in the Rust codebase after the April 23, 2026 mission, startup, and operator-surface work.

It is not a wishlist. It is the intended behavior that the TUI, runtime service, orchestrator, and packaged macOS launcher are expected to preserve.

## Scope

This specification covers:

- frontend selection and startup behavior
- runtime composition and shared state
- mission preset resolution and execution briefs
- operator command handling
- TUI input behavior, including history and autocomplete
- crew orchestration and checklist publication rules
- persistent knowledge and config storage
- observability and mock-mode behavior

This specification does not define:

- the full native GPUI parity roadmap
- every worker-tool parameter in detail
- long-term schema migration rules for persistence files

Use [docs/TOOL_REFERENCE.md](./TOOL_REFERENCE.md) for the detailed worker tool registry.

## Design Goals

The runtime should:

- keep the TUI as the default supported operator surface
- expose typed command and event flows instead of log-derived guesswork
- let the operator continue from accumulated state instead of restarting context each run
- force crew mode to publish a visible plan before it starts executing workers
- keep startup failures observable before the UI takes over
- degrade to mock or fallback modes instead of hanging invisibly

## Runtime Topology

Serpantoxide is arranged as a layered runtime:

1. `main.rs` selects the frontend and starts the runtime service.
2. `runtime.rs` owns shared services, commands, events, and persistent state.
3. `orchestrator.rs` runs mission-aware crew orchestration.
4. `worker_agent.rs` runs worker tool loops against explicit plan steps.
5. `tui.rs` and `gpui_app.rs` consume the same shared runtime state.

Shared runtime state includes:

- selected target
- selected mission preset
- selected model
- checklist state
- notes categories
- topology snapshot
- worker snapshots and tool history

## Frontend Selection Contract

### CLI behavior

The CLI supports two explicit frontend flags:

- `--tui`
- `--gpui`

If no frontend flag is supplied, the CLI defaults to the TUI.

### Packaged macOS app behavior

The packaged app launcher in `scripts/package_macos_app.sh` must preserve both GUI-style and terminal-attached usage:

- if `--tui` or `--gpui` is passed explicitly, the launcher must respect it
- if no explicit frontend flag is passed and stdin/stdout are attached to a terminal, the launcher must default to `--tui`
- if no explicit frontend flag is passed and the app is launched without a terminal, the launcher must default to `--gpui`

This preserves direct terminal access to the TUI while still allowing the `.app` bundle to behave like a native shell when launched as a GUI application.

## Startup Contract

### Startup stages

The runtime startup path must progress through these stages:

1. process start and frontend selection
2. Tokio runtime creation
3. runtime service launch
4. notes engine launch
5. LLM engine launch
6. browser engine launch with timeout
7. initial state emission
8. frontend handoff

### Startup tracing

Before the frontend takes over the terminal, startup tracing must be available in two places:

- `stderr`
- a persistent startup log file

Default log file:

```text
/tmp/serpantoxide-startup.log
```

Override:

```text
SERPANTOXIDE_STARTUP_LOG=/path/to/file
```

Tracing must stay enabled during early startup and then stop writing to `stderr` once the interactive frontend is ready, so the UI is not corrupted by later log output.

### Startup resilience rules

- OpenRouter model refresh must not block initial frontend startup.
- Browser engine launch must time out after a bounded window and fall back to read-only browser capabilities instead of hanging indefinitely.
- Missing provider credentials must enter deterministic mock mode rather than failing startup.

## Mission Preset System

### Preset catalog

The runtime currently supports these canonical mission presets:

| Preset | Intent |
| --- | --- |
| `auto` | infer the strongest current mission from discoveries |
| `recon` | expand attack-surface coverage |
| `service-foothold` | turn exposed services into a plausible foothold |
| `web-foothold` | drive discovered web surface toward exploit or auth outcomes |
| `credential-access` | validate credential reuse and authenticated pivots |
| `exploit-path` | confirm and drive the strongest vulnerability chain |
| `report` | consolidate findings and remaining uncertainty |

### Resolution rules

Preset resolution works as follows:

- if the operator explicitly selects a non-`auto` preset, that preset wins
- if the preset is `auto`, the runtime infers the resolved preset from:
  - operator task wording
  - discovered credentials
  - discovered vulnerabilities
  - discovered web findings
  - mapped services

### Mission profile

Both `/agent` and `/crew` must derive a `MissionProfile` containing:

- requested preset
- resolved preset
- preset title and summary
- normalized operator task
- desired outcome
- discovery summary
- heuristic basis
- continuation priorities
- suggested follow-up moves

### Mission brief behavior

When the operator submits a continuation-style request such as `continue`, `next`, or an empty/default crew task, the mission system must convert that into a concrete execution brief tied to the active target and the resolved preset.

This ensures the agent and crew loops continue from the current state rather than rephrasing the operator’s vaguer prompt into mush.

## Operator Command Surface

The runtime command surface currently includes:

| Command | Behavior |
| --- | --- |
| `/agent <task>` | start a focused autonomous worker run |
| `/crew <task>` | start crew orchestration |
| `/preset [name]` | show or set the mission preset |
| `/presets` | list presets |
| `/target <host>` | set the active target |
| `/tools` | show worker capabilities |
| `/notes [category]` | inspect note categories or category entries |
| `/store <category> <content>` | persist operator knowledge into shared notes |
| `/kb <category> <content>` | alias for `/store` |
| `/cancel <worker-id>` | cancel a worker |
| `/retry <worker-id>` | respawn a worker task |
| `/memory` | show graph-derived intelligence |
| `/topology` | open the topology explorer |
| `/prompt` | show the crew system prompt |
| `/report` | generate a report |
| `/config` | show runtime config |
| `/config set max_iterations <n>` | update persisted crew iteration budget |
| `/models` and `/model` | open the model picker |
| `/modes` | show execution mode help |
| `/help` | show command help |
| `/clear` | clear telemetry |
| `/quit`, `/exit`, `/q` | exit |

Freeform non-slash prompt input must continue to parse as `RunCrew`, not as an error.

## TUI Input Contract

### Freeform input and paste

The TUI prompt is a first-class operator surface, not a slash-command box with delusions of grandeur.

Rules:

- multiline paste must be accepted
- non-slash input must route through `parse_operator_input(...)`
- slash input must route through explicit slash-command parsing

### Prompt history

The TUI must keep an in-memory prompt history for the current session with these rules:

- `Up` navigates to older entries when the prompt is active
- `Down` navigates to newer entries
- the current unfinished draft is restored when navigation returns to the present
- typing, backspace, paste, and accepted completions exit history-navigation mode
- consecutive duplicate submissions are not re-added
- empty submissions are not added

### Local command completion

Before asking the LLM for help, the TUI must try deterministic slash-command completion for the first token.

This preserves quick completion for obvious cases such as `/con` to `/config`.

### LLM autocomplete

If local completion yields nothing and the input is eligible, the TUI may request an LLM completion suffix.

Autocomplete context must include:

- the current input
- the active target when available
- guidance toward normal Serpantoxide operator patterns

The prompt must favor completions such as:

- `/target <host>`
- `/crew <objective>`
- `/preset <name>`
- `/store <category> <finding>`
- `/config set max_iterations <number>`
- `NMAP: <host>`
- `BROWSER: http://<target>`
- `SEARCH: <query>`
- `EVM: <action>`

### Ghost-text acceptance rules

The autocomplete layer must:

- allow slash-command suffixes
- allow worker-prefix suffixes such as `P: `
- reject multiline, obviously prompt-leaked, or control-character output
- render the suggestion as ghost text in the prompt
- apply the suggestion with `Tab`

## Crew Orchestration Contract

### Launch behavior

When `/crew` is invoked:

1. the active target is read
2. the active preset is read
3. current discovery signals are assembled from topology and note categories
4. a mission profile is resolved
5. any stale checklist is cleared
6. the runtime logs the crew mission summary
7. orchestration starts asynchronously

### Checklist publication rule

The orchestrator must not allow the crew to proceed without first publishing a checklist via `update_plan`.

If the first orchestration response lacks `update_plan` and no current plan exists, the orchestrator must:

- log that the checklist is missing
- inject a follow-up user message requiring `update_plan`
- continue the loop instead of executing other tool calls as the first move

### Parallelism rule

Crew mode should prefer concurrent first-pass execution. The orchestrator prompt should encourage:

- `spawn_parallel_agents(...)` for the first recon spread
- several independent workers before the first wait step
- continuation after first-pass evidence if a better next step is exposed

### Iteration budget

Crew mode uses a persisted `max_iterations` value from config.

Rules:

- minimum accepted value: `1`
- maximum accepted value: `128`
- default: `16`

If the crew reaches the iteration budget without a stronger completion path, it must finish with a bounded summary rather than loop indefinitely.

## Shared Knowledge Contract

### `/store` and `/kb`

Operator-authored knowledge is stored through the notes engine rather than hidden in logs.

When `/store <category> <content>` is invoked:

- a generated note key is created from the category and a timestamp
- the note is written with source metadata identifying the operator-store command
- the active target is attached when available
- note categories are refreshed in the UI

This allows operator findings to affect future crew context, mission inference, and graph-derived hints.

## Persistent Config Contract

The runtime config file is:

```text
.serpantoxide_config
```

Current persisted fields:

```json
{
  "selected_model": "openai/gpt-4o",
  "selected_preset": "auto",
  "last_target": "example.org",
  "max_iterations": 16
}
```

When running inside the packaged macOS app, the config file lives in the bundle runtime directory selected through `SERPANTOXIDE_HOME`.

## Mock Mode Contract

If `OPENROUTER_API_KEY` is absent:

- the LLM engine must enter deterministic mock mode
- a mock model catalog must be loaded
- runtime startup must still complete
- orchestration and UI flows must remain testable

Mock mode is a plumbing aid, not operational evidence.

## Documentation Contract

If these behaviors change materially, update at minimum:

- `README.md`
- `docs/OPERATIONS.md`
- `docs/DEVELOPMENT.md`
- this specification
- `docs/macos-gpui-frontend-spec.md` when packaged or GPUI behavior changes

## Acceptance Criteria

The operator/runtime feature set described here is considered intact when all of the following are true:

- `cargo run -- --tui` reaches the TUI without waiting for a blocking model-refresh path
- early startup failures can be diagnosed through `/tmp/serpantoxide-startup.log` or its configured override
- `/preset`, `/presets`, `/store`, `/kb`, `/config`, and `/config set max_iterations <n>` all parse and execute
- freeform non-slash prompt input still launches crew mode
- `Up` and `Down` cycle prompt history and restore the unfinished draft correctly
- TUI autocomplete can suggest realistic slash-command and worker-prefix suffixes
- `/crew` clears stale checklist state and requires an initial `update_plan`
- the packaged macOS launcher defaults to TUI in a terminal and GPUI in GUI-style launches unless explicitly overridden
