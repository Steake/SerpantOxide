# Tool Reference

The worker tool registry is the practical constitution of Serpantoxide. Everything else is rhetoric until a worker calls a tool and the machine must either perform or confess.

## Worker Tool Registry

## `terminal`

Runs a shell command.

### Parameters

- `command` (required)
- `timeout`
- `working_dir`
- `inputs`
- `privileged`

### Notes

- Uses `sh -lc`
- Can feed stdin
- Can wrap command execution with `sudo -S`

## `browser`

Runs native browser automation through `chromiumoxide`.

### Actions

- `navigate`
- `screenshot`
- `get_content`
- `get_links`
- `get_forms`
- `click`
- `type`
- `execute_js`

### Common parameters

- `action` (required)
- `url`
- `selector`
- `text`
- `javascript`
- `wait_for`
- `timeout`

### Operational notes

- `click`, `type`, and `execute_js` require an active page
- `navigate` can optionally wait for a selector
- `screenshot` writes to `loot/artifacts/screenshots/`

## `web_search`

Performs target-focused intelligence gathering through Tavily.

### Parameters

- `query` (required)

### Output

- summary text when available
- up to five cited source URLs

## `notes`

Reads and writes shared findings.

### Actions

- `create`
- `update`
- `read`
- `list`

### Common parameters

- `action` (required)
- `key`
- `value`
- `category`
- `target`
- `source`
- `username`
- `password`
- `protocol`
- `port`
- `cve`
- `url`
- `evidence_path`

### Persistence

Writes to `loot/notes.json`.

## `nmap`

Runs a fast scan against a target.

### Parameters

- `target` (required)

### Side effects

- parsed services are ingested into the shadow graph
- open services are added to worker loot

## `sqlmap`

Runs `sqlmap` in batch mode against a URL.

### Parameters

- `url` (required)

### Side effects

- findings are parsed into vulnerability strings
- vulnerabilities are ingested into the graph

## `osint`

Runs supported OSINT binaries.

### Parameters

- `tool` (required)
- `target` (required)

### Supported values

- `holehe`
- `sherlock`
- `theHarvester`

## `hosting`

Runs a lightweight local HTTP server.

### Parameters

- `action` (required)
- `content_path`

### Supported actions

- `start`
- `stop`
- `status`

### Notes

- binds to `http://127.0.0.1:8000`
- uses `python3 -m http.server`

## `image_gen`

Generates images through Google’s image-capable models.

### Parameters

- `prompt` (required)
- `model`
- `output_file`

### Notes

- defaults to writing PNG output into `loot/images/`
- requires `GOOGLE_API_KEY`

## `evm_chain`

Runs EVM analysis against an RPC endpoint and, where needed, an explorer API.

### Parameters

- `action` (required)
- `address`
- `rpc_url`
- `network`
- `selector`
- `slot`
- `data`
- `topics`
- `from_block`
- `to_block`
- `block_number`
- `tx_hash`
- `offset`

### Supported actions

- `set_config`
- `abi_lookup`
- `transactions`
- `token_transfers`
- `contract_info`
- `balance`
- `bytecode`
- `storage`
- `call`
- `logs`
- `block_info`
- `tx_decode`
- `resolve_proxy`

### Notes

- RPC-backed actions require `rpc_url` or `EVM_RPC_URL`
- explorer-backed actions benefit from `ETHERSCAN_API_KEY`

## `finish`

Marks a plan step as resolved.

### Parameters

- `action` (required)
- `step_id` (required)
- `result`
- `reason`

### Supported actions

- `complete`
- `skip`
- `fail`

This tool is not optional ceremony. Without it, the worker has not truly advanced its plan; it has merely produced text and hoped someone else would dignify it with meaning.

## Forced Task Prefixes

The worker pool rewrites explicit prefixes into natural-language worker tasks:

```text
NMAP: ...
SQLMAP: ...
SEARCH: ...
BROWSER: ...
TERMINAL: ...
OSINT: ...
HOSTING: ...
IMAGE: ...
EVM: ...
```

These prefixes are useful when you want to be explicit about the path of execution rather than trusting the model to infer the obvious, a trust which experience teaches one to extend sparingly.
