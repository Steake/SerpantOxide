use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde_json::json;

pub async fn generate(
    prompt: &str,
    model_alias: Option<&str>,
    output_file: Option<&str>,
) -> Result<String, String> {
    let api_key = std::env::var("GOOGLE_API_KEY")
        .map_err(|_| "GOOGLE_API_KEY not found in environment".to_string())?;

    let model = resolve_model(model_alias.unwrap_or("nano-banana"));
    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
        model, api_key
    );

    let client = reqwest::Client::new();
    let response = client
        .post(url)
        .json(&json!({
            "contents": [{
                "parts": [{"text": prompt}]
            }],
            "generationConfig": {
                "responseModalities": ["TEXT", "IMAGE"]
            }
        }))
        .send()
        .await
        .map_err(|e| format!("Image generation request failed: {}", e))?;

    if !response.status().is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(format!("Image generation failed: {}", body));
    }

    let body: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("Invalid image generation response: {}", e))?;

    let inline = body["candidates"][0]["content"]["parts"]
        .as_array()
        .and_then(|parts| {
            parts
                .iter()
                .find_map(|part| part["inlineData"]["data"].as_str().map(ToString::to_string))
        })
        .ok_or_else(|| "No image payload returned by model".to_string())?;

    let bytes = STANDARD
        .decode(inline)
        .map_err(|e| format!("Failed to decode image bytes: {}", e))?;

    let path = if let Some(output) = output_file {
        PathBuf::from(output)
    } else {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        PathBuf::from(format!("loot/images/nav_gen_{}.png", ts))
    };

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create image output dir: {}", e))?;
    }
    fs::write(&path, bytes).map_err(|e| format!("Failed to write image: {}", e))?;

    Ok(format!(
        "Image generated successfully ({}): {}",
        model,
        path.display()
    ))
}

fn resolve_model(alias: &str) -> &str {
    match alias {
        "nano-banana-pro" => "gemini-2.5-flash-image-preview",
        "nano-banana" => "gemini-2.5-flash-image-preview",
        other => other,
    }
}
