use anyhow::{anyhow, Context, Result};
use futures_util::StreamExt;
use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use std::fs;
use std::path::Path;
use std::time::Duration;

use crate::settings::Settings;
use tracing::{info, error, instrument};



#[instrument(skip(settings, on_progress), fields(model = "gemini-2.5-flash-image-preview"))]
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
    
    // Build parts: prompt text + optional avatar image and description
    let mut parts: Vec<serde_json::Value> = vec![serde_json::json!({ "text": build_prompt_with_avatar_text(prompt, settings) })];
    if let Some(img_part) = try_build_avatar_image_part(settings) {
        parts.push(img_part);
    }

    let body = serde_json::json!({
        "contents": [
            {
                "role": "user",
                "parts": parts
            }
        ],
        "generationConfig": {
            "responseModalities": ["IMAGE", "TEXT"]
        }
    });
    
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(90))
        .connect_timeout(Duration::from_secs(10))
        .build()?;
    let resp = client
        .post(url)
        .header("X-goog-api-key", api_key)
        .json(&body)
        .send()
        .await
        .context("gemini image request failed")?;
    
    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_else(|_| "<no body>".into());
        error!(http = %status, body = %text, "gemini image error (stream)");
        return Err(anyhow!("gemini image error: HTTP {} - {}", status, text));
    }

    // Streamed NDJSON; collect last seen inlineData.data or HTTP file URI
    let mut latest_b64: Option<String> = None;
    let mut latest_http_uri: Option<String> = None;
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
                        // Try to capture http(s) URIs as a fallback
                        fn find_http_uri(v: &serde_json::Value) -> Option<String> {
                            if let Some(obj) = v.as_object() {
                                for key in ["fileData", "file_data"] {
                                    if let Some(fd) = obj.get(key) {
                                        if let Some(uri) = fd
                                            .get("fileUri")
                                            .or_else(|| fd.get("file_uri"))
                                            .and_then(|u| u.as_str())
                                        {
                                            if uri.starts_with("http://") || uri.starts_with("https://") {
                                                return Some(uri.to_string());
                                            }
                                        }
                                    }
                                }
                                for key in ["dataUris", "data_uris"] {
                                    if let Some(arr) = obj.get(key).and_then(|a| a.as_array()) {
                                        for s in arr {
                                            if let Some(u) = s.as_str() {
                                                if u.starts_with("http://") || u.starts_with("https://") {
                                                    return Some(u.to_string());
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                            match v {
                                serde_json::Value::Array(arr) => {
                                    for item in arr {
                                        if let Some(u) = find_http_uri(item) {
                                            return Some(u);
                                        }
                                    }
                                    None
                                }
                                serde_json::Value::Object(map) => {
                                    for (_k, val) in map.iter() {
                                        if let Some(u) = find_http_uri(val) {
                                            return Some(u);
                                        }
                                    }
                                    None
                                }
                                _ => None,
                            }
                        }
                        if latest_http_uri.is_none() {
                            latest_http_uri = find_http_uri(&json);
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
    let out = if let Some(b64) = latest_b64 {
        b64
    } else if let Some(uri) = latest_http_uri {
        // Best-effort fetch of file URI
        let bytes = client.get(uri.clone()).send().await
            .map_err(|e| anyhow!("gemini stream: fetch uri failed: {}", e))?
            .bytes().await
            .map_err(|e| anyhow!("gemini stream: read uri bytes failed: {}", e))?;
        B64.encode(bytes)
    } else {
        return Err(anyhow!("gemini stream: no image data received"));
    };
    on_progress(100, total);
    info!("gemini streaming image generation completed");
    Ok(out)
}

#[instrument(skip(settings), fields(model = "gemini-2.5-flash-image-preview"))]
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
    
    // Build parts: prompt text + optional avatar image and description
    let mut parts: Vec<serde_json::Value> = vec![serde_json::json!({ "text": build_prompt_with_avatar_text(prompt, settings) })];
    if let Some(img_part) = try_build_avatar_image_part(settings) {
        parts.push(img_part);
    }

    let body = serde_json::json!({
        "contents": [
            {
                "role": "user",
                "parts": parts
            }
        ],
        "generationConfig": {
            "responseModalities": ["IMAGE", "TEXT"]
        }
    });
    
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .connect_timeout(Duration::from_secs(10))
        .build()?;
    let resp = client
        .post(url)
        .header("X-goog-api-key", api_key)
        .json(&body)
        .send()
        .await
        .context("gemini image request failed")?;
    
    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_else(|_| "<no body>".into());
        error!(http = %status, body = %text, "gemini image error (once)");
        return Err(anyhow!("gemini image error: HTTP {} - {}", status, text));
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
        info!("gemini non-streaming image generation completed");
        return Ok(s);
    }
    // Try to locate an HTTP file URI and fetch it
    fn find_http_uri(v: &serde_json::Value) -> Option<String> {
        if let Some(obj) = v.as_object() {
            for key in ["fileData", "file_data"] {
                if let Some(fd) = obj.get(key) {
                    if let Some(uri) = fd
                        .get("fileUri")
                        .or_else(|| fd.get("file_uri"))
                        .and_then(|u| u.as_str())
                    {
                        if uri.starts_with("http://") || uri.starts_with("https://") {
                            return Some(uri.to_string());
                        }
                    }
                }
            }
            for key in ["dataUris", "data_uris"] {
                if let Some(arr) = obj.get(key).and_then(|a| a.as_array()) {
                    for s in arr {
                        if let Some(u) = s.as_str() {
                            if u.starts_with("http://") || u.starts_with("https://") {
                                return Some(u.to_string());
                            }
                        }
                    }
                }
            }
        }
        match v {
            serde_json::Value::Array(arr) => {
                for item in arr {
                    if let Some(u) = find_http_uri(item) {
                        return Some(u);
                    }
                }
                None
            }
            serde_json::Value::Object(map) => {
                for (_k, val) in map.iter() {
                    if let Some(u) = find_http_uri(val) {
                        return Some(u);
                    }
                }
                None
            }
            _ => None,
        }
    }
    if let Some(uri) = find_http_uri(&value) {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .connect_timeout(Duration::from_secs(10))
            .build()?;
        let bytes = client.get(uri.clone()).send().await
            .map_err(|e| anyhow!("gemini once: fetch uri failed: {}", e))?
            .bytes().await
            .map_err(|e| anyhow!("gemini once: read uri bytes failed: {}", e))?;
        info!("gemini non-streaming image fetched via file URI");
        return Ok(B64.encode(bytes));
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

fn build_prompt_with_avatar_text(prompt: &str, settings: &Settings) -> String {
    let mut out = String::new();
    out.push_str(prompt);
    if let Some(desc) = settings.avatar_description.as_ref().filter(|s| !s.trim().is_empty()) {
        out.push_str("\n\nCharacter consistency: The protagonist must match this description consistently across images.\n");
        out.push_str(desc);
    }
    out
}

pub fn build_avatar_image_prompt(description: &str) -> String {
    format!(r#"Task: Render a single character portrait avatar image.

Framing & Style Guidelines:
- Waist-up framing, clean/neutral background, neutral lighting.
- Keep character consistent across future images.
- Avoid text, watermarks, UI elements.
- Illustration vibe: cohesive, appealing, readable at small sizes.

Output: One portrait image.

Character Description:
{}"#, description)
}

fn try_build_avatar_image_part(settings: &Settings) -> Option<serde_json::Value> {
    let path = settings.avatar_image_path.as_ref()?;
    let p = Path::new(path);
    let bytes = fs::read(p).ok()?;
    let b64 = B64.encode(bytes);
    let mime = match p.extension().and_then(|e| e.to_str()).map(|s| s.to_ascii_lowercase()) {
        Some(ext) if ext == "jpg" || ext == "jpeg" => "image/jpeg",
        Some(ext) if ext == "webp" => "image/webp",
        _ => "image/png",
    };
    Some(serde_json::json!({
        "inlineData": { "mimeType": mime, "data": b64 }
    }))
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
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .connect_timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| format!("http client error: {e}"))?;
    
    // Inject avatar guidance into storyboard text so downstream renderer can try to respect it
    let mut storyboard_plus = storyboard_text.to_string();
    if let Some(desc) = settings.avatar_description.as_ref().filter(|s| !s.trim().is_empty()) {
        storyboard_plus.push_str("\n\nCharacter consistency: The protagonist must match this description consistently across panels.\n");
        storyboard_plus.push_str(desc);
    }

    let mut req = client.post(url).json(&serde_json::json!({
        "storyboard": storyboard_plus,
    }));
    
    if let Some(key) = &settings.nano_banana_api_key {
        req = req.header("X-API-Key", key);
    }
    
    let resp = req.send().await
        .map_err(|e| format!("nano-banana request failed: {e}"))?;
    
    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_else(|_| "<no body>".into());
        return Err(format!("nano-banana error: HTTP {} - {}", status, text));
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