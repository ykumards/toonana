use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use anyhow::{anyhow, Context, Result};
use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use dashmap::DashMap;
use directories::ProjectDirs;
use once_cell::sync::Lazy;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sqlx::{sqlite::{SqlitePoolOptions, SqliteConnectOptions}, Pool, Sqlite, Row};
use std::str::FromStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use time::OffsetDateTime;
use tokio::task::JoinHandle;
use uuid::Uuid;

static SERVICE_NAME: &str = "toonana";
static VAULT_KEY_LABEL: &str = "vault-key-v1";

#[derive(Clone)]
struct AppState {
    db: Pool<Sqlite>,
    data_dir: PathBuf,
    jobs: Arc<DashMap<String, JoinHandle<()>>>,
}

#[derive(Debug, Serialize, Deserialize)]
struct AppHealth {
    ok: bool,
    data_dir: String,
    db_path: String,
    has_vault_key: bool,
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

type JobId = String;

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

fn get_vault_key_bytes() -> Result<Vec<u8>> {
    let entry = keyring::Entry::new(SERVICE_NAME, VAULT_KEY_LABEL)?;
    let secret = entry.get_password().map_err(|_| anyhow!("vault not initialized"))?;
    let key = B64.decode(secret).context("decode vault key")?;
    if key.len() != 32 { return Err(anyhow!("invalid key length")); }
    Ok(key)
}

fn ensure_vault_key() -> Result<bool> {
    let entry = keyring::Entry::new(SERVICE_NAME, VAULT_KEY_LABEL)?;
    match entry.get_password() {
        Ok(_) => Ok(true),
        Err(_) => Ok(false),
    }
}

fn aes_encrypt(plaintext: &str) -> Result<Vec<u8>> {
    let key = get_vault_key_bytes()?;
    let cipher = Aes256Gcm::new_from_slice(&key).unwrap();
    let mut nonce_bytes = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ct = cipher
        .encrypt(nonce, plaintext.as_bytes())
        .map_err(|e| anyhow!("encrypt failed: {e}"))?;
    // Prepend nonce
    let mut out = nonce_bytes.to_vec();
    out.extend_from_slice(&ct);
    Ok(out)
}

fn aes_decrypt(ciphertext: &[u8]) -> Result<String> {
    if ciphertext.len() < 12 { return Err(anyhow!("ciphertext too short")); }
    let key = get_vault_key_bytes()?;
    let cipher = Aes256Gcm::new_from_slice(&key).unwrap();
    let (nonce_bytes, ct) = ciphertext.split_at(12);
    let nonce = Nonce::from_slice(nonce_bytes);
    let pt = cipher
        .decrypt(nonce, ct)
        .map_err(|e| anyhow!("decrypt failed: {e}"))?;
    String::from_utf8(pt).map_err(|e| anyhow!("utf8 error: {e}"))
}

#[tauri::command]
async fn health(state: tauri::State<'_, AppState>) -> Result<AppHealth, String> {
    let has_vault_key = ensure_vault_key().unwrap_or(false);
    Ok(AppHealth {
        ok: true,
        data_dir: state.data_dir.display().to_string(),
        db_path: db_path(&state.data_dir).display().to_string(),
        has_vault_key,
    })
}

#[tauri::command]
fn init_vault() -> Result<(), String> {
    let entry = keyring::Entry::new(SERVICE_NAME, VAULT_KEY_LABEL).map_err(|e| e.to_string())?;
    if entry.get_password().is_ok() { return Ok(()); }
    let mut key = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut key);
    let encoded = B64.encode(key);
    entry.set_password(&encoded).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn encrypt(plaintext: String) -> Result<Vec<u8>, String> {
    aes_encrypt(&plaintext).map_err(|e| e.to_string())
}

#[tauri::command]
fn decrypt(cipher: Vec<u8>) -> Result<String, String> {
    aes_decrypt(&cipher).map_err(|e| e.to_string())
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
    let handle = tokio::spawn(async move {
        // Simulate work for now; frontend can listen to events later
        let _ = (entry_id, style);
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    });
    state.jobs.insert(job_id.clone(), handle);
    Ok(job_id)
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

    Ok(AppState { db: pool, data_dir, jobs: Arc::new(DashMap::new()) })
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let state = STARTUP.as_ref().expect("startup failed").clone();
    tauri::Builder::default()
        .manage(state)
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            health,
            init_vault,
            encrypt,
            decrypt,
            db_upsert_entry,
            db_get_entry,
            db_list_entries,
            save_image_to_disk,
            export_pdf,
            create_comic_job,
            cancel_job
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
