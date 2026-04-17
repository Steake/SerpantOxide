use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use serde_json::{Value, json};

#[derive(Default)]
struct EvmConfig {
    rpc_url: Option<String>,
    network: Option<String>,
}

fn config() -> &'static Mutex<EvmConfig> {
    static CONFIG: OnceLock<Mutex<EvmConfig>> = OnceLock::new();
    CONFIG.get_or_init(|| Mutex::new(EvmConfig::default()))
}

pub async fn run(
    action: &str,
    address: Option<&str>,
    rpc_url: Option<&str>,
    network: Option<&str>,
    params: &Value,
) -> Result<String, String> {
    match action {
        "set_config" => set_config(params),
        "abi_lookup" => abi_lookup(params["selector"].as_str()).await,
        "transactions" => account_api("txlist", address, network, params).await,
        "token_transfers" => account_api("tokentx", address, network, params).await,
        "contract_info" => contract_info(address, network).await,
        "balance" => {
            rpc(
                "eth_getBalance",
                json!([required_address(address)?, "latest"]),
                rpc_url,
            )
            .await
        }
        "bytecode" => {
            rpc(
                "eth_getCode",
                json!([required_address(address)?, "latest"]),
                rpc_url,
            )
            .await
        }
        "storage" => {
            rpc(
                "eth_getStorageAt",
                json!([
                    required_address(address)?,
                    params["slot"].as_str().unwrap_or("0x0"),
                    "latest"
                ]),
                rpc_url,
            )
            .await
        }
        "call" => {
            rpc(
                "eth_call",
                json!([
                    {
                        "to": required_address(address)?,
                        "data": params["data"].as_str().unwrap_or("0x")
                    },
                    "latest"
                ]),
                rpc_url,
            )
            .await
        }
        "logs" => {
            rpc(
                "eth_getLogs",
                json!([{
                    "address": address,
                    "fromBlock": params["from_block"].as_str().unwrap_or("latest"),
                    "toBlock": params["to_block"].as_str().unwrap_or("latest"),
                    "topics": params["topics"].as_array().cloned().unwrap_or_default()
                }]),
                rpc_url,
            )
            .await
        }
        "block_info" => {
            rpc(
                "eth_getBlockByNumber",
                json!([params["block_number"].as_str().unwrap_or("latest"), true]),
                rpc_url,
            )
            .await
        }
        "tx_decode" => tx_decode(params["tx_hash"].as_str(), rpc_url).await,
        "resolve_proxy" => resolve_proxy(address, rpc_url).await,
        other => Err(format!("Unsupported evm_chain action: {}", other)),
    }
}

fn set_config(params: &Value) -> Result<String, String> {
    let mut guard = config()
        .lock()
        .map_err(|_| "EVM config lock poisoned".to_string())?;
    if let Some(rpc_url) = params["rpc_url"].as_str() {
        guard.rpc_url = Some(rpc_url.to_string());
    }
    if let Some(network) = params["network"].as_str() {
        guard.network = Some(network.to_string());
    }
    Ok("EVM configuration updated.".to_string())
}

async fn rpc(method: &str, params: Value, override_rpc: Option<&str>) -> Result<String, String> {
    let rpc_url = resolve_rpc(override_rpc)?;
    let client = reqwest::Client::new();
    let response = client
        .post(rpc_url)
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": method,
            "params": params
        }))
        .send()
        .await
        .map_err(|e| format!("RPC request failed: {}", e))?;
    let body: Value = response
        .json()
        .await
        .map_err(|e| format!("Invalid RPC response: {}", e))?;
    if body.get("error").is_some() {
        return Err(body["error"].to_string());
    }
    Ok(
        serde_json::to_string_pretty(&body["result"])
            .unwrap_or_else(|_| body["result"].to_string()),
    )
}

async fn tx_decode(tx_hash: Option<&str>, override_rpc: Option<&str>) -> Result<String, String> {
    let tx_hash = tx_hash.ok_or_else(|| "tx_hash is required".to_string())?;
    let rpc_url = resolve_rpc(override_rpc)?;
    let client = reqwest::Client::new();
    let response = client
        .post(rpc_url)
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "eth_getTransactionByHash",
            "params": [tx_hash]
        }))
        .send()
        .await
        .map_err(|e| format!("RPC request failed: {}", e))?;
    let body: Value = response
        .json()
        .await
        .map_err(|e| format!("Invalid RPC response: {}", e))?;
    let input = body["result"]["input"].as_str().unwrap_or("0x");
    let selector = if input.len() >= 10 {
        &input[..10]
    } else {
        input
    };
    let abi = abi_lookup(Some(selector))
        .await
        .unwrap_or_else(|_| "No signature found".to_string());
    Ok(format!(
        "TX: {}\nSelector: {}\nPotential Signatures:\n{}\n\nRaw Input: {}",
        tx_hash, selector, abi, input
    ))
}

