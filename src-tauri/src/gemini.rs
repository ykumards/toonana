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
        // Fallback: scan any string values for data:image/* URIs
        fn find_data_uri_in_any_string(v: &serde_json::Value) -> Option<String> {
            match v {
                serde_json::Value::String(s) => {
                    if s.starts_with("data:image/") { return Some(s.to_string()); }
                    None
                }
                serde_json::Value::Array(arr) => {
                    for item in arr { if let Some(u) = find_data_uri_in_any_string(item) { return Some(u); } }
                    None
                }
                serde_json::Value::Object(map) => {
                    for (_k, val) in map.iter() { if let Some(u) = find_data_uri_in_any_string(val) { return Some(u); } }
                    None
                }
                _ => None,
            }
        }
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
            // bytesBase64Encoded / b64_json (other providers sometimes use these)
            for key in ["bytesBase64Encoded", "b64_json"] {
                if let Some(s) = obj.get(key).and_then(|d| d.as_str()) {
                    if !s.is_empty() { return Some(s.to_string()); }
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
            // As a last resort, if any string field contains a data:image/* URI
            if let Some(uri) = find_data_uri_in_any_string(v) { return Some(uri); }
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
    let parts: Vec<serde_json::Value> = vec![serde_json::json!({ "text": build_prompt_with_avatar_text(prompt, settings) })];
    let avatar_part_included = false;
    // For avatar generation, avoid conditioning on the previously saved avatar image
    // so the model is free to produce a fresh portrait.

    let body = serde_json::json!({
        "contents": [
            {
                "role": "user",
                "parts": parts
            }
        ],
        "generationConfig": {
            "responseModalities": ["IMAGE"]
        }
    });
    
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(90))
        .connect_timeout(Duration::from_secs(10))
        .build()?;
    info!(prompt_len = prompt.len(), parts_len = parts.len(), avatar_part_included, "gemini(stream): sending request");
    let api_key_for_header = api_key.clone();
    let resp = client
        .post(url)
        .header("X-goog-api-key", api_key_for_header)
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
    let mut logged_inline_once = false;
    let mut logged_http_once = false;
    let mut progress: u32 = 1;
    let total: u32 = 100;
    on_progress(progress, total);
    
    let mut buf = String::new();
    let mut last_json_debug: Option<String> = None;
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
                        if last_json_debug.is_none() {
                            // store a truncated pretty sample for debugging
                            let s = serde_json::to_string(&json).unwrap_or_default();
                            let sample = if s.len() > 600 { format!("{}...", &s[..600]) } else { s };
                            last_json_debug = Some(sample);
                        }
                        if let Some(s) = find_image_data(&json) {
                            if !logged_inline_once {
                                info!(first_chunk_len = s.len(), "gemini(stream): found inline image data");
                                logged_inline_once = true;
                            }
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
                            if let Some(uri) = &latest_http_uri {
                                if !logged_http_once {
                                    info!(candidate_uri = %uri, "gemini(stream): found HTTP file URI candidate");
                                    logged_http_once = true;
                                }
                            }
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
        let mut req = client.get(uri.clone());
        if uri.contains("generativelanguage.googleapis.com") {
            req = req.header("X-goog-api-key", api_key.clone());
        }
        let bytes = req.send().await
            .map_err(|e| anyhow!("gemini stream: fetch uri failed: {}", e))?
            .bytes().await
            .map_err(|e| anyhow!("gemini stream: read uri bytes failed: {}", e))?;
        info!(fetched_bytes = bytes.len(), uri = %uri, "gemini(stream): fetched image via HTTP URI");
        B64.encode(bytes)
    } else {
        if let Some(sample) = last_json_debug.as_ref() {
            error!(sample = %sample, "gemini(stream): no image data received from stream (showing last JSON chunk)");
        } else {
            error!("gemini(stream): no image data received from stream");
        }
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
            "responseModalities": ["IMAGE"]
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
    // Log high-level structure for diagnostics
    if let Some(arr) = value.get("candidates").and_then(|c| c.as_array()) {
        let num_cand = arr.len();
        let mut parts_count = 0usize;
        if let Some(first) = arr.get(0) {
            if let Some(content) = first.get("content").and_then(|c| c.as_object()) {
                if let Some(parts) = content.get("parts").and_then(|p| p.as_array()) {
                    parts_count = parts.len();
                }
            }
        }
        info!(candidates = num_cand, first_parts = parts_count, "gemini(once): parsed response");
        // Deeper diagnostics on first part
        if let Some(first) = arr.get(0) {
            if let Some(content) = first.get("content").and_then(|c| c.as_object()) {
                if let Some(parts) = content.get("parts").and_then(|p| p.as_array()) {
                    if let Some(first_part) = parts.get(0) {
                        let keys: Vec<String> = match first_part.as_object() {
                            Some(map) => map.keys().cloned().collect(),
                            None => Vec::new(),
                        };
                        let has_inline = first_part.get("inlineData").is_some() || first_part.get("inline_data").is_some();
                        let has_media = first_part.get("media").is_some();
                        let has_data_uris = first_part.get("dataUris").is_some() || first_part.get("data_uris").is_some();
                        let has_file_data = first_part.get("fileData").is_some() || first_part.get("file_data").is_some();
                        let text_sample = first_part.get("text").and_then(|t| t.as_str()).map(|s| if s.len() > 200 { format!("{}...", &s[..200]) } else { s.to_string() });
                        info!(first_part_keys = ?keys, has_inline, has_media, has_data_uris, has_file_data, text_sample = ?text_sample, "gemini(once): first part diagnostics");
                    }
                }
            }
        }
    }

    // Reuse the same extractor as streaming path
    fn find_image_data(v: &serde_json::Value) -> Option<String> {
        fn find_data_uri_in_any_string(v: &serde_json::Value) -> Option<String> {
            match v {
                serde_json::Value::String(s) => {
                    if s.starts_with("data:image/") { return Some(s.to_string()); }
                    None
                }
                serde_json::Value::Array(arr) => {
                    for item in arr { if let Some(u) = find_data_uri_in_any_string(item) { return Some(u); } }
                    None
                }
                serde_json::Value::Object(map) => {
                    for (_k, val) in map.iter() { if let Some(u) = find_data_uri_in_any_string(val) { return Some(u); } }
                    None
                }
                _ => None,
            }
        }
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
            for key in ["bytesBase64Encoded", "b64_json"] {
                if let Some(s) = obj.get(key).and_then(|d| d.as_str()) {
                    if !s.is_empty() { return Some(s.to_string()); }
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
            if let Some(uri) = find_data_uri_in_any_string(v) { return Some(uri); }
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

    // Surface safety blocks more clearly
    if let Some(cands) = value.get("candidates").and_then(|c| c.as_array()) {
        if let Some(first) = cands.get(0) {
            if let Some(fr) = first.get("finishReason").and_then(|v| v.as_str()) {
                if fr.to_ascii_uppercase().contains("SAFETY") {
                    return Err(anyhow!("gemini image blocked by safety filters"));
                }
            }
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
        let mut req = client.get(uri.clone());
        if uri.contains("generativelanguage.googleapis.com") {
            // Some URIs require the same API key header to fetch
            if let Some(key) = settings
                .gemini_api_key
                .clone()
                .or_else(|| std::env::var("GEMINI_API_KEY").ok())
            { req = req.header("X-goog-api-key", key); }
        }
        let bytes = req.send().await
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
    format!(r#"Task: Render exactly one IMAGE of a single character portrait avatar.

Output Rules:
- Return IMAGE output only (no text content in the response).
- No acknowledgements, captions, watermarks, or UI elements.

Framing & Style:
- Waist-up framing, clean neutral background, neutral lighting.
- Keep the character identity consistent across future images.
- Illustration vibe; cohesive, appealing, readable at small sizes.

Deliverable:
- One portrait image.

Character Description:
{}"#, description)
}

// A stricter variant that strongly coerces IMAGE-only behavior
// Removed strict/fallback variant per simplified flow

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