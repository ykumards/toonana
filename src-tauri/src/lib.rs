mod comic;
mod database;
mod gemini;
mod ollama;
mod settings;
mod utils;

use anyhow::Result;
use dashmap::DashMap;
use once_cell::sync::Lazy;
use once_cell::sync::OnceCell;
use serde::{Deserialize, Serialize};
use sqlx::{Pool, Sqlite};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::task::JoinHandle;
use uuid::Uuid;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};
use tracing_appender::rolling;

use crate::comic::{ComicJobStatus, ComicStage, ExportPanel, JobId};
use crate::database::{
    create_pool, get_entry, list_entries, now_iso, upsert_entry, delete_entry,
    Entry, EntryListItem, EntryUpsert, ListParams
};
use crate::settings::{load_settings_from_dir, save_settings_to_dir, Settings};
use crate::utils::{db_path, ensure_data_dir};
use crate::comic::{decode_base64_png, guess_image_extension};

// kept for potential future re-enable of encryption
#[allow(dead_code)]
static SERVICE_NAME: &str = "toonana";
#[allow(dead_code)]
static VAULT_KEY_LABEL: &str = "vault-key-v1";

static LOG_GUARD: OnceCell<tracing_appender::non_blocking::WorkerGuard> = OnceCell::new();

fn init_tracing(data_dir: &Path) -> Result<()> {
    let logs_dir = data_dir.join("logs");
    let _ = fs::create_dir_all(&logs_dir);

    let file_appender = rolling::daily(&logs_dir, "toonana.log");
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);
    let _ = LOG_GUARD.set(guard);

    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info"));

    let stdout_layer = fmt::layer()
        .with_target(true)
        .with_thread_ids(false)
        .with_thread_names(false)
        .with_ansi(true)
        .with_writer(std::io::stdout);

    let file_layer = fmt::layer()
        .with_target(true)
        .with_ansi(false)
        .with_writer(non_blocking);

    tracing_subscriber::registry()
        .with(env_filter)
        .with(stdout_layer)
        .with(file_layer)
        .init();
    Ok(())
}

#[derive(Clone)]
struct AppState {
    db: Pool<Sqlite>,
    data_dir: PathBuf,
    jobs: Arc<DashMap<String, JoinHandle<()>>>,
    comic_status: Arc<DashMap<String, ComicJobStatus>>,
    avatar_status: Arc<DashMap<String, AvatarJobStatus>>,
}

