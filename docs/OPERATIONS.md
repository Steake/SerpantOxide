# Operations Guide

This document is for the operator who wants the thing to run, not the philosopher who wishes to discuss whether it ought to have been written at all.

## Installation

### 1. Build Prerequisites

- Rust toolchain
- system browser support for `chromiumoxide`
- optional external binaries:
  - `nmap`
  - `sqlmap`
  - `holehe`
  - `sherlock`
  - `theHarvester`
  - `python3`

### 2. Build and Run

```bash
cd Serpantoxide
cargo run
```

### 2a. Frontend Selection

```bash
# Default CLI TUI
cargo run

# Experimental macOS GPUI shell
cargo run -- --gpui

# Force the TUI explicitly
cargo run -- --tui
```

The CLI default is the TUI. The GPUI shell exists, but remains opt-in on macOS until it is visually and operationally less suspect.

### 2b. Package a macOS App Bundle

```bash
scripts/package_macos_app.sh
scripts/package_macos_app.sh --target x86_64-apple-darwin --zip
```

The bundle launcher stages a bundle-local runtime directory under `Contents/Resources/runtime`.
When it is opened as a macOS app it defaults to `--gpui`; when it is invoked from a terminal it defaults to `--tui` unless you pass an explicit frontend flag.

### 3. Verification

```bash
cargo fmt
cargo check
```

## Environment Variables

### LLM and search

```bash
OPENROUTER_API_KEY=...
TAVILY_API_KEY=...
LLM_MODEL=openai/gpt-4o
```

### Image generation

```bash
GOOGLE_API_KEY=...
```

### EVM analysis

```bash
EVM_RPC_URL=https://...
ETHERSCAN_API_KEY=...
```

## Local Configuration

Serpantoxide persists its selected model and last target to:

```text
.serpantoxide_config
```

When running from the packaged macOS app bundle, the config file is stored inside the bundle runtime directory pointed to by `SERPANTOXIDE_HOME`, rather than in the repository checkout.

The format is plain JSON:

```json
{
  "selected_model": "openai/gpt-4o",
  "last_target": "example.org"
}
```

## Runtime Commands

### Mission commands

```text
/agent <task>
/crew <task>
/target <host>
/topology
/report
```

### Visibility commands

```text
/tools
/notes [category]
/memory
/prompt
/modes
/help
```

### Exit

```text
/quit
/exit
/q
```

## Runtime Outputs

Serpantoxide writes operational artifacts into `loot/`.

### Notes

```text
loot/notes.json
```

Persistent shared findings are stored here. Categories are free-form, though the current code frequently uses values such as `finding`, `credential`, and `vulnerability`.

### Screenshots

```text
loot/artifacts/screenshots/
```

Browser screenshots are written here as PNG files.

### Generated Images

```text
loot/images/
```

`image_gen` writes generated PNG files into this directory.

## Operational Modes

### TUI Interaction Model

The Rust console is no longer a keyboard-only sermon delivered by a machine to a captive audience.

- worker output streams live into the interface while runs are active
- click an agent in the right rail to open its detail pane
- click a tool in the agent pane to inspect arguments and output
- click the topology strip to open the topology explorer
- use `/topology` to open the explorer directly in fullscreen mode

The TUI is the supported CLI surface today.

### GPUI Shell

The macOS GPUI shell is available through `--gpui` or through the packaged `.app` bundle launcher.

- it uses the same runtime service as the TUI
- it is not yet feature-complete relative to the TUI
- if it looks wrong, use the TUI and treat the shell as an experimental native frontend rather than a finished operator console

Inside the topology explorer:

- `Tab` or left/right switches focus between `Hosts`, `Selected Host Detail`, `Host Graph Canvas`, and `Findings / Access`
- up/down acts on the focused panel
- `Enter` or `f` toggles fullscreen
- mouse wheel scrolls the panel under the cursor
- `Esc` closes the explorer

### Mock mode

If `OPENROUTER_API_KEY` is absent, the LLM engine enters deterministic mock mode. This is useful for:

- UI development
- orchestration testing
- worker-loop verification

It is not useful for real assessment work. One should not confuse a mannequin with a field operative.

### Partial native tool availability

If binaries such as `nmap` or `sqlmap` are missing, some wrappers fall back to mock output. That makes development easier and operational certainty harder. You should know which world you are in.

## Browser Runtime Notes

The browser engine uses `chromiumoxide` and maintains one active page reference. Browser actions include:

- navigation
- content extraction
- link and form enumeration
- screenshot capture
- clicking
- typing
- JavaScript execution

If no page is active, interactive actions will fail until navigation occurs first. This is not a bug. It is causality.

## Topology Explorer Notes

The topology explorer is driven by graph snapshots rather than by reverse-engineering its own rendered text, which would be an excellent method if one wished to go mad.

- host relationships are inferred from shared `/24` subnet, shared services, and shared credentials
- the graph canvas is an ASCII layout, not a graphical viewport
- the selected host is the center of the local relationship map
- the detail pane shows the active peer-link rationale while the canvas shows the spatial arrangement

## Common Failure Modes

### Browser launch failure

Symptom:

- startup logs show a native browser engine error

Effect:

- browser-backed worker actions become unavailable

Likely causes:

- Chromium launch problem
- environment incompatibility

### Search returns weak results

Symptom:

- `web_search` output is thin or generic

Likely causes:

- missing `TAVILY_API_KEY`
- poor query specificity

### EVM tool failure

Symptom:

- `evm_chain` returns RPC or explorer errors

Likely causes:

- missing `EVM_RPC_URL`
- unsupported network selection
- missing `ETHERSCAN_API_KEY` for explorer-backed actions

### Image generation failure

Symptom:

- `image_gen` reports missing provider key or no image payload

Likely causes:

- absent `GOOGLE_API_KEY`
- model/provider response mismatch

## Recommended Operator Practice

- set the target first
- use `/crew` for campaign-level work
- use `/agent` for a single bounded problem
- read `/tools` before improvising worker instructions
- generate `/report` only after there is actually something worth reporting

This last point deserves emphasis. A report generated too early is merely a confession that one values paperwork over evidence.
