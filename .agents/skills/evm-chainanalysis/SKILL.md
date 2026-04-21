---
name: evm-chainanalysis
description: Use when investigating EVM-compatible chains, tracing funds, decoding calldata, resolving proxies, reading contract storage, or analyzing malicious transactions with Serpantoxide's native `evm_chain` tool.
---

# EVM Chainanalysis

Use this skill for Ethereum and other EVM-compatible investigations where the work is primarily on-chain.

## Use This Skill When

- Tracing ETH or token flows across addresses.
- Decoding transaction calldata or function selectors.
- Reading contract bytecode, storage slots, or event logs.
- Resolving EIP-1967 proxies and identifying implementations.
- Investigating exploit paths, flashloan sequences, or laundering routes.

## Project-Specific Workflow

Serpantoxide already exposes a native `evm_chain` tool. Prefer it before reaching for ad hoc scripts.

### 1. Configure the chain context

- Prefer `EVM_RPC_URL` in the environment for RPC-backed actions.
- If needed, set state explicitly:

```text
EVM: set_config rpc_url=https://... network=ethereum
```

### 2. Use the native actions directly

Core actions exposed by `src/evm_chain.rs`:

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

Examples:

```text
EVM: transactions 0x...
EVM: token_transfers 0x...
EVM: tx_decode tx_hash=0x...
EVM: storage 0x... slot=0x0
EVM: resolve_proxy 0x...
EVM: logs 0x... from_block=0x... to_block=latest topics=["0xddf252ad..."]
```

## Investigation Order

1. Confirm the chain, target addresses, and relevant block range.
2. Pull normal transactions and token transfers to build a first-hop map.
3. Decode suspicious calldata and identify contract roles.
4. Resolve proxy contracts before reasoning about logic.
5. Read storage or logs for hidden state and ownership clues.
6. Summarize fund paths, key counterparties, and confidence level.

## When To Escalate Beyond `evm_chain`

Use external tooling only when the native tool is not enough:

- `cast` for manual RPC reads, selector work, and storage math.
- Python or Node scripts for bulk log scraping or cross-transaction correlation.
- Explorer UIs or trace platforms for deep call-stack analysis.

For heuristics, proxy slots, hack-analysis patterns, and tracing guidance, read [references/evm-playbook.md](references/evm-playbook.md).
