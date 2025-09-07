use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use futures_util::StreamExt;

use crate::settings::Settings;

#[derive(Debug, Serialize, Deserialize)]
struct GeminiPartsRequestText {
    text: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct GeminiContentRequest {
    parts: Vec<GeminiPartsRequestText>,
}

#[derive(Debug, Serialize, Deserialize)]
struct GeminiRequestBody {
    contents: Vec<GeminiContentRequest>,
}

#[derive(Debug, Serialize, Deserialize)]
struct GeminiPartText {
    text: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct GeminiContentResponse {
    parts: Option<Vec<GeminiPartText>>,
}

#[derive(Debug, Serialize, Deserialize)]
struct GeminiCandidate {
    content: Option<GeminiContentResponse>,
}

#[derive(Debug, Serialize, Deserialize)]
struct GeminiResponseBody {
    candidates: Option<Vec<GeminiCandidate>>,
}

pub async fn generate_text(prompt: &str, settings: &Settings) -> Result<String> {
    let api_key = settings
        .gemini_api_key
        .clone()
        .or_else(|| std::env::var("GEMINI_API_KEY").ok())
        .context("Gemini API key not set")?;
    
    let url = "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.0-flash:generateContent";
    let body = GeminiRequestBody {
        contents: vec![GeminiContentRequest {
            parts: vec![GeminiPartsRequestText { 
                text: prompt.to_string() 
            }],
        }],
    };
    
    let client = reqwest::Client::new();
    let resp = client
        .post(url)
        .header("X-goog-api-key", api_key)
        .json(&body)
        .send()
        .await
        .context("gemini request failed")?;
    
    if !resp.status().is_success() {
        return Err(anyhow!("gemini error: HTTP {}", resp.status()));
    }
    
    let value: GeminiResponseBody = resp.json().await
        .context("gemini parse error")?;
    
    if let Some(cands) = value.candidates {
        for cand in cands {
            if let Some(content) = cand.content {
                if let Some(parts) = content.parts {
                    for p in parts {
                        if let Some(t) = p.text {
                            if !t.is_empty() { 
                                return Ok(t); 
                            }
                        }
                    }
                }
            }
        }
    }
    
    Err(anyhow!("gemini: no text in response"))
}

pub async fn generate_image_stream_progress(
    prompt: &str,
    settings: &Settings,
    mut on_progress: impl FnMut(u32, u32),
) -> Result<String> {
    // Helper: recursively search for inline image data or data URIs in arbitrary JSON
    fn find_image_data(v: &serde_json::Value) -> Option<String> {
        // 1) Direct inline data objects
        if let Some(obj) = v.as_object() {
            // inlineData / inline_data forms
            for key in ["inlineData", "inline_data"] {
                if let Some(inline) = obj.get(key) {
                    if let Some(data) = inline.get("data").and_then(|d| d.as_str()) {
                        if !data.is_empty() {
                            return Some(data.to_string());
                        }
                    }
                }
            }
            // media[].inlineData.data
            if let Some(media) = obj.get("media").and_then(|m| m.as_array()) {
                for m in media {
                    if let Some(inline) = m.get("inlineData").or_else(|| m.get("inline_data")) {
                        if let Some(data) = inline.get("data").and_then(|d| d.as_str()) {
                            if !data.is_empty() {
                                return Some(data.to_string());
                            }
                        }
                    }
                }
            }
            // dataUris / data_uris (may contain data: URLs)
            for key in ["dataUris", "data_uris"] {
                if let Some(arr) = obj.get(key).and_then(|a| a.as_array()) {
                    for s in arr {
                        if let Some(u) = s.as_str() {
                            if !u.is_empty() {
                                return Some(u.to_string());
                            }
                        }
                    }
                }
            }
            // fileData.fileUri that is already a data URI
            for key in ["fileData", "file_data"] {
                if let Some(fd) = obj.get(key) {
                    if let Some(uri) = fd
                        .get("fileUri")
                        .or_else(|| fd.get("file_uri"))
                        .and_then(|u| u.as_str())
                    {
                        if uri.starts_with("data:") {
                            return Some(uri.to_string());
                        }
                    }
                }
            }
        }
        // Recurse into arrays and objects
        match v {
            serde_json::Value::Array(arr) => {
                for item in arr {
                    if let Some(s) = find_image_data(item) {
                        return Some(s);
                    }
                }
                None
            }
            serde_json::Value::Object(map) => {
                for (_k, val) in map.iter() {
                    if let Some(s) = find_image_data(val) {
                        return Some(s);
                    }
                }
                None
            }
            _ => None,
        }
    }
    let api_key = settings
        .gemini_api_key
        .clone()
        .or_else(|| std::env::var("GEMINI_API_KEY").ok())
        .context("Gemini API key not set")?;
    
    let model_id = "gemini-2.5-flash-image-preview";
    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/{}:streamGenerateContent",
        model_id
    );
    