#[derive(Debug, Serialize, Deserialize)]
struct AppHealth {
    ok: bool,
    data_dir: String,
    db_path: String,
    has_vault_key: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct ComicItem {
    entry_id: String,
    image_path: String,
    created_at: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct ComicsByDay {
    date: String,
    comics: Vec<ComicItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "stage", rename_all = "snake_case")]
enum AvatarStage {
    Queued,
    Rendering { completed: u32, total: u32 },
    Done,
    Failed { error: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AvatarJobStatus {
    job_id: String,
    updated_at: String,
    stage: AvatarStage,
    image_base64: Option<String>,
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

#[tauri::command]
async fn generate_avatar_image(prompt: String) -> Result<String, String> {
    let state = STARTUP.as_ref().map_err(|e| e.to_string())?.clone();
    let mut settings = load_settings_from_dir(&state.data_dir);
    // Do not include previous avatar image as an input when generating a new avatar
    settings.avatar_image_path = None;
    let full_prompt = gemini::build_avatar_image_prompt(&prompt);
    tracing::info!(
        nano_banana = %settings.nano_banana_base_url.as_deref().unwrap_or("(none)"),
        desc_len = full_prompt.len(),
        "avatar: start generation"
    );
    // Helper to ensure we always return a correctly-typed data URI
    fn to_data_uri(s: String) -> String {
        if s.starts_with("data:") {
            return s;
        }
        let (mime, _ext) = match decode_base64_png(&s) {
            Ok(bytes) => match guess_image_extension(&bytes) {
                "jpg" => ("image/jpeg", "jpg"),
                "webp" => ("image/webp", "webp"),
                _ => ("image/png", "png"),
            },
            Err(_) => ("image/png", "png"),
        };
        format!("data:{};base64,{}", mime, s)
    }
    if settings.nano_banana_base_url.is_some() {
        match gemini::nano_banana_generate_image(&full_prompt, &settings).await {
            Ok(s) => {
                tracing::info!("avatar: nano-banana success");
                return Ok(to_data_uri(s));
            }
            Err(e) => {
                tracing::warn!(error = %e, "avatar: nano-banana failed, falling back to gemini (stream)");
            }
        }
    }
    match gemini::generate_image_with_progress(&full_prompt, &settings, |_c, _t| {}).await {
        Ok(s) => {
            tracing::info!("avatar: gemini (stream) success");
            Ok(to_data_uri(s))
        }
        Err(e) => {
            tracing::error!(error = %e, "avatar: gemini (stream) failed");
            Err(format!("avatar generation failed: {}", e))
        }
    }
}

#[tauri::command]
async fn create_avatar_job(
    state: tauri::State<'_, AppState>,
    description: String,
) -> Result<JobId, String> {
    let job_id = Uuid::new_v4().to_string();
    state.avatar_status.insert(job_id.clone(), AvatarJobStatus {
        job_id: job_id.clone(),
        updated_at: now_iso(),
        stage: AvatarStage::Queued,
        image_base64: None,
    });

    let data_dir = state.data_dir.clone();
    let status_map = state.avatar_status.clone();

    let job_id_for_task = job_id.clone();
    let handle = tokio::spawn(async move {
        let settings = load_settings_from_dir(&data_dir);
        let full_prompt = gemini::build_avatar_image_prompt(&description);
        tracing::info!(job_id = %job_id_for_task, desc_len = description.len(), "avatar job: started");

        // helper to update progress
        let mut last_tick: u32 = 0;
        let update_progress = |completed: u32, total: u32| {
            status_map.insert(job_id_for_task.clone(), AvatarJobStatus {
                job_id: job_id_for_task.clone(),
                updated_at: now_iso(),
                stage: AvatarStage::Rendering { completed, total },
                image_base64: None,
            });
        };

        // Try Nano-Banana first when configured, with periodic progress ticks
        let result_b64: Result<String, String> = if settings.nano_banana_base_url.is_some() {
            tracing::info!(job_id = %job_id_for_task, "avatar job: sending to nano-banana");
            let fut = gemini::nano_banana_generate_image(&full_prompt, &settings);
            tokio::pin!(fut);
            let res = loop {
                tokio::select! {
                    r = &mut fut => { break r; }
                    _ = tokio::time::sleep(std::time::Duration::from_millis(800)) => {
                        if last_tick < 98 {
                            last_tick = last_tick.saturating_add(2).min(98);
                            update_progress(last_tick, 100);
                        }
                    }
                }
            };
            match res {
                Ok(s) => Ok(s),
                Err(e) => {
                    tracing::warn!(job_id = %job_id_for_task, error = %e, "avatar job: nano-banana failed, fallback to gemini");
                    gemini::generate_image_with_progress(&full_prompt, &settings, |c, t| {
                        if c > last_tick && c % 5 == 0 { last_tick = c; }
                        update_progress(c, t);
                    }).await
                }
            }
        } else {
            gemini::generate_image_with_progress(&full_prompt, &settings, |c, t| {
                if c > last_tick && c % 5 == 0 { last_tick = c; }
                update_progress(c, t);
            }).await
        };

        match result_b64 {
            Ok(b64) => {
                tracing::info!(job_id = %job_id_for_task, len = b64.len(), "avatar job: image received");
                // ensure data URI with correct mime
                let data_uri = {
                    if b64.starts_with("data:") { b64.clone() } else {
                        match decode_base64_png(&b64) {
                            Ok(bytes) => {
                                let mime = match guess_image_extension(&bytes) {
                                    "jpg" => "image/jpeg",
                                    "webp" => "image/webp",
                                    _ => "image/png",
                                };
                                format!("data:{};base64,{}", mime, b64)
                            }
                            Err(_) => format!("data:image/png;base64,{}", b64),
                        }
                    }
                };
                status_map.insert(job_id_for_task.clone(), AvatarJobStatus {
                    job_id: job_id_for_task.clone(),
                    updated_at: now_iso(),
                    stage: AvatarStage::Done,
                    image_base64: Some(data_uri),
                });
            }
            Err(e) => {
                tracing::error!(job_id = %job_id_for_task, error = %e, "avatar job: failed");
                status_map.insert(job_id_for_task.clone(), AvatarJobStatus {
                    job_id: job_id_for_task.clone(),
                    updated_at: now_iso(),
                    stage: AvatarStage::Failed { error: e },
                    image_base64: None,
                });
            }
        }
    });

    state.jobs.insert(job_id.clone(), handle);
    Ok(job_id)
}

#[tauri::command]
async fn get_avatar_job_status(
    state: tauri::State<'_, AppState>,
    job_id: String,
) -> Result<AvatarJobStatus, String> {
    state
        .avatar_status
        .get(&job_id)
        .map(|v| v.clone())
        .ok_or_else(|| "job not found".to_string())
}

#[tauri::command]
async fn cancel_avatar_job(state: tauri::State<'_, AppState>, job_id: String) -> Result<(), String> {
    if let Some((_, handle)) = state.jobs.remove(&job_id) {
        handle.abort();
    }
    Ok(())
}

#[tauri::command]
async fn save_avatar_image(base64_png: String) -> Result<String, String> {
    let state = STARTUP.as_ref().map_err(|e| e.to_string())?.clone();
    let bytes = decode_base64_png(&base64_png).map_err(|e| e.to_string())?;
    let ext = guess_image_extension(&bytes);
    let avatars_dir = state.data_dir.join("avatars");
    let _ = std::fs::create_dir_all(&avatars_dir);
    // Clean older avatar files to avoid cache collisions
    if let Ok(rd) = std::fs::read_dir(&avatars_dir) {
        for ent in rd.flatten() {
            if let Some(name) = ent.file_name().to_str() {
                if name.starts_with("avatar") {
                    let _ = std::fs::remove_file(ent.path());
                }
            }
        }
    }
    // Use a unique filename to bust caches
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let path = avatars_dir.join(format!("avatar-{}.{}", ts, ext));
    std::fs::write(&path, &bytes).map_err(|e| e.to_string())?;
    tracing::info!(path = %path.display(), ext = %ext, "avatar: saved image to disk");
    // Update settings with saved path
    let mut s = load_settings_from_dir(&state.data_dir);
    s.avatar_image_path = Some(path.display().to_string());
    save_settings_to_dir(&state.data_dir, &s).map_err(|e| e.to_string())?;
    Ok(path.display().to_string())
}

#[tauri::command]
async fn list_comics_by_day(
    state: tauri::State<'_, AppState>,
    limit_days: Option<i64>,
) -> Result<Vec<ComicsByDay>, String> {
    use std::collections::BTreeMap;
    use std::fs;
    // removed unused local import of Path

    let limit_days = limit_days.unwrap_or(120);

    // Fetch recent entries
    let entries = list_entries(
        &state.db,
        Some(ListParams { limit: Some(2000), offset: Some(0) }),
    )
    .await?;

    let mut by_day: BTreeMap<String, Vec<ComicItem>> = BTreeMap::new();

    for e in entries.into_iter() {
        let created = e.created_at.clone();
        let day = created.split('T').next().unwrap_or("").to_string();
        if day.is_empty() { continue; }

        let entry_img_dir = state.data_dir.join("images").join(&e.id);
        if !entry_img_dir.exists() { continue; }

        // Find the newest generated image in the entry image folder
        let mut best_path: Option<(String, std::time::SystemTime)> = None;
        if let Ok(rd) = fs::read_dir(&entry_img_dir) {
            for ent in rd.flatten() {
                let path = ent.path();
                if !path.is_file() { continue; }
                let file_name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
                let ext_ok = path.extension().and_then(|s| s.to_str()).map(|ext| {
                    matches!(ext.to_ascii_lowercase().as_str(), "png" | "jpg" | "jpeg" | "webp")
                }).unwrap_or(false);
                if !ext_ok { continue; }
                // Prefer the composite result image if present
                if !file_name.contains("-result") { /* still acceptable */ }
                let meta = ent.metadata().ok();
                let modified = meta.and_then(|m| m.modified().ok()).unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                let path_str = path.display().to_string();
                match &best_path {
                    Some((_, ts)) if modified <= *ts => {}
                    _ => { best_path = Some((path_str, modified)); }
                }
            }
        }

        if let Some((img_path, _)) = best_path {
            by_day.entry(day).or_default().push(ComicItem {
                entry_id: e.id,
                image_path: img_path,
                created_at: created,
            });
        }
    }

    // Convert to Vec, sort by date desc, and optionally limit by recent days
    let mut items: Vec<(String, Vec<ComicItem>)> = by_day.into_iter().collect();
    items.sort_by(|a, b| b.0.cmp(&a.0));

    // Apply limit_days by truncating to that many unique days
    let items: Vec<ComicsByDay> = items
        .into_iter()
        .take(limit_days as usize)
        .map(|(date, comics)| ComicsByDay { date, comics })
        .collect();

    Ok(items)
}

#[tauri::command]
async fn db_delete_entry(
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<(), String> {
    delete_entry(&state.db, &id).await?;
    let img_dir = state.data_dir.join("images").join(&id);
    if img_dir.exists() {
        let _ = tokio::fs::remove_dir_all(&img_dir).await;
    }
    Ok(())
}

// ===== Startup and Main =====

static STARTUP: Lazy<Result<AppState>> = Lazy::new(|| tauri_startup());

fn tauri_startup() -> Result<AppState> {
    let data_dir = ensure_data_dir()?;
    let db_file = db_path(&data_dir);
    // Initialize structured logging early
    let _ = init_tracing(&data_dir);
    
    // We need a synchronous runtime here to construct the pool
    let rt = tokio::runtime::Runtime::new()?;
    let pool = rt.block_on(create_pool(&db_file))?;

    Ok(AppState {
        db: pool,
        data_dir,
        jobs: Arc::new(DashMap::new()),
        comic_status: Arc::new(DashMap::new()),
        avatar_status: Arc::new(DashMap::new()),
    })
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let state = STARTUP.as_ref().expect("startup failed").clone();
    tracing::info!(data_dir = %state.data_dir.display(), "backend initialized");
    
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
            db_delete_entry,
            save_image_to_disk,
            export_pdf,
            create_comic_job,
            get_comic_job_status,
            cancel_job,
            ollama_health,
            ollama_list_models,
            ollama_generate,
            list_comics_by_day
            , generate_avatar_image
            , save_avatar_image
            , create_avatar_job
            , get_avatar_job_status
            , cancel_avatar_job
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}