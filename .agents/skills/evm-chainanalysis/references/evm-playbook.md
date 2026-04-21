# EVM Chainanalysis Playbook

This reference file contains the heavier on-chain investigation guidance for Serpantoxide's EVM work.

## Tooling

- **Explorer APIs and UIs**: Etherscan-style explorers remain the fastest way to inspect verified contracts, token transfers, and account activity.
- **Foundry `cast`**: Useful for direct RPC inspection, selector decoding, and storage reads.
- **Python or Node scripts**: Use when you need to scrape many receipts or correlate large volumes of events.

Representative `cast` commands:

```bash
cast 4byte-decode <calldata>
cast storage <address> 0 --rpc-url <rpc>
cast --to-dec <hex_value>
```

## Fund-Flow Heuristics

### Exchange and service clustering

- Check whether funds sweep into known exchange hot wallets or deposit aggregators.
- Look for repeated counterparties and common timing windows.

### Mixers and privacy systems

- Tornado Cash and similar systems break deterministic tracing.
- Fall back to timing, denomination matching, bridge activity, and downstream consolidation patterns.

### Cross-chain bridges

- Identify the source-chain lock or burn event.
- Then locate the destination-chain mint or release event and continue tracing there.

### Peel chains and staging wallets

- Watch for repeated transfers where a wallet forwards most value and peels off smaller amounts.
- Prioritize the largest retained balance as the likely active control wallet.

## Contract Investigation

### Unverified contracts

If explorer verification is missing:

1. Pull bytecode.
2. Check whether the address is a proxy.
3. Use selector decoding and storage reads to infer purpose.
4. Escalate to decompilers only when needed.

### Storage analysis

- Slot `0x0` often contains ownership or guard state in simple contracts.
- Mapping entries are stored at `keccak256(key . slot)`.
- Storage reads are especially useful for unverified contracts and access-control analysis.

### Proxy patterns

Always test for proxies before attributing logic to the visible contract.

Common EIP-1967 slots:

- Implementation:
  `0x360894a13ba1a3210667c828492db98dca3e2076cc3735a920a3ca505d382bbc`
- Admin:
  `0xb53127684a568b3173ae13b9f8a6016e243e63b6e8ee1178d6a717850b5d6103`

## Hack and Exploit Analysis

### Transaction trace interpretation

When reviewing a malicious transaction:

1. Identify the initial caller, target, and transferred value.
2. Decode the selector and likely function.
3. Separate setup calls from exploit logic.
4. Track post-exploit fund movement immediately after the draining step.

### Flashloans

Flashloan exploits often follow a recognizable structure:

1. Borrow from a lending or AMM venue.
2. Manipulate state or pricing mid-transaction.
3. Extract value.
4. Repay before the transaction ends.

The exploit logic is usually between the borrow and repay legs.

### Funding-source recovery

Trace backward from the exploiter wallet to identify:

- the funding wallet,
- any bridge origin,
- any mixer withdrawal,
- or exchange off-ramp path.

## Automation Guidance

If the built-in tool is insufficient, write short scripts for:

- bulk `eth_getLogs`,
- receipt parsing,
- repeated balance snapshots,
- or cross-address transaction graphing.

Example `web3.py` snippet:

```python
from web3 import Web3

w3 = Web3(Web3.HTTPProvider("https://eth.llamarpc.com"))
target = "0x..."

balance = w3.eth.get_balance(target)
nonce = w3.eth.get_transaction_count(target)

print(f"Balance: {w3.from_wei(balance, 'ether')} ETH")
print(f"Nonce: {nonce}")
```

## Expected Output

A good investigation summary should include:

- target chain and addresses,
- relevant transactions or logs,
- decoded behaviors,
- traced inflows and outflows,
- notable infrastructure such as proxies, bridges, or mixers,
- and a short confidence statement about what is known versus inferred.