    let body = serde_json::json!({
        "contents": [
            {
                "role": "user",
                "parts": [ { "text": prompt } ]
            }
        ],
        "generationConfig": {
            "responseModalities": ["IMAGE", "TEXT"]
        }
    });
    
    let client = reqwest::Client::new();
    let resp = client
        .post(url)
        .header("X-goog-api-key", api_key)
        .json(&body)
        .send()
        .await
        .context("gemini image request failed")?;
    
    if !resp.status().is_success() {
        return Err(anyhow!("gemini image error: HTTP {}", resp.status()));
    }

    // Streamed NDJSON; collect last seen inlineData.data
    let mut latest_b64: Option<String> = None;
    let mut progress: u32 = 1;
    let total: u32 = 100;
    on_progress(progress, total);
    
    let mut buf = String::new();
    let mut stream = resp.bytes_stream();
    
    while let Some(chunk) = stream.next().await {
        let bytes = chunk.map_err(|e| anyhow!("gemini stream error: {}", e))?;
        let s = String::from_utf8_lossy(&bytes);
        buf.push_str(&s);
        let mut start = 0usize;
        for (i, ch) in buf.char_indices() {
            if ch == '\n' {
                let mut line = &buf[start..i];
                if !line.trim().is_empty() {
                    // Some servers prefix with "data: " like SSE
                    if let Some(stripped) = line.strip_prefix("data: ") {
                        line = stripped;
                    }
                    
                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(line) {
                        if let Some(s) = find_image_data(&json) {
                            latest_b64 = Some(s);
                        }
                    }
                }
                start = i + 1;
                
                // Nudge progress for each processed line
                if progress < 98 { 
                    progress = progress.saturating_add(2); 
                    on_progress(progress, total); 
                }
            }
        }
        
        if start > 0 { 
            buf = buf[start..].to_string(); 
        }
    }
    
    // Finalize progress
    on_progress(99, total);
    let out = latest_b64.ok_or_else(|| anyhow!("gemini stream: no image data received"))?;
    on_progress(100, total);
    Ok(out)
}

pub async fn generate_image_once(prompt: &str, settings: &Settings) -> Result<String> {
    let api_key = settings
        .gemini_api_key
        .clone()
        .or_else(|| std::env::var("GEMINI_API_KEY").ok())
        .context("Gemini API key not set")?;
    
    let model_id = "gemini-2.5-flash-image-preview";
    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent",
        model_id
    );
    
    let body = serde_json::json!({
        "contents": [
            {
                "role": "user",
                "parts": [ { "text": prompt } ]
            }
        ],
        "generationConfig": {
            "responseModalities": ["IMAGE", "TEXT"]
        }
    });
    
    let client = reqwest::Client::new();
    let resp = client
        .post(url)
        .header("X-goog-api-key", api_key)
        .json(&body)
        .send()
        .await
        .context("gemini image request failed")?;
    
    if !resp.status().is_success() {
        return Err(anyhow!("gemini image error: HTTP {}", resp.status()));
    }
    
