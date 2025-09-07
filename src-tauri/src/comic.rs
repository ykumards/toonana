use anyhow::{anyhow, Result};
use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use sqlx::{Pool, Sqlite};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::task::JoinHandle;

use crate::database::{get_entry_body, now_iso};
use crate::gemini::{generate_image_with_progress, nano_banana_generate_image};
use crate::ollama::generate_streaming;
use crate::settings::load_settings_from_dir;

pub type JobId = String;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "stage", rename_all = "snake_case")]
pub enum ComicStage {
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
pub struct ComicJobStatus {
    pub job_id: String,
    pub entry_id: String,
    pub style: String,
    pub stage: ComicStage,
    pub updated_at: String,
    pub result_image_path: Option<String>,
    pub storyboard_text: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ExportPanel {
    pub panel_id: String,
    pub image_path: Option<String>,
    pub dialogue_cipher: Option<Vec<u8>>,
}

pub fn decode_base64_png(s: &str) -> Result<Vec<u8>> {
    let data = if let Some(idx) = s.find(",") {
        &s[(idx + 1)..]
    } else {
        s
    };
    B64.decode(data).map_err(|e| anyhow!("base64 decode: {e}"))
}

pub fn guess_image_extension(bytes: &[u8]) -> &'static str {
    // PNG
    if bytes.len() >= 8 && bytes[0..8] == [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A] {
        return "png";
    }
    // JPEG
    if bytes.len() >= 3 && bytes[0..3] == [0xFF, 0xD8, 0xFF] {
        return "jpg";
    }
    // WEBP (RIFF....WEBP)
    if bytes.len() >= 12
        && &bytes[0..4] == b"RIFF"
        && &bytes[8..12] == b"WEBP"
    {
        return "webp";
    }
    "png"
}

pub async fn create_comic_job(
    job_id: String,
    entry_id: String,
    style: String,
    status_map: Arc<DashMap<String, ComicJobStatus>>,
    db_pool: Pool<Sqlite>,
    data_root: PathBuf,
) -> JoinHandle<()> {
    let jid = job_id.clone();
    let eid = entry_id.clone();
    let st = style.clone();
    
    tokio::spawn(async move {
        // Step 1: Parse entry
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
        let entry_body = get_entry_body(&db_pool, &eid).await;
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

        // Step 3: Prompting
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
            "You are a helpful assistant that writes short 4-6 panel comic storyboards from a journal entry.\n\nJournal Entry:\n{}\n\nGuidelines:\n- Keep tone light, hopeful, and not too dark; find a positive spin.\n- Avoid heavy or sensitive content; keep it PG and uplifting.\n- Privacy: do not reveal personal or identifying information from the journal entry; do not quote it verbatim. Replace names, places, dates, or unique details with neutral terms (e.g., 'a friend', 'a cafe', 'today').\n\nReturn ONLY the transcript lines in exactly this format (no titles, explanations, or extra text):\nPanel 1\nCaption: <short caption>\nPanel 2\nCharacter 1: <dialogue>\n...\nKeep each caption/dialogue under 12 words.",
            entry_text
        );

        let mut storyboard_text = String::new();
        let settings = load_settings_from_dir(&data_root);
        
        let stream_res = generate_streaming(None, ollama_prompt, &settings, |chunk| {
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

        // Step 4: Rendering
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

        let nb_res = if settings.nano_banana_base_url.is_some() {
            nano_banana_generate_image(&storyboard_text, &settings).await
        } else {
            let mut last_tick = 0u32;
            generate_image_with_progress(&storyboard_text, &settings, |completed, total| {
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
            }).await
        };
        
        match nb_res {
            Ok(b64_img) => {
                match decode_base64_png(&b64_img) {
                    Ok(bytes) => {
                        let ext = guess_image_extension(&bytes);
                        let img_path = images_dir.join(format!("{}-result.{}", &jid, ext));
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
                    stage: ComicStage::Failed { error: format!("image generation failed: {}", e) },
                    updated_at: now_iso(),
                    result_image_path: None,
                    storyboard_text: Some(storyboard_text.clone()),
                });
            }
        }
    })
}

pub async fn save_image_to_disk(
    data_dir: PathBuf,
    base64_png: String,
    entry_id: String,
    panel_id: String,
) -> Result<String, String> {
    let bytes = decode_base64_png(&base64_png).map_err(|e| e.to_string())?;
    let img_dir = data_dir.join("images").join(&entry_id);
    tokio::fs::create_dir_all(&img_dir)
        .await
        .map_err(|e| e.to_string())?;
    let file_path = img_dir.join(format!("{panel_id}.png"));
    tokio::fs::write(&file_path, bytes)
        .await
        .map_err(|e| e.to_string())?;
    Ok(file_path.display().to_string())
}