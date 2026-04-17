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

Serpantoxide persists its selected model to:

```text
.serpantoxide_config
```

The format is plain JSON:

```json
{
  "selected_model": "openai/gpt-4o"
}
```

## Runtime Commands

### Mission commands

```text
/agent <task>
/crew <task>
/target <host>
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
