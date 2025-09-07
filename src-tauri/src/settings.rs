use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Settings {
    pub gemini_api_key: Option<String>,
    pub ollama_base_url: Option<String>,
    pub default_ollama_model: Option<String>,
    pub ollama_temperature: Option<f32>,
    pub ollama_top_p: Option<f32>,
    pub nano_banana_base_url: Option<String>,
    pub nano_banana_api_key: Option<String>,
}

pub fn settings_path(data_dir: &Path) -> PathBuf {
    data_dir.join("settings.json")
}

pub fn load_settings_from_dir(data_dir: &Path) -> Settings {
    let path = settings_path(data_dir);
    if let Ok(bytes) = fs::read(&path) {
        if let Ok(s) = serde_json::from_slice::<Settings>(&bytes) {
            return s;
        }
    }
    Settings::default()
}

pub fn save_settings_to_dir(data_dir: &Path, s: &Settings) -> Result<()> {
    let path = settings_path(data_dir);
    let json = serde_json::to_vec_pretty(s)?;
    fs::write(path, json).context("write settings")?;
    Ok(())
}