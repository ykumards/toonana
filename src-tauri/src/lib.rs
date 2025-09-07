mod comic;
mod database;
mod gemini;
mod ollama;
mod settings;
mod utils;

use anyhow::Result;
use dashmap::DashMap;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use sqlx::{Pool, Sqlite};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::task::JoinHandle;
use uuid::Uuid;

use crate::comic::{ComicJobStatus, ComicStage, ExportPanel, JobId};
use crate::database::{
    create_pool, get_entry, list_entries, now_iso, upsert_entry, 
    Entry, EntryListItem, EntryUpsert, ListParams
};
use crate::settings::{load_settings_from_dir, save_settings_to_dir, Settings};
use crate::utils::{db_path, ensure_data_dir};

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
    comic_status: Arc<DashMap<String, ComicJobStatus>>,
}

#[derive(Debug, Serialize, Deserialize)]
struct AppHealth {
    ok: bool,
    data_dir: String,
    db_path: String,
    has_vault_key: bool,
}

// ===== Tauri Commands =====

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
async fn update_settings(
    state: tauri::State<'_, AppState>,
    settings: Settings,
) -> Result<Settings, String> {
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
async fn db_upsert_entry(
    state: tauri::State<'_, AppState>,
    entry: EntryUpsert,
) -> Result<Entry, String> {
    upsert_entry(&state.db, entry).await
}

#[tauri::command]
async fn db_get_entry(state: tauri::State<'_, AppState>, id: String) -> Result<Entry, String> {
    get_entry(&state.db, id).await
}

#[tauri::command]
async fn db_list_entries(
    state: tauri::State<'_, AppState>,
    p: Option<ListParams>,
) -> Result<Vec<EntryListItem>, String> {
    list_entries(&state.db, p).await
}

#[tauri::command]
async fn ollama_health(state: tauri::State<'_, AppState>) -> Result<ollama::OllamaHealth, String> {
    let settings = load_settings_from_dir(&state.data_dir);
    ollama::check_health(&settings).await
}

#[tauri::command]
async fn ollama_list_models(state: tauri::State<'_, AppState>) -> Result<Vec<String>, String> {
    let settings = load_settings_from_dir(&state.data_dir);
    ollama::list_models(&settings).await
}

#[tauri::command]
async fn ollama_generate(model: Option<String>, prompt: String) -> Result<String, String> {
    let state = STARTUP.as_ref().map_err(|e| e.to_string())?.clone();
    let settings = load_settings_from_dir(&state.data_dir);
    ollama::generate(model, prompt, &settings).await
}

#[tauri::command]
async fn create_comic_job(
    state: tauri::State<'_, AppState>,
    entry_id: String,
    style: String,
) -> Result<JobId, String> {
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

    let handle = comic::create_comic_job(
        job_id.clone(),
        entry_id,
        style,
        state.comic_status.clone(),
        state.db.clone(),
        state.data_dir.clone(),
    ).await;
    
    state.jobs.insert(job_id.clone(), handle);
    Ok(job_id)
}

#[tauri::command]
async fn get_comic_job_status(
    state: tauri::State<'_, AppState>,
    job_id: String,
) -> Result<ComicJobStatus, String> {
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

#[tauri::command]
async fn save_image_to_disk(
    state: tauri::State<'_, AppState>,
    base64_png: String,
    entry_id: String,
    panel_id: String,
) -> Result<String, String> {
    comic::save_image_to_disk(state.data_dir.clone(), base64_png, entry_id, panel_id).await
}

#[tauri::command]
async fn export_pdf(
    _state: tauri::State<'_, AppState>,
    _entry_id: String,
    _panels: Vec<ExportPanel>,
    path: String,
) -> Result<(), String> {
    // Placeholder: create an empty file so the UI can proceed
    if let Some(parent) = Path::new(&path).parent() {
        let _ = fs::create_dir_all(parent);
    }
    fs::write(&path, b"PDF export handled in frontend").map_err(|e| e.to_string())?;
    Ok(())
}

// ===== Startup and Main =====

static STARTUP: Lazy<Result<AppState>> = Lazy::new(|| tauri_startup());

fn tauri_startup() -> Result<AppState> {
    let data_dir = ensure_data_dir()?;
    let db_file = db_path(&data_dir);
    
    // We need a synchronous runtime here to construct the pool
    let rt = tokio::runtime::Runtime::new()?;
    let pool = rt.block_on(create_pool(&db_file))?;

    Ok(AppState {
        db: pool,
        data_dir,
        jobs: Arc::new(DashMap::new()),
        comic_status: Arc::new(DashMap::new()),
    })
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