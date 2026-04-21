# Serpantoxide macOS GPUI Frontend Specification

## Summary

- Build a `macOS-first` desktop frontend in `GPUI` around the existing Rust runtime.
- Treat the current modules (`orchestrator`, `worker_agent`, `pool`, `llm`, `notes`, `graph`, tool wrappers) as the retained core and replace `ratatui` as the primary operator surface.
- Preserve current product behaviors: target management, single-agent runs, crew runs, topology exploration, worker inspection, report generation, notes, model selection, and live telemetry.
- Product direction: a real `Mac app`, not a GUI-skinned terminal, while keeping operator-grade depth in detail views and a command palette.

## Options Considered

### GPUI

- Chosen. Best fit for the brief: new, Rust-native, visually fast, and closest to a Zed-like macOS feel.
- Tradeoff: thinner docs and smaller ecosystem, so runtime and UI must stay cleanly separated.

### Slint

- Best fallback if delivery risk matters more than macOS-specific feel.
- Strong tooling and production readiness, but less specifically Apple-like in interaction and visual identity.

### Dioxus

- Rejected for this spec. Official desktop today is WebView-based, while the newer native renderer is promising but still the less conservative choice for a native macOS desktop shell.

### Tauri

- Explicitly not chosen because it is not a native Rust UI toolkit; it is a Rust backend plus web frontend shell.

## Public Interfaces and Runtime Contract

- Extract a reusable runtime layer from `main.rs` and `tui.rs` into a shared runtime service.
- Replace serialized string UI messages with typed frontend-facing contracts:
  - `RuntimeCommand`
  - `RuntimeSnapshot`
  - expanded `UiEvent`
- Keep slash command compatibility through `parse_slash_command`.
- Support both frontends during migration by adapting the TUI and GPUI shell to the same runtime.

### `RuntimeCommand`

- `SetTarget`
- `RunAgent`
- `RunCrew`
- `GenerateReport`
- `SelectModel`
- `OpenNotes`
- `CancelWorker`
- `RetryWorker`
- `Shutdown`

Additional operator commands such as help, modes, prompt preview, topology dump, memory view, and log clearing are also part of the runtime command surface for the TUI migration path.

### `RuntimeSnapshot`

- Target
- LLM telemetry and model catalog
- Checklist state
- Activity log
- Worker summaries and detail state
- Topology snapshot
- Note categories and note payloads
- Latest report and latest crew summary
- Shutdown flag

### `UiEvent`

Retains the existing worker, checklist, and crew events while adding:

- `ReportReady`
- `TargetUpdated`
- `ModelChanged`
- `ModelsUpdated`
- `TelemetryUpdated`
- `TopologyUpdated`
- `NotesUpdated`
- `LogsCleared`
- `ShutdownRequested`

## UI Specification

### App Shell

- One main macOS window with left sidebar navigation:
  - `Mission`
  - `Agents`
  - `Topology`
  - `Notes`
  - `Reports`
  - `Settings`
- Top action bar with current target, current model, and primary actions:
  - `Run Agent`
  - `Run Crew`
  - `Generate Report`
- Use pane-based layout rather than terminal-style nested popups.

### Mission

- Current target, model, status, latency, and token telemetry.
- Mission checklist.
- Live activity stream.
- Compact active-worker summary.

### Agents

- Master-detail worker layout.
- Worker detail shows:
  - status
  - task
  - result
  - error
  - loot
  - tool history

### Topology

- Topology metrics for hosts, services, web assets, vulnerabilities, and credentials.
- Native pane layout for hosts and relationships.
- Current implementation preserves `ShadowGraph` semantics and replaces the TUI ASCII summary with pane-based structured views.

### Notes

- Category list.
- Note detail pane by category.

### Reports

- Latest generated report or latest crew summary.

### Settings

- Runtime overview.
- Model catalog.
- Settings-adjacent quick actions for report generation and navigation.

## Architecture and Migration

### Phase 1

- Introduce typed runtime state and events.
- Move boot and orchestration wiring into `RuntimeService`.

### Phase 2

- Build the GPUI shell and implement `Mission`, `Agents`, and the shared action surface.

### Phase 3

- Implement `Topology`, `Notes`, `Reports`, and `Settings`.

### Phase 4

- Make the GPUI app production-ready enough to consider changing the default macOS behavior.
- Until then, keep the CLI default on the TUI and launch GPUI explicitly with `--gpui` or through the packaged app bundle.

## Test Plan

- Unit tests for slash-command parsing into `RuntimeCommand`.
- Unit tests for worker-to-snapshot adaptation.
- Manual runtime verification:
  - start the default CLI TUI
  - run with `--gpui`
  - set target
  - start single-agent run
  - start crew run
  - inspect worker details
  - generate report
  - switch model

## Assumptions

- Scope is `macOS first`.
- Interaction target is a conventional `Mac app`.
- Initial distribution is internal/operator-facing.
- GPUI is chosen for frontend feel, not because it has the broadest Rust GUI ecosystem.
- The current implementation keeps GPUI opt-in while the TUI remains the stable default CLI surface.

## Sources

- [GPUI official site](https://www.gpui.rs/)
- [Slint desktop support docs](https://docs.slint.dev/latest/docs/slint/guide/platforms/desktop/)
- [Dioxus desktop guide](https://dioxuslabs.com/learn/0.7/guides/platforms/desktop/)
- [Dioxus 0.7 release notes](https://dioxuslabs.com/blog/release-070/)
- [Tauri official overview](https://tauri.app/start/)