async fn resolve_proxy(
    address: Option<&str>,
    override_rpc: Option<&str>,
) -> Result<String, String> {
    let address = required_address(address)?;
    let slot = "0x360894A13BA1A3210667C828492DB98DCA3E2076CC3735A920A3CA505D382BBC";
    let value = rpc(
        "eth_getStorageAt",
        json!([address, slot, "latest"]),
        override_rpc,
    )
    .await?;
    Ok(format!(
        "EIP-1967 implementation slot {}\nValue: {}",
        slot, value
    ))
}

async fn abi_lookup(selector: Option<&str>) -> Result<String, String> {
    let selector = selector.ok_or_else(|| "selector is required".to_string())?;
    let url = format!(
        "https://www.4byte.directory/api/v1/signatures/?hex_signature={}",
        selector
    );
    let body: Value = reqwest::get(url)
        .await
        .map_err(|e| format!("ABI lookup failed: {}", e))?
        .json()
        .await
        .map_err(|e| format!("Invalid ABI lookup response: {}", e))?;
    let signatures = body["results"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item["text_signature"].as_str())
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if signatures.is_empty() {
        Ok(format!("No signature found for {}", selector))
    } else {
        Ok(signatures.join("\n"))
    }
}

async fn account_api(
    action: &str,
    address: Option<&str>,
    network: Option<&str>,
    params: &Value,
) -> Result<String, String> {
    let address = required_address(address)?;
    let url = scan_api_url(resolve_network(network));
    let mut query = HashMap::from([
        ("module", "account".to_string()),
        ("action", action.to_string()),
        ("address", address.to_string()),
        ("sort", "desc".to_string()),
        ("page", "1".to_string()),
        (
            "offset",
            params["offset"].as_u64().unwrap_or(10).to_string(),
        ),
    ]);
    if let Ok(api_key) = std::env::var("ETHERSCAN_API_KEY") {
        query.insert("apikey", api_key);
    }
    let client = reqwest::Client::new();
    let response = client
        .get(url)
        .query(&query)
        .send()
        .await
        .map_err(|e| format!("Explorer request failed: {}", e))?;
    let body: Value = response
        .json()
        .await
        .map_err(|e| format!("Invalid explorer response: {}", e))?;
    Ok(serde_json::to_string_pretty(&body["result"]).unwrap_or_else(|_| body.to_string()))
}

async fn contract_info(address: Option<&str>, network: Option<&str>) -> Result<String, String> {
    let address = required_address(address)?;
    let url = scan_api_url(resolve_network(network));
    let mut query = HashMap::from([
        ("module", "contract".to_string()),
        ("action", "getsourcecode".to_string()),
        ("address", address.to_string()),
    ]);
    if let Ok(api_key) = std::env::var("ETHERSCAN_API_KEY") {
        query.insert("apikey", api_key);
    }
    let client = reqwest::Client::new();
    let response = client
        .get(url)
        .query(&query)
        .send()
        .await
        .map_err(|e| format!("Explorer request failed: {}", e))?;
    let body: Value = response
        .json()
        .await
        .map_err(|e| format!("Invalid explorer response: {}", e))?;
    Ok(serde_json::to_string_pretty(&body["result"]).unwrap_or_else(|_| body.to_string()))
}

fn required_address(address: Option<&str>) -> Result<&str, String> {
    address.ok_or_else(|| "address is required".to_string())
}

fn resolve_rpc(override_rpc: Option<&str>) -> Result<String, String> {
    if let Some(rpc) = override_rpc {
        return Ok(rpc.to_string());
    }
    if let Ok(guard) = config().lock() {
        if let Some(rpc) = &guard.rpc_url {
            return Ok(rpc.clone());
        }
    }
    std::env::var("EVM_RPC_URL").map_err(|_| "EVM_RPC_URL is not set".to_string())
}

fn resolve_network(override_network: Option<&str>) -> String {
    if let Some(network) = override_network {
        return network.to_string();
    }
    if let Ok(guard) = config().lock() {
        if let Some(network) = &guard.network {
            return network.clone();
        }
    }
    "mainnet".to_string()
}

fn scan_api_url(network: String) -> String {
    match network.as_str() {
        "sepolia" => "https://api-sepolia.etherscan.io/api".to_string(),
        "goerli" => "https://api-goerli.etherscan.io/api".to_string(),
        "optimism" => "https://api-optimistic.etherscan.io/api".to_string(),
        "arbitrum" => "https://api.arbiscan.io/api".to_string(),
        "polygon" => "https://api.polygonscan.com/api".to_string(),
        "base" => "https://api.basescan.org/api".to_string(),
        _ => "https://api.etherscan.io/api".to_string(),
    }
}
