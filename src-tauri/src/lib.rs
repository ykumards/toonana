use anyhow::{anyhow, Context, Result};
use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use dashmap::DashMap;
use directories::ProjectDirs;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use sqlx::{sqlite::{SqlitePoolOptions, SqliteConnectOptions}, Pool, Sqlite, Row};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use time::OffsetDateTime;
use tokio::task::JoinHandle;
use uuid::Uuid;
use reqwest::StatusCode;
use futures_util::StreamExt;

// kept for potential future re-enable of encryption
#[allow(dead_code)]
static SERVICE_NAME: &str = "toonana";
#[allow(dead_code)]
static VAULT_KEY_LABEL: &str = "vault-key-v1";

#[derive(Clone)]
struct AppState {
    db: Pool<Sqlite>,
    data_dir: PathBuf,
    jobs: Arc<DashMap<String, JoinHandle<()>>>,
    comic_status: Arc<DashMap<String, ComicJobStatus>>, // job_id -> status
}

#[derive(Debug, Serialize, Deserialize)]
struct AppHealth {
    ok: bool,
    data_dir: String,
    db_path: String,
    has_vault_key: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct Settings {
    gemini_api_key: Option<String>,
    ollama_base_url: Option<String>,
    default_ollama_model: Option<String>,
    ollama_temperature: Option<f32>,
    ollama_top_p: Option<f32>,
    nano_banana_base_url: Option<String>,
    nano_banana_api_key: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct EntryUpsert {
    id: Option<String>,
    title: String,
    body_cipher: Vec<u8>,
    mood: Option<String>,
    tags: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Entry {
    id: String,
    created_at: String,
    updated_at: String,
    title: String,
    body_cipher: Vec<u8>,
    mood: Option<String>,
    tags: Option<serde_json::Value>,
    embedding: Option<Vec<u8>>,
}

#[derive(Debug, Serialize, Deserialize)]
struct EntryListItem {
    id: String,
    created_at: String,
    updated_at: String,
    title: String,
    mood: Option<String>,
    tags: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ListParams {
    limit: Option<i64>,
    offset: Option<i64>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ExportPanel {
    panel_id: String,
    image_path: Option<String>,
    dialogue_cipher: Option<Vec<u8>>,
}

#[derive(Debug, Serialize, Deserialize)]
struct OllamaGenerateRequest {
    model: String,
    prompt: String,
    stream: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct OllamaGenerateResponse {
    response: String,
}

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

async fn gemini_generate(prompt: &str) -> Result<String> {
    let state_ref = STARTUP.as_ref().map_err(|_| anyhow!("startup not ready"))?;
    let settings = load_settings_from_dir(&state_ref.data_dir);
    let api_key = settings
        .gemini_api_key
        .or_else(|| std::env::var("GEMINI_API_KEY").ok())
        .context("Gemini API key not set")?;
    let url = "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.0-flash:generateContent";
    let body = GeminiRequestBody {
        contents: vec![GeminiContentRequest {
            parts: vec![GeminiPartsRequestText { text: prompt.to_string() }],
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
    let value: GeminiResponseBody = resp.json().await.context("gemini parse error")?;
    if let Some(cands) = value.candidates {
        for cand in cands {
            if let Some(content) = cand.content {
                if let Some(parts) = content.parts {
                    for p in parts {
                        if let Some(t) = p.text {
                            if !t.is_empty() { return Ok(t); }
                        }
                    }
                }
            }
        }
    }
    Err(anyhow!("gemini: no text in response"))
}

async fn gemini_generate_image_stream_progress(
    prompt: &str,
    mut on_progress: impl FnMut(u32, u32),
) -> Result<String> {
    let state_ref = STARTUP.as_ref().map_err(|_| anyhow!("startup not ready"))?;
    let settings = load_settings_from_dir(&state_ref.data_dir);
    let api_key = settings
        .gemini_api_key
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
    let mut progress: u32 = 1; // start at 1 for a visible tick
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
                let line = &buf[start..i];
                if !line.trim().is_empty() {
                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(line) {
                        // Try common structures
                        // 1) top-level candidates[].content.parts[].inlineData.data
                        if let Some(cands) = json.get("candidates").and_then(|v| v.as_array()) {
                            for cand in cands {
                                if let Some(parts) = cand
                                    .get("content")
                                    .and_then(|c| c.get("parts"))
                                    .and_then(|p| p.as_array())
                                {
                                    for p in parts {
                                        if let Some(inline) = p.get("inlineData").or_else(|| p.get("inline_data")) {
                                            if let Some(data) = inline.get("data").and_then(|d| d.as_str()) {
                                                latest_b64 = Some(data.to_string());
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        // 2) sometimes the chunk is simply a part
                        if latest_b64.is_none() {
                            if let Some(inline) = json.get("inlineData").or_else(|| json.get("inline_data")) {
                                if let Some(data) = inline.get("data").and_then(|d| d.as_str()) {
                                    latest_b64 = Some(data.to_string());
                                }
                            }
                        }
                    }
                }
                start = i + 1;
                // Nudge progress for each processed line; cap below 98
                if progress < 98 { progress = progress.saturating_add(2); on_progress(progress, total); }
            }
        }
        if start > 0 { buf = buf[start..].to_string(); }
    }
    // Finalize progress
    on_progress(99, total);
    let out = latest_b64.ok_or_else(|| anyhow!("gemini stream: no image data received"))?;
    on_progress(100, total);
    Ok(out)
}

#[derive(Debug, Serialize, Deserialize)]
struct OllamaTagsModel {
    name: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct OllamaTagsResponse {
    models: Option<Vec<OllamaTagsModel>>,
}

#[derive(Debug, Serialize, Deserialize)]
struct OllamaHealth {
    ok: bool,
    message: Option<String>,
    models: Option<Vec<String>>,
}

#[tauri::command]
async fn ollama_health(state: tauri::State<'_, AppState>) -> Result<OllamaHealth, String> {
    let settings = load_settings_from_dir(&state.data_dir);
    let base = settings.ollama_base_url.unwrap_or_else(|| "http://127.0.0.1:11434".to_string());
    let client = reqwest::Client::new();
    let url = format!("{}/api/tags", base);
    let resp = client.get(url).send().await;
    match resp {
        Ok(r) if r.status().is_success() => {
            let tags: OllamaTagsResponse = r.json().await.map_err(|e| e.to_string())?;
            let models = tags.models.unwrap_or_default().into_iter().filter_map(|m| m.name).collect::<Vec<_>>();
            Ok(OllamaHealth { ok: true, message: None, models: Some(models) })
        }
        Ok(r) => Ok(OllamaHealth { ok: false, message: Some(format!("HTTP {}", r.status())), models: None }),
        Err(e) => Ok(OllamaHealth { ok: false, message: Some(e.to_string()), models: None }),
    }
}

#[tauri::command]
async fn ollama_list_models(state: tauri::State<'_, AppState>) -> Result<Vec<String>, String> {
    let health = ollama_health(state).await?;
    Ok(health.models.unwrap_or_default())
}

#[tauri::command]
async fn ollama_generate(model: Option<String>, prompt: String) -> Result<String, String> {
    let state = STARTUP.as_ref().map_err(|e| e.to_string())?.clone();
    let settings = load_settings_from_dir(&state.data_dir);
    let base = settings.ollama_base_url.unwrap_or_else(|| "http://127.0.0.1:11434".to_string());
    let model_name = model.or(settings.default_ollama_model).unwrap_or_else(|| "gemma3:1b".to_string());
    let body = OllamaGenerateRequest { model: model_name, prompt, stream: false };
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
    let value: serde_json::Value = resp.json().await.map_err(|e| format!("response parse error: {e}"))?;
    if let Some(s) = value.get("response").and_then(|v| v.as_str()) {
        return Ok(s.to_string());
    }
    // Some servers may return multiple JSON lines even if stream=false; handle array of chunks
    if let Some(arr) = value.as_array() {
        let mut out = String::new();
        for v in arr {
            if let Some(s) = v.get("response").and_then(|x| x.as_str()) {
                out.push_str(s);
            }
        }
        if !out.is_empty() { return Ok(out); }
    }
    Err("Unexpected Ollama response format".to_string())
}

async fn nano_banana_generate_image(storyboard_text: &str) -> Result<String, String> {
    let state = STARTUP.as_ref().map_err(|e| e.to_string())?.clone();
    let settings = load_settings_from_dir(&state.data_dir);
    let base = settings
        .nano_banana_base_url
        .ok_or_else(|| "nano-banana base URL not set in settings".to_string())?;
    let url = format!("{}/generate", base.trim_end_matches('/'));
    let client = reqwest::Client::new();
    let mut req = client.post(url).json(&serde_json::json!({
        "storyboard": storyboard_text,
    }));
    if let Some(key) = settings.nano_banana_api_key {
        req = req.header("X-API-Key", key);
    }
    let resp = req.send().await.map_err(|e| format!("nano-banana request failed: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("nano-banana error: HTTP {}", resp.status()));
    }
    let value: serde_json::Value = resp.json().await.map_err(|e| format!("nano-banana parse error: {e}"))?;
    if let Some(s) = value.get("image_base64").and_then(|v| v.as_str()) {
        return Ok(s.to_string());
    }
    if let Some(s) = value.get("image").and_then(|v| v.as_str()) {
        return Ok(s.to_string());
    }
    Err("nano-banana: no image in response".to_string())
}

async fn ollama_generate_streaming(
    model: Option<String>,
    prompt: String,
    mut on_chunk: impl FnMut(&str),
) -> Result<(), String> {
    let state = STARTUP.as_ref().map_err(|e| e.to_string())?.clone();
    let settings = load_settings_from_dir(&state.data_dir);
    let base = settings
        .ollama_base_url
        .unwrap_or_else(|| "http://127.0.0.1:11434".to_string());
    let model_name = model
        .or(settings.default_ollama_model)
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

type JobId = String;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "stage", rename_all = "snake_case")]
enum ComicStage {
    Queued,
    Parsing,
    Storyboarding,
    Prompting,
    Rendering { completed: u32, total: u32 },
    Saving,
    Done,
    Failed { error: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ComicJobStatus {
    job_id: String,
    entry_id: String,
    style: String,
    stage: ComicStage,
    updated_at: String,
    result_image_path: Option<String>,
    storyboard_text: Option<String>,
}

fn now_iso() -> String {
    OffsetDateTime::now_utc().format(&time::format_description::well_known::Rfc3339).unwrap_or_default()
}

fn app_dirs() -> Result<ProjectDirs> {
    ProjectDirs::from("app", "toonana", "toonana").ok_or_else(|| anyhow!("cannot resolve project dirs"))
}

fn ensure_data_dir() -> Result<PathBuf> {
    let dirs = app_dirs()?;
    let data_dir = dirs.data_dir().to_path_buf();
    fs::create_dir_all(&data_dir).context("create data dir")?;
    Ok(data_dir)
}

fn db_path(data_dir: &Path) -> PathBuf {
    data_dir.join("app.sqlite")
}

fn settings_path(data_dir: &Path) -> PathBuf {
    data_dir.join("settings.json")
}

fn load_settings_from_dir(data_dir: &Path) -> Settings {
    let path = settings_path(data_dir);
    if let Ok(bytes) = fs::read(&path) {
        if let Ok(s) = serde_json::from_slice::<Settings>(&bytes) {
            return s;
        }
    }
    Settings::default()
}

fn save_settings_to_dir(data_dir: &Path, s: &Settings) -> Result<()> {
    let path = settings_path(data_dir);
    let json = serde_json::to_vec_pretty(s)?;
    fs::write(path, json).context("write settings")?;
    Ok(())
}

async fn init_db(pool: &Pool<Sqlite>) -> Result<()> {
    // Minimal schema per spec
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS entries (
            id TEXT PRIMARY KEY,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            title TEXT NOT NULL,
            body_cipher BLOB NOT NULL,
            mood TEXT,
            tags TEXT,
            embedding BLOB
        );
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS storyboards (
            id TEXT PRIMARY KEY,
            entry_id TEXT NOT NULL,
            json_cipher BLOB NOT NULL,
            model TEXT NOT NULL,
            created_at TEXT NOT NULL
        );
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS panels (
            id TEXT PRIMARY KEY,
            entry_id TEXT NOT NULL,
            idx INTEGER NOT NULL,
            prompt_cipher BLOB,
            dialogue_cipher BLOB,
            seed INTEGER,
            cfg REAL,
            style TEXT,
            image_path TEXT,
            meta TEXT
        );
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS assets (
            id TEXT PRIMARY KEY,
            kind TEXT NOT NULL,
            path TEXT NOT NULL,
            meta TEXT
        );
        "#,
    )
    .execute(pool)
    .await?;

    Ok(())
}

// Note: Encryption disabled per user preference; store plaintext bytes on-device only.

#[tauri::command]
async fn health(state: tauri::State<'_, AppState>) -> Result<AppHealth, String> {
    Ok(AppHealth {
        ok: true,
        data_dir: state.data_dir.display().to_string(),
        db_path: db_path(&state.data_dir).display().to_string(),
        has_vault_key: true,
    })
}

#[tauri::command]
async fn get_settings(state: tauri::State<'_, AppState>) -> Result<Settings, String> {
    Ok(load_settings_from_dir(&state.data_dir))
}

#[tauri::command]
async fn update_settings(state: tauri::State<'_, AppState>, settings: Settings) -> Result<Settings, String> {
    save_settings_to_dir(&state.data_dir, &settings).map_err(|e| e.to_string())?;
    Ok(settings)
}

#[tauri::command]
fn init_vault() -> Result<(), String> {
    Ok(())
}

#[tauri::command]
fn encrypt(plaintext: String) -> Result<Vec<u8>, String> {
    Ok(plaintext.into_bytes())
}

#[tauri::command]
fn decrypt(cipher: Vec<u8>) -> Result<String, String> {
    String::from_utf8(cipher).map_err(|e| e.to_string())
}

#[tauri::command]
async fn db_upsert_entry(state: tauri::State<'_, AppState>, entry: EntryUpsert) -> Result<Entry, String> {
    let id = entry.id.unwrap_or_else(|| Uuid::new_v4().to_string());
    let now = now_iso();
    let tags_json = entry.tags.map(|t| t.to_string());

    let _ = sqlx::query(
        r#"
        INSERT INTO entries (id, created_at, updated_at, title, body_cipher, mood, tags, embedding)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, NULL)
        ON CONFLICT(id) DO UPDATE SET
          updated_at=excluded.updated_at,
          title=excluded.title,
          body_cipher=excluded.body_cipher,
          mood=excluded.mood,
          tags=excluded.tags
        "#,
    )
    .bind(&id)
    .bind(&now)
    .bind(&now)
    .bind(&entry.title)
    .bind(&entry.body_cipher)
    .bind(&entry.mood)
    .bind(&tags_json)
    .execute(&state.db)
    .await
    .map_err(|e| e.to_string())?;

    let row = sqlx::query(
        r#"SELECT id, created_at, updated_at, title, body_cipher, mood, tags, embedding FROM entries WHERE id = ?1"#
    )
    .bind(&id)
    .fetch_one(&state.db)
    .await
    .map_err(|e| e.to_string())?;

    let tags_str: Option<String> = row.try_get("tags").map_err(|e| e.to_string())?;
    let tags_val = tags_str
        .as_deref()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok());

    Ok(Entry {
        id: row.try_get("id").map_err(|e| e.to_string())?,
        created_at: row.try_get("created_at").map_err(|e| e.to_string())?,
        updated_at: row.try_get("updated_at").map_err(|e| e.to_string())?,
        title: row.try_get("title").map_err(|e| e.to_string())?,
        body_cipher: row.try_get("body_cipher").map_err(|e| e.to_string())?,
        mood: row.try_get("mood").map_err(|e| e.to_string())?,
        tags: tags_val,
        embedding: row.try_get("embedding").ok(),
    })
}

#[tauri::command]
async fn db_get_entry(state: tauri::State<'_, AppState>, id: String) -> Result<Entry, String> {
    let row = sqlx::query(
        r#"SELECT id, created_at, updated_at, title, body_cipher, mood, tags, embedding FROM entries WHERE id = ?1"#
    )
    .bind(&id)
    .fetch_one(&state.db)
    .await
    .map_err(|e| e.to_string())?;
    let tags_str: Option<String> = row.try_get("tags").map_err(|e| e.to_string())?;
    let tags_val = tags_str
        .as_deref()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok());
    Ok(Entry {
        id: row.try_get("id").map_err(|e| e.to_string())?,
        created_at: row.try_get("created_at").map_err(|e| e.to_string())?,
        updated_at: row.try_get("updated_at").map_err(|e| e.to_string())?,
        title: row.try_get("title").map_err(|e| e.to_string())?,
        body_cipher: row.try_get("body_cipher").map_err(|e| e.to_string())?,
        mood: row.try_get("mood").map_err(|e| e.to_string())?,
        tags: tags_val,
        embedding: row.try_get("embedding").ok(),
    })
}

#[tauri::command]
async fn db_list_entries(state: tauri::State<'_, AppState>, p: Option<ListParams>) -> Result<Vec<EntryListItem>, String> {
    let limit = p.as_ref().and_then(|p| p.limit).unwrap_or(100);
    let offset = p.as_ref().and_then(|p| p.offset).unwrap_or(0);
    let rows = sqlx::query(
        r#"SELECT id, created_at, updated_at, title, mood, tags FROM entries ORDER BY created_at DESC LIMIT ?1 OFFSET ?2"#
    )
    .bind(limit)
    .bind(offset)
    .fetch_all(&state.db)
    .await
    .map_err(|e| e.to_string())?;
    let items = rows
        .into_iter()
        .map(|row| {
            let tags_str: Option<String> = row.try_get("tags").ok();
            let tags_val = tags_str
                .as_deref()
                .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok());
            EntryListItem {
                id: row.try_get("id").unwrap_or_default(),
                created_at: row.try_get("created_at").unwrap_or_default(),
                updated_at: row.try_get("updated_at").unwrap_or_default(),
                title: row.try_get("title").unwrap_or_default(),
                mood: row.try_get("mood").ok(),
                tags: tags_val,
            }
        })
        .collect();
    Ok(items)
}

fn decode_base64_png(s: &str) -> Result<Vec<u8>> {
    let data = if let Some(idx) = s.find(",") {
        &s[(idx + 1)..]
    } else {
        s
    };
    B64.decode(data).map_err(|e| anyhow!("base64 decode: {e}"))
}

#[tauri::command]
async fn save_image_to_disk(
    state: tauri::State<'_, AppState>,
    base64_png: String,
    entry_id: String,
    panel_id: String,
) -> Result<String, String> {
    let bytes = decode_base64_png(&base64_png).map_err(|e| e.to_string())?;
    let img_dir = state.data_dir.join("images").join(&entry_id);
    tokio::fs::create_dir_all(&img_dir)
        .await
        .map_err(|e| e.to_string())?;
    let file_path = img_dir.join(format!("{panel_id}.png"));
    tokio::fs::write(&file_path, bytes)
        .await
        .map_err(|e| e.to_string())?;
    Ok(file_path.display().to_string())
}

#[tauri::command]
async fn export_pdf(_state: tauri::State<'_, AppState>, _entry_id: String, _panels: Vec<ExportPanel>, path: String) -> Result<(), String> {
    // Placeholder: create an empty file so the UI can proceed; real export handled in FE via pdf-lib
    if let Some(parent) = Path::new(&path).parent() { let _ = fs::create_dir_all(parent); }
    fs::write(&path, b"PDF export handled in frontend").map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
async fn create_comic_job(state: tauri::State<'_, AppState>, entry_id: String, style: String) -> Result<JobId, String> {
    let job_id = Uuid::new_v4().to_string();
    state.comic_status.insert(job_id.clone(), ComicJobStatus {
        job_id: job_id.clone(),
        entry_id: entry_id.clone(),
        style: style.clone(),
        stage: ComicStage::Queued,
        updated_at: now_iso(),
        result_image_path: None,
        storyboard_text: None,
    });

    let status_map = state.comic_status.clone();
    let jid = job_id.clone();
    let eid = entry_id.clone();
    let st = style.clone();
    let db_pool = state.db.clone();
    let data_root = state.data_dir.clone();
    let handle = tokio::spawn(async move {
        // Step 1: Parse entry (no-op placeholder)
        status_map.insert(jid.clone(), ComicJobStatus {
            job_id: jid.clone(),
            entry_id: eid.clone(),
            style: st.clone(),
            stage: ComicStage::Parsing,
            updated_at: now_iso(),
            result_image_path: None,
            storyboard_text: None,
        });
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;

        // Step 2: Storyboard
        status_map.insert(jid.clone(), ComicJobStatus {
            job_id: jid.clone(),
            entry_id: eid.clone(),
            style: st.clone(),
            stage: ComicStage::Storyboarding,
            updated_at: now_iso(),
            result_image_path: None,
            storyboard_text: None,
        });
        // Load entry body for prompting
        let entry_body: Result<String> = async {
            let row = sqlx::query(
                r#"SELECT body_cipher FROM entries WHERE id = ?1"#
            )
            .bind(&eid)
            .fetch_one(&db_pool)
            .await
            .map_err(|e| anyhow!("db: {}", e))?;
            let cipher: Vec<u8> = row.try_get("body_cipher").map_err(|e| anyhow!("row: {}", e))?;
            let text = String::from_utf8(cipher).map_err(|e| anyhow!("utf8: {}", e))?;
            Ok::<_, anyhow::Error>(text)
        }.await;
        if let Err(e) = entry_body {
            status_map.insert(jid.clone(), ComicJobStatus {
                job_id: jid.clone(),
                entry_id: eid.clone(),
                style: st.clone(),
                stage: ComicStage::Failed { error: format!("load entry failed: {}", e) },
                updated_at: now_iso(),
                result_image_path: None,
                storyboard_text: None,
            });
            return;
        }
        let entry_text = entry_body.unwrap_or_default();

        // Step 3: Prompting (ask Ollama for storyboard; stream partials)
        status_map.insert(jid.clone(), ComicJobStatus {
            job_id: jid.clone(),
            entry_id: eid.clone(),
            style: st.clone(),
            stage: ComicStage::Prompting,
            updated_at: now_iso(),
            result_image_path: None,
            storyboard_text: None,
        });
        let ollama_prompt = format!(
            "You are a helpful assistant that writes short 4-6 panel comic storyboards from journal entries.\\nJournal Entry:\\n{}\\n\\nOutput format strictly as lines:\\nPanel 1\\nCaption: <short caption>\\nPanel 2\\nCharacter 1: <dialogue>\\n...\\nKeep each caption/dialogue under 12 words.",
            entry_text
        );

        let mut storyboard_text = String::new();
        let stream_res = ollama_generate_streaming(None, ollama_prompt, |chunk| {
            storyboard_text.push_str(chunk);
            // Update status with partial text
            status_map.insert(jid.clone(), ComicJobStatus {
                job_id: jid.clone(),
                entry_id: eid.clone(),
                style: st.clone(),
                stage: ComicStage::Prompting,
                updated_at: now_iso(),
                result_image_path: None,
                storyboard_text: Some(storyboard_text.clone()),
            });
        }).await;
        if let Err(e) = stream_res {
            status_map.insert(jid.clone(), ComicJobStatus {
                job_id: jid.clone(),
                entry_id: eid.clone(),
                style: st.clone(),
                stage: ComicStage::Failed { error: format!("ollama prompting failed: {}", e) },
                updated_at: now_iso(),
                result_image_path: None,
                storyboard_text: None,
            });
            return;
        }

        // Step 4: Rendering (call nano-banana to generate image)
        status_map.insert(jid.clone(), ComicJobStatus {
            job_id: jid.clone(),
            entry_id: eid.clone(),
            style: st.clone(),
            stage: ComicStage::Rendering { completed: 1, total: 1 },
            updated_at: now_iso(),
            result_image_path: None,
            storyboard_text: Some(storyboard_text.clone()),
        });

        let images_dir = data_root.join("images").join(&eid);
        let _ = tokio::fs::create_dir_all(&images_dir).await;
        let img_path = images_dir.join(format!("{}-result.png", &jid));

        let settings = load_settings_from_dir(&data_root);
        let nb_res = if settings.nano_banana_base_url.is_some() {
            nano_banana_generate_image(&storyboard_text).await
        } else {
            let mut last_tick = 0u32;
            gemini_generate_image_stream_progress(&storyboard_text, |completed, total| {
                // Avoid chatty updates; only on meaningful increments
                if completed > last_tick && completed % 5 == 0 {
                    last_tick = completed;
                    status_map.insert(jid.clone(), ComicJobStatus {
                        job_id: jid.clone(),
                        entry_id: eid.clone(),
                        style: st.clone(),
                        stage: ComicStage::Rendering { completed, total },
                        updated_at: now_iso(),
                        result_image_path: None,
                        storyboard_text: Some(storyboard_text.clone()),
                    });
                }
            }).await.map_err(|e| format!("gemini image failed: {}", e))
        };
        match nb_res {
            Ok(b64_png) => {
                match decode_base64_png(&b64_png) {
                    Ok(bytes) => {
                        let _ = tokio::fs::write(&img_path, bytes).await;
                        status_map.insert(jid.clone(), ComicJobStatus {
                            job_id: jid.clone(),
                            entry_id: eid.clone(),
                            style: st.clone(),
                            stage: ComicStage::Saving,
                            updated_at: now_iso(),
                            result_image_path: Some(img_path.display().to_string()),
                            storyboard_text: Some(storyboard_text.clone()),
                        });
                        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                        status_map.insert(jid.clone(), ComicJobStatus {
                            job_id: jid.clone(),
                            entry_id: eid.clone(),
                            style: st.clone(),
                            stage: ComicStage::Done,
                            updated_at: now_iso(),
                            result_image_path: Some(img_path.display().to_string()),
                            storyboard_text: Some(storyboard_text.clone()),
                        });
                    }
                    Err(e) => {
                        status_map.insert(jid.clone(), ComicJobStatus {
                            job_id: jid.clone(),
                            entry_id: eid.clone(),
                            style: st.clone(),
                            stage: ComicStage::Failed { error: format!("image decode failed: {}", e) },
                            updated_at: now_iso(),
                            result_image_path: None,
                            storyboard_text: Some(storyboard_text.clone()),
                        });
                    }
                }
            }
            Err(e) => {
                status_map.insert(jid.clone(), ComicJobStatus {
                    job_id: jid.clone(),
                    entry_id: eid.clone(),
                    style: st.clone(),
                    stage: ComicStage::Failed { error: format!("nano-banana failed: {}", e) },
                    updated_at: now_iso(),
                    result_image_path: None,
                    storyboard_text: Some(storyboard_text.clone()),
                });
            }
        }
    });
    state.jobs.insert(job_id.clone(), handle);
    Ok(job_id)
}

#[tauri::command]
async fn get_comic_job_status(state: tauri::State<'_, AppState>, job_id: String) -> Result<ComicJobStatus, String> {
    state
        .comic_status
        .get(&job_id)
        .map(|v| v.clone())
        .ok_or_else(|| "job not found".to_string())
}

#[tauri::command]
async fn cancel_job(state: tauri::State<'_, AppState>, job_id: String) -> Result<(), String> {
    if let Some((_, handle)) = state.jobs.remove(&job_id) {
        handle.abort();
    }
    Ok(())
}

static STARTUP: Lazy<Result<AppState>> = Lazy::new(|| {
    tauri_startup()
});

fn tauri_startup() -> Result<AppState> {
    let data_dir = ensure_data_dir()?;
    let db_file = db_path(&data_dir);
    // We need a synchronous runtime here to construct the pool; Tauri will use async in commands
    let rt = tokio::runtime::Runtime::new()?;
    let pool = rt.block_on(async {
        let opts = SqliteConnectOptions::new()
            .filename(&db_file)
            .create_if_missing(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect_with(opts)
            .await?;
        init_db(&pool).await?;
        Ok::<_, anyhow::Error>(pool)
    })?;

    Ok(AppState { db: pool, data_dir, jobs: Arc::new(DashMap::new()), comic_status: Arc::new(DashMap::new()) })
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let state = STARTUP.as_ref().expect("startup failed").clone();
    tauri::Builder::default()
        .manage(state)
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            health,
            get_settings,
            update_settings,
            init_vault,
            encrypt,
            decrypt,
            db_upsert_entry,
            db_get_entry,
            db_list_entries,
            save_image_to_disk,
            export_pdf,
            create_comic_job,
            get_comic_job_status,
            cancel_job,
            ollama_health,
            ollama_list_models,
            ollama_generate
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
