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
use tracing::{info, warn, error, debug, instrument};

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

fn build_gemini_image_prompt(storyboard_text: &str, style: &str) -> String {
    // A structured, style-aware prompt for image models
    // Render exactly 3 panels in a single row, guided by the storyboard
    format!(r#"Task: Render a single-row comic with 3-4 panels from the storyboard.

Style: {}
Layout Guidelines:
- Layout: 3-4 panels, left-to-right in one horizontal row, equal width, small gutters.
- Keep characters consistent across panels (appearance, clothing, hair).
- Include speech bubbles and captions exactly as written in the storyboard.
- Avoid extra text, UI, or watermarks beyond bubbles/captions.
- Maintain clear line art, readable bubbles, cohesive backgrounds.
- Tone: light, charming, hopeful.

Output: One coherent 3-4 panel comic image (single row).

Storyboard:
{}"#,
        style,
        storyboard_text
    )
}

#[instrument(skip(status_map, db_pool, data_root), fields(job_id = %job_id, entry_id = %entry_id, style = %style))]
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
        info!("comic job queued -> parsing");
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
        debug!("comic job -> storyboarding");
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
            error!(error = %e, "failed to load entry body");
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
        debug!("comic job -> prompting");
        status_map.insert(jid.clone(), ComicJobStatus {
            job_id: jid.clone(),
            entry_id: eid.clone(),
            style: st.clone(),
            stage: ComicStage::Prompting,
            updated_at: now_iso(),
            result_image_path: None,
            storyboard_text: None,
        });
        
        let ollama_prompt = format!(r#"You are a helpful assistant that writes a short 3‑panel comic storyboard from a journal entry.

Guidelines:
- Keep tone light, hopeful, and not too dark; find a positive spin.
- Avoid heavy or sensitive content; keep it PG and uplifting.
- Privacy: do not reveal personal or identifying information from the journal entry; do not quote it verbatim. Replace names, places, dates, or unique details with neutral terms (e.g., 'a friend', 'a cafe', 'today').
- Only include characters or speakers that are clearly present in the journal entry.
- Do NOT invent specific locations, props, or events beyond what the journal clearly implies. If details are unspecified, use a neutral everyday setting.
- Maintain continuity across panels.

Output strictly in this structure for exactly 3-4 panels (no extra commentary, no blank lines between panels):
Panel 1
Description: <one concise sentence describing what the viewer sees>
Caption: <optional; short; ≤ 12 words>
Character 1: <optional; dialogue or inner thought; ≤ 12 words>
Character 2: <optional; dialogue; ≤ 12 words>
Panel 2
Description: <visual description>
Caption: <optional>
Character 1: <optional>
Panel 3
Description: <visual description>
Caption: <optional>
Character 1: <optional>

Rules:
- If a field is not needed for a panel, omit that line entirely (do not write "none").
- Prefer everyday, grounded scenes that could plausibly match the journal entry.
- Use generic references (e.g., "a friend") instead of names. Do not quote the journal directly.

Journal Entry:
{}
"#,
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
            error!(error = %e, "ollama prompting failed");
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
        debug!("comic job -> rendering");
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
            // While waiting for Nano-Banana, periodically bump progress so the UI stays alive
            let mut tick_completed: u32 = 0;
            info!("sending storyboard to nano-banana");
            let req_fut = nano_banana_generate_image(&storyboard_text, &settings);
            tokio::pin!(req_fut);

            let res = loop {
                tokio::select! {
                    r = &mut req_fut => { break r; }
                    _ = tokio::time::sleep(std::time::Duration::from_millis(800)) => {
                        // Cap at 98 to leave room for finalize/saving
                        if tick_completed < 98 {
                            tick_completed = tick_completed.saturating_add(2).min(98);
                            debug!(progress = tick_completed, "nano-banana waiting...");
                            status_map.insert(jid.clone(), ComicJobStatus {
                                job_id: jid.clone(),
                                entry_id: eid.clone(),
                                style: st.clone(),
                                stage: ComicStage::Rendering { completed: tick_completed, total: 100 },
                                updated_at: now_iso(),
                                result_image_path: None,
                                storyboard_text: Some(storyboard_text.clone()),
                            });
                        }
                    }
                }
            };

            // Fallback to direct Gemini if Nano-Banana failed
            match res {
                Ok(s) => {
                    info!("nano-banana image received");
                    Ok(s)
                },
                Err(e) => {
                    warn!(error = %e, "nano-banana failed, falling back to gemini");
                    let prompt = build_gemini_image_prompt(&storyboard_text, &st);
                    let mut last_tick = tick_completed;
                    generate_image_with_progress(&prompt, &settings, |completed, total| {
                        if completed > last_tick && completed % 5 == 0 {
                            last_tick = completed;
                            debug!(progress = completed, total = total, "gemini rendering progress");
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
                    }).await.map_err(|ge| format!("nano-banana failed: {e}; gemini fallback failed: {ge}"))
                }
            }
        } else {
            let prompt = build_gemini_image_prompt(&storyboard_text, &st);
            let mut last_tick = 0u32;
            generate_image_with_progress(&prompt, &settings, |completed, total| {
                if completed > last_tick && completed % 5 == 0 {
                    last_tick = completed;
                    debug!(progress = completed, total = total, "gemini rendering progress");
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
                        info!(path = %img_path.display(), "saved generated image");
                        
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
                        error!(error = %e, "image decode failed");
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
                error!(error = %e, "image generation failed");
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