use anyhow::Result;
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use futures_util::StreamExt;

use crate::settings::Settings;

#[derive(Debug, Serialize, Deserialize)]
pub struct OllamaGenerateRequest {
    pub model: String,
    pub prompt: String,
    pub stream: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct OllamaGenerateResponse {
    pub response: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct OllamaTagsModel {
    pub name: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct OllamaTagsResponse {
    pub models: Option<Vec<OllamaTagsModel>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct OllamaHealth {
    pub ok: bool,
    pub message: Option<String>,
    pub models: Option<Vec<String>>,
}

pub async fn check_health(settings: &Settings) -> Result<OllamaHealth, String> {
    let base = settings.ollama_base_url.as_ref()
        .map(|s| s.as_str())
        .unwrap_or("http://127.0.0.1:11434");
    
    let client = reqwest::Client::new();
    let url = format!("{}/api/tags", base);
    let resp = client.get(url).send().await;
    
    match resp {
        Ok(r) if r.status().is_success() => {
            let tags: OllamaTagsResponse = r.json().await.map_err(|e| e.to_string())?;
            let models = tags.models.unwrap_or_default()
                .into_iter()
                .filter_map(|m| m.name)
                .collect::<Vec<_>>();
            Ok(OllamaHealth { 
                ok: true, 
                message: None, 
                models: Some(models) 
            })
        }
        Ok(r) => Ok(OllamaHealth { 
            ok: false, 
            message: Some(format!("HTTP {}", r.status())), 
            models: None 
        }),
        Err(e) => Ok(OllamaHealth { 
            ok: false, 
            message: Some(e.to_string()), 
            models: None 
        }),
    }
}

pub async fn list_models(settings: &Settings) -> Result<Vec<String>, String> {
    let health = check_health(settings).await?;
    Ok(health.models.unwrap_or_default())
}

pub async fn generate(
    model: Option<String>,
    prompt: String,
    settings: &Settings,
) -> Result<String, String> {
    let base = settings.ollama_base_url.as_ref()
        .map(|s| s.as_str())
        .unwrap_or("http://127.0.0.1:11434");
    
    let model_name = model
        .or_else(|| settings.default_ollama_model.clone())
        .unwrap_or_else(|| "gemma3:1b".to_string());
    
    let body = OllamaGenerateRequest { 
        model: model_name, 
        prompt, 
        stream: false 
    };
    
    let client = reqwest::Client::new();
    let url = format!("{}/api/generate", base);
    let resp = client
        .post(url)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("ollama request failed: {e}"))?;

    if resp.status() == StatusCode::NOT_FOUND || resp.status() == StatusCode::BAD_GATEWAY {
        return Err("Ollama server not reachable. Is it running on port 11434?".to_string());
    }

    if !resp.status().is_success() {
        return Err(format!("ollama error: HTTP {}", resp.status()));
    }

    // When stream=false, Ollama returns a single JSON object with `response`
    let value: serde_json::Value = resp.json().await
        .map_err(|e| format!("response parse error: {e}"))?;
    
    if let Some(s) = value.get("response").and_then(|v| v.as_str()) {
        return Ok(s.to_string());
    }
    
    // Some servers may return multiple JSON lines even if stream=false
    if let Some(arr) = value.as_array() {
        let mut out = String::new();
        for v in arr {
            if let Some(s) = v.get("response").and_then(|x| x.as_str()) {
                out.push_str(s);
            }
        }
        if !out.is_empty() { 
            return Ok(out); 
        }
    }
    
    Err("Unexpected Ollama response format".to_string())
}

pub async fn generate_streaming(
    model: Option<String>,
    prompt: String,
    settings: &Settings,
    mut on_chunk: impl FnMut(&str),
) -> Result<(), String> {
    let base = settings.ollama_base_url.as_ref()
        .map(|s| s.as_str())
        .unwrap_or("http://127.0.0.1:11434");
    
    let model_name = model
        .or_else(|| settings.default_ollama_model.clone())
        .unwrap_or_else(|| "gemma3:1b".to_string());
    
    let body = OllamaGenerateRequest {
        model: model_name,
        prompt,
        stream: true,
    };
    
    let client = reqwest::Client::new();
    let url = format!("{}/api/generate", base);
    let resp = client
        .post(url)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("ollama request failed: {e}"))?;

    if resp.status() == StatusCode::NOT_FOUND || resp.status() == StatusCode::BAD_GATEWAY {
        return Err("Ollama server not reachable. Is it running on port 11434?".to_string());
    }

    if !resp.status().is_success() {
        return Err(format!("ollama error: HTTP {}", resp.status()));
    }

    // Stream NDJSON lines and accumulate `response` text
    let mut buf = String::new();
    let mut stream = resp.bytes_stream();
    
    while let Some(item) = stream.next().await {
        let bytes = item.map_err(|e| format!("stream error: {e}"))?;
        let chunk = String::from_utf8_lossy(&bytes);
        buf.push_str(&chunk);
        
        // Process complete lines
        let mut start_idx = 0usize;
        for (i, ch) in buf.char_indices() {
            if ch == '\n' {
                let line = &buf[start_idx..i];
                if !line.trim().is_empty() {
                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(line) {
                        if let Some(s) = json.get("response").and_then(|v| v.as_str()) {
                            if !s.is_empty() {
                                on_chunk(s);
                            }
                        }
                    }
                }
                start_idx = i + 1;
            }
        }
        
        // Keep the unfinished tail
        if start_idx > 0 {
            buf = buf[start_idx..].to_string();
        }
    }
    
    // Process any final buffered line
    let line = buf.trim();
    if !line.is_empty() {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(line) {
            if let Some(s) = json.get("response").and_then(|v| v.as_str()) {
                if !s.is_empty() {
                    on_chunk(s);
                }
            }
        }
    }
    
    Ok(())
}