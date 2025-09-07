use anyhow::{anyhow, Context, Result};
use directories::ProjectDirs;
use std::fs;
use std::path::PathBuf;

pub fn app_dirs() -> Result<ProjectDirs> {
    ProjectDirs::from("app", "toonana", "toonana")
        .ok_or_else(|| anyhow!("cannot resolve project dirs"))
}

pub fn ensure_data_dir() -> Result<PathBuf> {
    let dirs = app_dirs()?;
    let data_dir = dirs.data_dir().to_path_buf();
    fs::create_dir_all(&data_dir).context("create data dir")?;
    Ok(data_dir)
}

pub fn db_path(data_dir: &PathBuf) -> PathBuf {
    data_dir.join("app.sqlite")
}