    let value: serde_json::Value = resp.json().await
        .context("gemini image parse error")?;

    // Reuse the same extractor as streaming path
    fn find_image_data(v: &serde_json::Value) -> Option<String> {
        if let Some(obj) = v.as_object() {
            for key in ["inlineData", "inline_data"] {
                if let Some(inline) = obj.get(key) {
                    if let Some(data) = inline.get("data").and_then(|d| d.as_str()) {
                        if !data.is_empty() {
                            return Some(data.to_string());
                        }
                    }
                }
            }
            if let Some(media) = obj.get("media").and_then(|m| m.as_array()) {
                for m in media {
                    if let Some(inline) = m.get("inlineData").or_else(|| m.get("inline_data")) {
                        if let Some(data) = inline.get("data").and_then(|d| d.as_str()) {
                            if !data.is_empty() {
                                return Some(data.to_string());
                            }
                        }
                    }
                }
            }
            for key in ["dataUris", "data_uris"] {
                if let Some(arr) = obj.get(key).and_then(|a| a.as_array()) {
                    for s in arr {
                        if let Some(u) = s.as_str() {
                            if !u.is_empty() {
                                return Some(u.to_string());
                            }
                        }
                    }
                }
            }
            for key in ["fileData", "file_data"] {
                if let Some(fd) = obj.get(key) {
                    if let Some(uri) = fd
                        .get("fileUri")
                        .or_else(|| fd.get("file_uri"))
                        .and_then(|u| u.as_str())
                    {
                        if uri.starts_with("data:") {
                            return Some(uri.to_string());
                        }
                    }
                }
            }
        }
        match v {
            serde_json::Value::Array(arr) => {
                for item in arr {
                    if let Some(s) = find_image_data(item) {
                        return Some(s);
                    }
                }
                None
            }
            serde_json::Value::Object(map) => {
                for (_k, val) in map.iter() {
                    if let Some(s) = find_image_data(val) {
                        return Some(s);
                    }
                }
                None
            }
            _ => None,
        }
    }

    if let Some(s) = find_image_data(&value) {
        return Ok(s);
    }

    Err(anyhow!("gemini image: no inline image data in response"))
}

pub async fn generate_image_with_progress(
    prompt: &str,
    settings: &Settings,
    on_progress: impl FnMut(u32, u32),
) -> Result<String, String> {
    match generate_image_stream_progress(prompt, settings, on_progress).await {
        Ok(b64) => Ok(b64),
        Err(_) => generate_image_once(prompt, settings)
            .await
            .map_err(|e| format!("gemini image failed: {}", e)),
    }
}

// Nano-Banana integration
pub async fn nano_banana_generate_image(
    storyboard_text: &str,
    settings: &Settings,
) -> Result<String, String> {
    let base = settings
        .nano_banana_base_url
        .as_ref()
        .ok_or_else(|| "nano-banana base URL not set in settings".to_string())?;
    
    let url = format!("{}/generate", base.trim_end_matches('/'));
    let client = reqwest::Client::new();
    
    let mut req = client.post(url).json(&serde_json::json!({
        "storyboard": storyboard_text,
    }));
    
    if let Some(key) = &settings.nano_banana_api_key {
        req = req.header("X-API-Key", key);
    }
    
    let resp = req.send().await
        .map_err(|e| format!("nano-banana request failed: {e}"))?;
    
    if !resp.status().is_success() {
        return Err(format!("nano-banana error: HTTP {}", resp.status()));
    }
    
    let value: serde_json::Value = resp.json().await
        .map_err(|e| format!("nano-banana parse error: {e}"))?;
    
    if let Some(s) = value.get("image_base64").and_then(|v| v.as_str()) {
        return Ok(s.to_string());
    }
    
    if let Some(s) = value.get("image").and_then(|v| v.as_str()) {
        return Ok(s.to_string());
    }
    
    Err("nano-banana: no image in response".to_string())
}