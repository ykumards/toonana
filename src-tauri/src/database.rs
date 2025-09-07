use anyhow::Result;
use serde::{Deserialize, Serialize};
use sqlx::{Pool, Sqlite, Row, sqlite::SqlitePoolOptions, sqlite::SqliteConnectOptions};
use std::path::Path;
use uuid::Uuid;
use time::OffsetDateTime;

#[derive(Debug, Serialize, Deserialize)]
pub struct EntryUpsert {
    pub id: Option<String>,
    pub body_cipher: Vec<u8>,
    pub mood: Option<String>,
    pub tags: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Entry {
    pub id: String,
    pub created_at: String,
    pub updated_at: String,
    pub body_cipher: Vec<u8>,
    pub mood: Option<String>,
    pub tags: Option<serde_json::Value>,
    pub embedding: Option<Vec<u8>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct EntryListItem {
    pub id: String,
    pub created_at: String,
    pub updated_at: String,
    pub body_preview: Option<String>,
    pub mood: Option<String>,
    pub tags: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ListParams {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

pub fn now_iso() -> String {
    OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default()
}

pub async fn init_db(pool: &Pool<Sqlite>) -> Result<()> {
    // First, check if we need to migrate from the old schema with title
    let table_info = sqlx::query("PRAGMA table_info(entries)")
        .fetch_all(pool)
        .await
        .unwrap_or_default();
    
    let has_title_column = table_info.iter().any(|row| {
        row.try_get::<String, _>("name")
            .map(|n| n == "title")
            .unwrap_or(false)
    });
    
    if has_title_column {
        // Need to migrate: create new table without title column
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS entries_new (
                id TEXT PRIMARY KEY,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                body_cipher BLOB NOT NULL,
                mood TEXT,
                tags TEXT,
                embedding BLOB
            );
            "#,
        )
        .execute(pool)
        .await?;
        
        // Copy data from old table (excluding title)
        sqlx::query(
            r#"
            INSERT INTO entries_new (id, created_at, updated_at, body_cipher, mood, tags, embedding)
            SELECT id, created_at, updated_at, body_cipher, mood, tags, embedding FROM entries
            "#,
        )
        .execute(pool)
        .await?;
        
        // Drop old table and rename new one
        sqlx::query("DROP TABLE entries")
            .execute(pool)
            .await?;
        
        sqlx::query("ALTER TABLE entries_new RENAME TO entries")
            .execute(pool)
            .await?;
    } else {
        // Create table with new schema (no title)
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS entries (
                id TEXT PRIMARY KEY,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                body_cipher BLOB NOT NULL,
                mood TEXT,
                tags TEXT,
                embedding BLOB
            );
            "#,
        )
        .execute(pool)
        .await?;
    }

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

pub async fn create_pool(db_path: &Path) -> Result<Pool<Sqlite>> {
    let opts = SqliteConnectOptions::new()
        .filename(db_path)
        .create_if_missing(true);
    
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(opts)
        .await?;
    
    init_db(&pool).await?;
    Ok(pool)
}

pub async fn upsert_entry(pool: &Pool<Sqlite>, entry: EntryUpsert) -> Result<Entry, String> {
    let id = entry.id.unwrap_or_else(|| Uuid::new_v4().to_string());
    let now = now_iso();
    let tags_json = entry.tags.map(|t| t.to_string());

    let _ = sqlx::query(
        r#"
        INSERT INTO entries (id, created_at, updated_at, body_cipher, mood, tags, embedding)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL)
        ON CONFLICT(id) DO UPDATE SET
          updated_at=excluded.updated_at,
          body_cipher=excluded.body_cipher,
          mood=excluded.mood,
          tags=excluded.tags
        "#,
    )
    .bind(&id)
    .bind(&now)
    .bind(&now)
    .bind(&entry.body_cipher)
    .bind(&entry.mood)
    .bind(&tags_json)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;

    get_entry(pool, id).await
}

pub async fn get_entry(pool: &Pool<Sqlite>, id: String) -> Result<Entry, String> {
    let row = sqlx::query(
        r#"SELECT id, created_at, updated_at, body_cipher, mood, tags, embedding FROM entries WHERE id = ?1"#
    )
    .bind(&id)
    .fetch_one(pool)
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
        body_cipher: row.try_get("body_cipher").map_err(|e| e.to_string())?,
        mood: row.try_get("mood").map_err(|e| e.to_string())?,
        tags: tags_val,
        embedding: row.try_get("embedding").ok(),
    })
}

pub async fn list_entries(pool: &Pool<Sqlite>, params: Option<ListParams>) -> Result<Vec<EntryListItem>, String> {
    let limit = params.as_ref().and_then(|p| p.limit).unwrap_or(100);
    let offset = params.as_ref().and_then(|p| p.offset).unwrap_or(0);
    
    let rows = sqlx::query(
        r#"SELECT id, created_at, updated_at, body_cipher, mood, tags FROM entries ORDER BY created_at DESC LIMIT ?1 OFFSET ?2"#
    )
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;
    
    let items = rows
        .into_iter()
        .map(|row| {
            let tags_str: Option<String> = row.try_get("tags").ok();
            let tags_val = tags_str
                .as_deref()
                .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok());
            
            // Get body preview - first 50 chars of decrypted body
            let body_preview = if let Ok(cipher) = row.try_get::<Vec<u8>, _>("body_cipher") {
                String::from_utf8(cipher)
                    .ok()
                    .map(|text| {
                        let preview = text.chars().take(50).collect::<String>();
                        if text.len() > 50 {
                            format!("{}...", preview.trim())
                        } else {
                            preview.trim().to_string()
                        }
                    })
            } else {
                None
            };
            
            EntryListItem {
                id: row.try_get("id").unwrap_or_default(),
                created_at: row.try_get("created_at").unwrap_or_default(),
                updated_at: row.try_get("updated_at").unwrap_or_default(),
                body_preview,
                mood: row.try_get("mood").ok(),
                tags: tags_val,
            }
        })
        .collect();
    
    Ok(items)
}

pub async fn get_entry_body(pool: &Pool<Sqlite>, entry_id: &str) -> Result<String> {
    let row = sqlx::query(
        r#"SELECT body_cipher FROM entries WHERE id = ?1"#
    )
    .bind(entry_id)
    .fetch_one(pool)
    .await
    .map_err(|e| anyhow::anyhow!("db: {}", e))?;
    
    let cipher: Vec<u8> = row.try_get("body_cipher")
        .map_err(|e| anyhow::anyhow!("row: {}", e))?;
    
    let text = String::from_utf8(cipher)
        .map_err(|e| anyhow::anyhow!("utf8: {}", e))?;
    
    Ok(text)
}

pub async fn delete_entry(pool: &Pool<Sqlite>, id: &str) -> Result<(), String> {
    // Remove dependent rows first to maintain integrity
    let _ = sqlx::query(r#"DELETE FROM panels WHERE entry_id = ?1"#)
        .bind(id)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;

    let _ = sqlx::query(r#"DELETE FROM storyboards WHERE entry_id = ?1"#)
        .bind(id)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;

    let _ = sqlx::query(r#"DELETE FROM entries WHERE id = ?1"#)
        .bind(id)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;

    Ok(())
}