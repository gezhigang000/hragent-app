//! SQLite database — schema creation and CRUD operations.
#![allow(dead_code)]

use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;

use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use log::{debug, info};
use rusqlite::{params, Connection, OptionalExtension};
use std::sync::MutexGuard;
use uuid::Uuid;

/// Thread-safe SQLite database handle.
pub struct Database {
    conn: Mutex<Connection>,
}

impl Database {
    /// Acquire the database connection, returning a descriptive error
    /// instead of panicking if the mutex is poisoned.
    fn conn(&self) -> Result<MutexGuard<'_, Connection>> {
        self.conn.lock().map_err(|_| anyhow!("Database mutex poisoned"))
    }

    /// Open (or create) the database at `db_path` and initialise the schema.
    pub fn new(db_path: &Path) -> Result<Self> {
        // Ensure parent directory exists.
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create database directory: {:?}", parent))?;
        }

        let conn = Connection::open(db_path)
            .with_context(|| format!("Failed to open database at {:?}", db_path))?;

        // Enable WAL mode and foreign keys.
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA foreign_keys = ON;
             PRAGMA busy_timeout = 5000;",
        )
        .context("Failed to set database pragmas")?;

        let db = Self {
            conn: Mutex::new(conn),
        };

        db.initialize_schema()?;

        info!("Database initialized at {:?}", db_path);
        Ok(db)
    }

    // ───────────────────────────────────────────────
    // Schema
    // ───────────────────────────────────────────────

    fn initialize_schema(&self) -> Result<()> {
        let conn = self.conn()?;
        conn.execute_batch(
            "
            -- Conversations
            CREATE TABLE IF NOT EXISTS conversations (
                id          TEXT PRIMARY KEY,
                title       TEXT NOT NULL,
                created_at  TEXT NOT NULL,
                updated_at  TEXT NOT NULL,
                is_archived INTEGER DEFAULT 0
            );

            -- Messages
            CREATE TABLE IF NOT EXISTS messages (
                id               TEXT PRIMARY KEY,
                conversation_id  TEXT NOT NULL,
                role             TEXT NOT NULL CHECK(role IN ('user', 'assistant', 'system')),
                content_json     TEXT NOT NULL,
                created_at       TEXT NOT NULL,
                FOREIGN KEY (conversation_id) REFERENCES conversations(id) ON DELETE CASCADE
            );
            CREATE INDEX IF NOT EXISTS idx_messages_conversation
                ON messages(conversation_id, created_at);

            -- Uploaded files
            CREATE TABLE IF NOT EXISTS uploaded_files (
                id               TEXT PRIMARY KEY,
                conversation_id  TEXT NOT NULL,
                original_name    TEXT NOT NULL,
                stored_path      TEXT NOT NULL,
                file_type        TEXT NOT NULL,
                file_size        INTEGER NOT NULL,
                parsed_summary   TEXT,
                uploaded_at      TEXT NOT NULL,
                FOREIGN KEY (conversation_id) REFERENCES conversations(id) ON DELETE CASCADE
            );

            -- Generated files
            CREATE TABLE IF NOT EXISTS generated_files (
                id              TEXT PRIMARY KEY,
                conversation_id TEXT NOT NULL,
                message_id      TEXT,
                file_name       TEXT NOT NULL,
                stored_path     TEXT NOT NULL,
                file_type       TEXT NOT NULL,
                file_size       INTEGER NOT NULL DEFAULT 0,
                category        TEXT NOT NULL CHECK(category IN ('report', 'chart', 'data', 'temp', 'other')),
                description     TEXT,
                version         INTEGER DEFAULT 1,
                is_latest       INTEGER DEFAULT 1,
                superseded_by   TEXT,
                created_by_step INTEGER,
                created_at      TEXT NOT NULL,
                expires_at      TEXT,
                FOREIGN KEY (conversation_id) REFERENCES conversations(id) ON DELETE CASCADE
            );
            CREATE INDEX IF NOT EXISTS idx_generated_files_conversation
                ON generated_files(conversation_id, is_latest);
            CREATE INDEX IF NOT EXISTS idx_generated_files_category
                ON generated_files(category, is_latest);
            CREATE INDEX IF NOT EXISTS idx_generated_files_expires
                ON generated_files(expires_at)
                WHERE expires_at IS NOT NULL;

            -- Analysis states
            CREATE TABLE IF NOT EXISTS analysis_states (
                id               TEXT PRIMARY KEY,
                conversation_id  TEXT NOT NULL UNIQUE,
                current_step     INTEGER DEFAULT 0,
                step_status      TEXT DEFAULT '{}',
                state_data       TEXT DEFAULT '{}',
                updated_at       TEXT NOT NULL,
                FOREIGN KEY (conversation_id) REFERENCES conversations(id) ON DELETE CASCADE
            );

            -- Enterprise memory (key-value knowledge store)
            CREATE TABLE IF NOT EXISTS enterprise_memory (
                id          TEXT PRIMARY KEY,
                key         TEXT NOT NULL UNIQUE,
                value       TEXT NOT NULL,
                source      TEXT,
                created_at  TEXT NOT NULL,
                updated_at  TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_enterprise_memory_key
                ON enterprise_memory(key);

            -- Settings (simple key-value)
            CREATE TABLE IF NOT EXISTS settings (
                key   TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );

            -- Audit log
            CREATE TABLE IF NOT EXISTS audit_log (
                id         INTEGER PRIMARY KEY AUTOINCREMENT,
                action     TEXT NOT NULL,
                detail     TEXT,
                created_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_audit_log_created
                ON audit_log(created_at);

            -- Search cache
            CREATE TABLE IF NOT EXISTS search_cache (
                query_hash TEXT PRIMARY KEY,
                query      TEXT NOT NULL,
                results    TEXT NOT NULL,
                expires_at TEXT NOT NULL
            );
            ",
        )
        .context("Failed to initialize database schema")?;

        debug!("Database schema initialized");
        Ok(())
    }

    // ───────────────────────────────────────────────
    // Conversations
    // ───────────────────────────────────────────────

    /// Create a new conversation.
    pub fn create_conversation(&self, id: &str, title: &str) -> Result<()> {
        let conn = self.conn()?;
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO conversations (id, title, created_at, updated_at) VALUES (?1, ?2, ?3, ?4)",
            params![id, title, now, now],
        )
        .context("Failed to create conversation")?;
        Ok(())
    }

    /// Update a conversation's title.
    pub fn update_conversation_title(&self, id: &str, title: &str) -> Result<()> {
        let conn = self.conn()?;
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE conversations SET title = ?1, updated_at = ?2 WHERE id = ?3",
            params![title, now, id],
        )
        .context("Failed to update conversation title")?;
        Ok(())
    }

    /// Retrieve all non-archived conversations, most recent first.
    pub fn get_conversations(&self) -> Result<Vec<serde_json::Value>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT id, title, created_at, updated_at, is_archived
                 FROM conversations
                 WHERE is_archived = 0
                 ORDER BY updated_at DESC",
            )
            .context("Failed to prepare get_conversations")?;

        let rows = stmt
            .query_map([], |row| {
                Ok(serde_json::json!({
                    "id": row.get::<_, String>(0)?,
                    "title": row.get::<_, String>(1)?,
                    "createdAt": row.get::<_, String>(2)?,
                    "updatedAt": row.get::<_, String>(3)?,
                    "isArchived": row.get::<_, bool>(4)?,
                }))
            })
            .context("Failed to query conversations")?;

        let mut result = Vec::new();
        for row in rows {
            result.push(row.context("Failed to read conversation row")?);
        }
        Ok(result)
    }

    /// Delete a conversation and all associated data (cascading).
    pub fn delete_conversation(&self, id: &str) -> Result<()> {
        let conn = self.conn()?;
        conn.execute("DELETE FROM conversations WHERE id = ?1", params![id])
            .context("Failed to delete conversation")?;
        Ok(())
    }

    /// Get all physical file paths associated with a conversation.
    ///
    /// Returns stored_path values from both `uploaded_files` and
    /// `generated_files`. Used to clean up disk files before deleting
    /// the conversation (CASCADE only removes DB rows, not files).
    pub fn get_file_paths_for_conversation(&self, conversation_id: &str) -> Result<Vec<String>> {
        let conn = self.conn()?;
        let mut paths = Vec::new();

        // Uploaded files
        let mut stmt = conn.prepare(
            "SELECT stored_path FROM uploaded_files WHERE conversation_id = ?1"
        ).context("Failed to prepare uploaded_files path query")?;
        let rows = stmt.query_map(params![conversation_id], |row| {
            row.get::<_, String>(0)
        }).context("Failed to query uploaded file paths")?;
        for row in rows {
            paths.push(row.context("Failed to read uploaded file path")?);
        }

        // Generated files
        let mut stmt2 = conn.prepare(
            "SELECT stored_path FROM generated_files WHERE conversation_id = ?1"
        ).context("Failed to prepare generated_files path query")?;
        let rows2 = stmt2.query_map(params![conversation_id], |row| {
            row.get::<_, String>(0)
        }).context("Failed to query generated file paths")?;
        for row in rows2 {
            paths.push(row.context("Failed to read generated file path")?);
        }

        Ok(paths)
    }

    // ───────────────────────────────────────────────
    // Messages
    // ───────────────────────────────────────────────

    /// Insert a new message.
    pub fn insert_message(
        &self,
        id: &str,
        conversation_id: &str,
        role: &str,
        content_json: &str,
    ) -> Result<()> {
        let conn = self.conn()?;
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO messages (id, conversation_id, role, content_json, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![id, conversation_id, role, content_json, now],
        )
        .context("Failed to insert message")?;

        // Update conversation's updated_at timestamp.
        conn.execute(
            "UPDATE conversations SET updated_at = ?1 WHERE id = ?2",
            params![now, conversation_id],
        )
        .context("Failed to update conversation timestamp")?;

        Ok(())
    }

    /// Get all messages in a conversation, ordered chronologically.
    pub fn get_messages(&self, conversation_id: &str) -> Result<Vec<serde_json::Value>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT id, conversation_id, role, content_json, created_at
                 FROM messages
                 WHERE conversation_id = ?1
                 ORDER BY created_at ASC",
            )
            .context("Failed to prepare get_messages")?;

        let rows = stmt
            .query_map(params![conversation_id], |row| {
                let content_str: String = row.get(3)?;
                let content: serde_json::Value =
                    serde_json::from_str(&content_str).unwrap_or(serde_json::json!({}));
                Ok(serde_json::json!({
                    "id": row.get::<_, String>(0)?,
                    "conversationId": row.get::<_, String>(1)?,
                    "role": row.get::<_, String>(2)?,
                    "content": content,
                    "createdAt": row.get::<_, String>(4)?,
                }))
            })
            .context("Failed to query messages")?;

        let mut result = Vec::new();
        for row in rows {
            result.push(row.context("Failed to read message row")?);
        }
        Ok(result)
    }

    /// Get the most recent N messages for a conversation (SQL LIMIT).
    ///
    /// Uses a subquery with `ORDER BY created_at DESC LIMIT ?` to fetch only
    /// the last `limit` rows, then re-sorts ascending. This avoids loading
    /// the full message history for long conversations.
    pub fn get_recent_messages(
        &self,
        conversation_id: &str,
        limit: u32,
    ) -> Result<Vec<serde_json::Value>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT id, conversation_id, role, content_json, created_at
                 FROM (
                     SELECT id, conversation_id, role, content_json, created_at
                     FROM messages
                     WHERE conversation_id = ?1
                     ORDER BY created_at DESC
                     LIMIT ?2
                 )
                 ORDER BY created_at ASC",
            )
            .context("Failed to prepare get_recent_messages")?;

        let rows = stmt
            .query_map(params![conversation_id, limit], |row| {
                let content_str: String = row.get(3)?;
                let content: serde_json::Value =
                    serde_json::from_str(&content_str).unwrap_or(serde_json::json!({}));
                Ok(serde_json::json!({
                    "id": row.get::<_, String>(0)?,
                    "conversationId": row.get::<_, String>(1)?,
                    "role": row.get::<_, String>(2)?,
                    "content": content,
                    "createdAt": row.get::<_, String>(4)?,
                }))
            })
            .context("Failed to query recent messages")?;

        let mut result = Vec::new();
        for row in rows {
            result.push(row.context("Failed to read message row")?);
        }
        Ok(result)
    }

    /// Update the content of an existing message (e.g. streaming append).
    pub fn update_message_content(&self, id: &str, content_json: &str) -> Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "UPDATE messages SET content_json = ?1 WHERE id = ?2",
            params![content_json, id],
        )
        .context("Failed to update message content")?;
        Ok(())
    }

    // ───────────────────────────────────────────────
    // Settings
    // ───────────────────────────────────────────────

    /// Get a single setting value.
    pub fn get_setting(&self, key: &str) -> Result<Option<String>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare("SELECT value FROM settings WHERE key = ?1")
            .context("Failed to prepare get_setting")?;

        let result = stmt
            .query_row(params![key], |row| row.get::<_, String>(0))
            .optional()
            .context("Failed to query setting")?;

        Ok(result)
    }

    /// Upsert a setting.
    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )
        .context("Failed to set setting")?;
        Ok(())
    }

    /// Get all settings as a HashMap.
    pub fn get_all_settings(&self) -> Result<HashMap<String, String>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare("SELECT key, value FROM settings")
            .context("Failed to prepare get_all_settings")?;

        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .context("Failed to query settings")?;

        let mut map = HashMap::new();
        for row in rows {
            let (k, v) = row.context("Failed to read settings row")?;
            map.insert(k, v);
        }
        Ok(map)
    }

    /// Get all settings whose key starts with the given prefix.
    pub fn get_settings_by_prefix(&self, prefix: &str) -> Result<HashMap<String, String>> {
        let conn = self.conn()?;
        let like_pattern = format!("{}%", prefix);
        let mut stmt = conn
            .prepare("SELECT key, value FROM settings WHERE key LIKE ?1")
            .context("Failed to prepare get_settings_by_prefix")?;

        let rows = stmt
            .query_map(params![like_pattern], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .context("Failed to query settings by prefix")?;

        let mut map = HashMap::new();
        for row in rows {
            let (k, v) = row.context("Failed to read settings row")?;
            map.insert(k, v);
        }
        Ok(map)
    }

    /// Delete a setting by key.
    pub fn delete_setting(&self, key: &str) -> Result<()> {
        let conn = self.conn()?;
        conn.execute("DELETE FROM settings WHERE key = ?1", params![key])
            .context("Failed to delete setting")?;
        Ok(())
    }

    // ───────────────────────────────────────────────
    // Generated Files
    // ───────────────────────────────────────────────

    /// Insert a generated file record.
    #[allow(clippy::too_many_arguments)]
    pub fn insert_generated_file(
        &self,
        id: &str,
        conversation_id: &str,
        message_id: Option<&str>,
        file_name: &str,
        stored_path: &str,
        file_type: &str,
        file_size: i64,
        category: &str,
        description: Option<&str>,
        version: i32,
        is_latest: bool,
        superseded_by: Option<&str>,
        created_by_step: Option<i32>,
        expires_at: Option<&str>,
    ) -> Result<()> {
        let conn = self.conn()?;
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO generated_files
             (id, conversation_id, message_id, file_name, stored_path, file_type,
              file_size, category, description, version, is_latest, superseded_by,
              created_by_step, created_at, expires_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
            params![
                id,
                conversation_id,
                message_id,
                file_name,
                stored_path,
                file_type,
                file_size,
                category,
                description,
                version,
                is_latest,
                superseded_by,
                created_by_step,
                now,
                expires_at,
            ],
        )
        .context("Failed to insert generated file")?;
        Ok(())
    }

    /// Get all latest generated files for a conversation.
    pub fn get_generated_files_for_conversation(
        &self,
        conversation_id: &str,
    ) -> Result<Vec<serde_json::Value>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT id, conversation_id, message_id, file_name, stored_path,
                        file_type, file_size, category, description, version,
                        is_latest, superseded_by, created_by_step, created_at, expires_at
                 FROM generated_files
                 WHERE conversation_id = ?1 AND is_latest = 1
                 ORDER BY created_at DESC",
            )
            .context("Failed to prepare get_generated_files_for_conversation")?;

        let rows = stmt
            .query_map(params![conversation_id], |row| {
                Ok(serde_json::json!({
                    "id": row.get::<_, String>(0)?,
                    "conversationId": row.get::<_, String>(1)?,
                    "messageId": row.get::<_, Option<String>>(2)?,
                    "fileName": row.get::<_, String>(3)?,
                    "storedPath": row.get::<_, String>(4)?,
                    "fileType": row.get::<_, String>(5)?,
                    "fileSize": row.get::<_, i64>(6)?,
                    "category": row.get::<_, String>(7)?,
                    "description": row.get::<_, Option<String>>(8)?,
                    "version": row.get::<_, i32>(9)?,
                    "isLatest": row.get::<_, bool>(10)?,
                    "supersededBy": row.get::<_, Option<String>>(11)?,
                    "createdByStep": row.get::<_, Option<i32>>(12)?,
                    "createdAt": row.get::<_, String>(13)?,
                    "expiresAt": row.get::<_, Option<String>>(14)?,
                }))
            })
            .context("Failed to query generated files")?;

        let mut result = Vec::new();
        for row in rows {
            result.push(row.context("Failed to read generated file row")?);
        }
        Ok(result)
    }

    /// Get a single generated file by ID, verified to belong to the given conversation.
    pub fn get_generated_file_for_conversation(
        &self,
        id: &str,
        conversation_id: &str,
    ) -> Result<Option<serde_json::Value>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT id, conversation_id, message_id, file_name, stored_path,
                        file_type, file_size, category, description, version,
                        is_latest, superseded_by, created_by_step, created_at, expires_at
                 FROM generated_files
                 WHERE id = ?1 AND conversation_id = ?2",
            )
            .context("Failed to prepare get_generated_file_for_conversation")?;

        let result = stmt
            .query_row(params![id, conversation_id], |row| {
                Ok(serde_json::json!({
                    "id": row.get::<_, String>(0)?,
                    "conversationId": row.get::<_, String>(1)?,
                    "messageId": row.get::<_, Option<String>>(2)?,
                    "fileName": row.get::<_, String>(3)?,
                    "storedPath": row.get::<_, String>(4)?,
                    "fileType": row.get::<_, String>(5)?,
                    "fileSize": row.get::<_, i64>(6)?,
                    "category": row.get::<_, String>(7)?,
                    "description": row.get::<_, Option<String>>(8)?,
                    "version": row.get::<_, i32>(9)?,
                    "isLatest": row.get::<_, bool>(10)?,
                    "supersededBy": row.get::<_, Option<String>>(11)?,
                    "createdByStep": row.get::<_, Option<i32>>(12)?,
                    "createdAt": row.get::<_, String>(13)?,
                    "expiresAt": row.get::<_, Option<String>>(14)?,
                }))
            })
            .optional()
            .context("Failed to query generated file for conversation")?;

        Ok(result)
    }

    /// Mark an existing file as superseded by a newer version.
    pub fn mark_file_superseded(&self, old_id: &str, new_id: &str) -> Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "UPDATE generated_files SET is_latest = 0, superseded_by = ?1 WHERE id = ?2",
            params![new_id, old_id],
        )
        .context("Failed to mark file superseded")?;
        Ok(())
    }

    /// Find temporary files that have expired.
    pub fn find_expired_temp_files(&self) -> Result<Vec<serde_json::Value>> {
        let conn = self.conn()?;
        let now = Utc::now().to_rfc3339();
        let mut stmt = conn
            .prepare(
                "SELECT id, conversation_id, file_name, stored_path, file_type,
                        file_size, category, expires_at
                 FROM generated_files
                 WHERE category = 'temp' AND expires_at IS NOT NULL AND expires_at < ?1",
            )
            .context("Failed to prepare find_expired_temp_files")?;

        let rows = stmt
            .query_map(params![now], |row| {
                Ok(serde_json::json!({
                    "id": row.get::<_, String>(0)?,
                    "conversationId": row.get::<_, String>(1)?,
                    "fileName": row.get::<_, String>(2)?,
                    "storedPath": row.get::<_, String>(3)?,
                    "fileType": row.get::<_, String>(4)?,
                    "fileSize": row.get::<_, i64>(5)?,
                    "category": row.get::<_, String>(6)?,
                    "expiresAt": row.get::<_, String>(7)?,
                }))
            })
            .context("Failed to query expired temp files")?;

        let mut result = Vec::new();
        for row in rows {
            result.push(row.context("Failed to read expired temp file row")?);
        }
        Ok(result)
    }

    /// Delete a generated file record.
    pub fn delete_generated_file(&self, id: &str) -> Result<()> {
        let conn = self.conn()?;
        conn.execute("DELETE FROM generated_files WHERE id = ?1", params![id])
            .context("Failed to delete generated file")?;
        Ok(())
    }

    // ───────────────────────────────────────────────
    // Uploaded Files
    // ───────────────────────────────────────────────

    /// Insert an uploaded file record.
    #[allow(clippy::too_many_arguments)]
    pub fn insert_uploaded_file(
        &self,
        id: &str,
        conversation_id: &str,
        original_name: &str,
        stored_path: &str,
        file_type: &str,
        file_size: i64,
        parsed_summary: Option<&str>,
    ) -> Result<()> {
        let conn = self.conn()?;
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO uploaded_files
             (id, conversation_id, original_name, stored_path, file_type, file_size,
              parsed_summary, uploaded_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![id, conversation_id, original_name, stored_path, file_type, file_size, parsed_summary, now],
        )
        .context("Failed to insert uploaded file")?;
        Ok(())
    }

    /// Get a single uploaded file by ID.
    pub fn get_uploaded_file(&self, id: &str) -> Result<Option<serde_json::Value>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT id, conversation_id, original_name, stored_path, file_type,
                        file_size, parsed_summary, uploaded_at
                 FROM uploaded_files
                 WHERE id = ?1",
            )
            .context("Failed to prepare get_uploaded_file")?;

        let result = stmt
            .query_row(params![id], |row| {
                Ok(serde_json::json!({
                    "id": row.get::<_, String>(0)?,
                    "conversationId": row.get::<_, String>(1)?,
                    "originalName": row.get::<_, String>(2)?,
                    "storedPath": row.get::<_, String>(3)?,
                    "fileType": row.get::<_, String>(4)?,
                    "fileSize": row.get::<_, i64>(5)?,
                    "parsedSummary": row.get::<_, Option<String>>(6)?,
                    "uploadedAt": row.get::<_, String>(7)?,
                }))
            })
            .optional()
            .context("Failed to query uploaded file")?;

        Ok(result)
    }

    /// Get multiple uploaded files by their IDs in a single query.
    ///
    /// More efficient than calling `get_uploaded_file()` in a loop when
    /// processing multiple file attachments.
    pub fn get_uploaded_files_by_ids(&self, ids: &[String]) -> Result<Vec<serde_json::Value>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let conn = self.conn()?;
        // Build a parameterized IN clause: WHERE id IN (?1, ?2, ...)
        let placeholders: Vec<String> = (1..=ids.len()).map(|i| format!("?{}", i)).collect();
        let sql = format!(
            "SELECT id, conversation_id, original_name, stored_path, file_type,
                    file_size, parsed_summary, uploaded_at
             FROM uploaded_files
             WHERE id IN ({})",
            placeholders.join(", ")
        );
        let mut stmt = conn.prepare(&sql).context("Failed to prepare get_uploaded_files_by_ids")?;

        let params: Vec<&dyn rusqlite::types::ToSql> = ids.iter().map(|s| s as &dyn rusqlite::types::ToSql).collect();
        let rows = stmt
            .query_map(params.as_slice(), |row| {
                Ok(serde_json::json!({
                    "id": row.get::<_, String>(0)?,
                    "conversationId": row.get::<_, String>(1)?,
                    "originalName": row.get::<_, String>(2)?,
                    "storedPath": row.get::<_, String>(3)?,
                    "fileType": row.get::<_, String>(4)?,
                    "fileSize": row.get::<_, i64>(5)?,
                    "parsedSummary": row.get::<_, Option<String>>(6)?,
                    "uploadedAt": row.get::<_, String>(7)?,
                }))
            })
            .context("Failed to query uploaded files by IDs")?;

        let mut result = Vec::new();
        for row in rows {
            result.push(row.context("Failed to read uploaded file row")?);
        }
        Ok(result)
    }

    /// Get an uploaded file by ID, verified to belong to the given conversation.
    pub fn get_uploaded_file_for_conversation(
        &self,
        id: &str,
        conversation_id: &str,
    ) -> Result<Option<serde_json::Value>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT id, conversation_id, original_name, stored_path, file_type,
                        file_size, parsed_summary, uploaded_at
                 FROM uploaded_files
                 WHERE id = ?1 AND conversation_id = ?2",
            )
            .context("Failed to prepare get_uploaded_file_for_conversation")?;

        let result = stmt
            .query_row(params![id, conversation_id], |row| {
                Ok(serde_json::json!({
                    "id": row.get::<_, String>(0)?,
                    "conversationId": row.get::<_, String>(1)?,
                    "originalName": row.get::<_, String>(2)?,
                    "storedPath": row.get::<_, String>(3)?,
                    "fileType": row.get::<_, String>(4)?,
                    "fileSize": row.get::<_, i64>(5)?,
                    "parsedSummary": row.get::<_, Option<String>>(6)?,
                    "uploadedAt": row.get::<_, String>(7)?,
                }))
            })
            .optional()
            .context("Failed to query uploaded file for conversation")?;

        Ok(result)
    }

    /// Get all uploaded files for a conversation.
    pub fn get_uploaded_files_for_conversation(
        &self,
        conversation_id: &str,
    ) -> Result<Vec<serde_json::Value>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT id, conversation_id, original_name, stored_path, file_type,
                        file_size, parsed_summary, uploaded_at
                 FROM uploaded_files
                 WHERE conversation_id = ?1
                 ORDER BY uploaded_at DESC",
            )
            .context("Failed to prepare get_uploaded_files_for_conversation")?;

        let rows = stmt
            .query_map(params![conversation_id], |row| {
                Ok(serde_json::json!({
                    "id": row.get::<_, String>(0)?,
                    "conversationId": row.get::<_, String>(1)?,
                    "originalName": row.get::<_, String>(2)?,
                    "storedPath": row.get::<_, String>(3)?,
                    "fileType": row.get::<_, String>(4)?,
                    "fileSize": row.get::<_, i64>(5)?,
                    "parsedSummary": row.get::<_, Option<String>>(6)?,
                    "uploadedAt": row.get::<_, String>(7)?,
                }))
            })
            .context("Failed to query uploaded files for conversation")?;

        let mut results = Vec::new();
        for row in rows {
            results.push(row?);
        }
        Ok(results)
    }

    // ───────────────────────────────────────────────
    // Analysis States
    // ───────────────────────────────────────────────

    /// Upsert the analysis state for a conversation.
    pub fn upsert_analysis_state(
        &self,
        conversation_id: &str,
        current_step: i32,
        step_status: &str,
        state_data: &str,
    ) -> Result<()> {
        let conn = self.conn()?;
        let now = Utc::now().to_rfc3339();
        let id = Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO analysis_states (id, conversation_id, current_step, step_status, state_data, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(conversation_id) DO UPDATE SET
                 current_step = excluded.current_step,
                 step_status  = excluded.step_status,
                 state_data   = excluded.state_data,
                 updated_at   = excluded.updated_at",
            params![id, conversation_id, current_step, step_status, state_data, now],
        )
        .context("Failed to upsert analysis state")?;
        Ok(())
    }

    /// Get the analysis state for a conversation.
    pub fn get_analysis_state(&self, conversation_id: &str) -> Result<Option<serde_json::Value>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT id, conversation_id, current_step, step_status, state_data, updated_at
                 FROM analysis_states
                 WHERE conversation_id = ?1",
            )
            .context("Failed to prepare get_analysis_state")?;

        let result = stmt
            .query_row(params![conversation_id], |row| {
                let step_status_str: String = row.get(3)?;
                let state_data_str: String = row.get(4)?;
                let step_status: serde_json::Value =
                    serde_json::from_str(&step_status_str).unwrap_or(serde_json::json!({}));
                let state_data: serde_json::Value =
                    serde_json::from_str(&state_data_str).unwrap_or(serde_json::json!({}));
                Ok(serde_json::json!({
                    "id": row.get::<_, String>(0)?,
                    "conversationId": row.get::<_, String>(1)?,
                    "currentStep": row.get::<_, i32>(2)?,
                    "stepStatus": step_status,
                    "stateData": state_data,
                    "updatedAt": row.get::<_, String>(5)?,
                }))
            })
            .optional()
            .context("Failed to query analysis state")?;

        Ok(result)
    }

    // ───────────────────────────────────────────────
    // Audit Log
    // ───────────────────────────────────────────────

    /// Append an entry to the audit log.
    pub fn log_action(&self, action: &str, detail: Option<&str>) -> Result<()> {
        let conn = self.conn()?;
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO audit_log (action, detail, created_at) VALUES (?1, ?2, ?3)",
            params![action, detail, now],
        )
        .context("Failed to insert audit log")?;
        Ok(())
    }

    // ───────────────────────────────────────────────
    // Enterprise Memory
    // ───────────────────────────────────────────────

    /// Get a value from enterprise memory by key.
    pub fn get_memory(&self, key: &str) -> Result<Option<String>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare("SELECT value FROM enterprise_memory WHERE key = ?1")
            .context("Failed to prepare get_memory")?;

        let result = stmt
            .query_row(params![key], |row| row.get::<_, String>(0))
            .optional()
            .context("Failed to query enterprise memory")?;

        Ok(result)
    }

    /// Upsert a value in enterprise memory.
    pub fn set_memory(&self, key: &str, value: &str, source: Option<&str>) -> Result<()> {
        let conn = self.conn()?;
        let now = Utc::now().to_rfc3339();
        let id = Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO enterprise_memory (id, key, value, source, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(key) DO UPDATE SET
                 value      = excluded.value,
                 source     = excluded.source,
                 updated_at = excluded.updated_at",
            params![id, key, value, source, now, now],
        )
        .context("Failed to set enterprise memory")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn test_db() -> (Database, TempDir) {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.db");
        let db = Database::new(&path).unwrap();
        (db, dir)
    }

    #[test]
    fn test_conversation_crud() {
        let (db, _dir) = test_db();

        db.create_conversation("c1", "Test Conversation").unwrap();
        let convs = db.get_conversations().unwrap();
        assert_eq!(convs.len(), 1);
        assert_eq!(convs[0]["title"], "Test Conversation");

        db.delete_conversation("c1").unwrap();
        let convs = db.get_conversations().unwrap();
        assert_eq!(convs.len(), 0);
    }

    #[test]
    fn test_message_crud() {
        let (db, _dir) = test_db();

        db.create_conversation("c1", "Conv").unwrap();
        db.insert_message("m1", "c1", "user", r#"{"text":"hello"}"#)
            .unwrap();
        db.insert_message("m2", "c1", "assistant", r#"{"text":"hi"}"#)
            .unwrap();

        let msgs = db.get_messages("c1").unwrap();
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0]["role"], "user");
        assert_eq!(msgs[1]["content"]["text"], "hi");

        db.update_message_content("m1", r#"{"text":"updated"}"#)
            .unwrap();
        let msgs = db.get_messages("c1").unwrap();
        assert_eq!(msgs[0]["content"]["text"], "updated");
    }

    #[test]
    fn test_settings() {
        let (db, _dir) = test_db();

        assert_eq!(db.get_setting("theme").unwrap(), None);
        db.set_setting("theme", "dark").unwrap();
        assert_eq!(db.get_setting("theme").unwrap(), Some("dark".to_string()));

        db.set_setting("theme", "light").unwrap();
        assert_eq!(db.get_setting("theme").unwrap(), Some("light".to_string()));

        db.set_setting("lang", "en").unwrap();
        let all = db.get_all_settings().unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all["lang"], "en");
    }

    #[test]
    fn test_generated_files() {
        let (db, _dir) = test_db();
        db.create_conversation("c1", "Conv").unwrap();

        db.insert_generated_file(
            "f1", "c1", None, "report.pdf", "/tmp/report.pdf", "pdf", 1024, "report",
            Some("Monthly report"), 1, true, None, Some(3), None,
        )
        .unwrap();

        let files = db.get_generated_files_for_conversation("c1").unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0]["fileName"], "report.pdf");

        // Supersede the file
        db.insert_generated_file(
            "f2", "c1", None, "report_v2.pdf", "/tmp/report_v2.pdf", "pdf", 2048, "report",
            Some("Monthly report v2"), 2, true, None, Some(3), None,
        )
        .unwrap();
        db.mark_file_superseded("f1", "f2").unwrap();

        let files = db.get_generated_files_for_conversation("c1").unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0]["id"], "f2");

        db.delete_generated_file("f2").unwrap();
        let files = db.get_generated_files_for_conversation("c1").unwrap();
        assert_eq!(files.len(), 0);
    }

    #[test]
    fn test_uploaded_files() {
        let (db, _dir) = test_db();
        db.create_conversation("c1", "Conv").unwrap();

        db.insert_uploaded_file(
            "uf1", "c1", "data.csv", "/tmp/data.csv", "csv", 512, Some("100 rows of sales data"),
        )
        .unwrap();

        let file = db.get_uploaded_file("uf1").unwrap();
        assert!(file.is_some());
        assert_eq!(file.as_ref().unwrap()["originalName"], "data.csv");

        assert!(db.get_uploaded_file("nonexistent").unwrap().is_none());
    }

    #[test]
    fn test_analysis_state() {
        let (db, _dir) = test_db();
        db.create_conversation("c1", "Conv").unwrap();

        assert!(db.get_analysis_state("c1").unwrap().is_none());

        db.upsert_analysis_state("c1", 2, r#"{"step1":"done"}"#, r#"{"key":"val"}"#)
            .unwrap();

        let state = db.get_analysis_state("c1").unwrap().unwrap();
        assert_eq!(state["currentStep"], 2);
        assert_eq!(state["stepStatus"]["step1"], "done");

        // Upsert again (update path)
        db.upsert_analysis_state("c1", 3, r#"{"step2":"done"}"#, r#"{}"#)
            .unwrap();
        let state = db.get_analysis_state("c1").unwrap().unwrap();
        assert_eq!(state["currentStep"], 3);
    }

    #[test]
    fn test_audit_log() {
        let (db, _dir) = test_db();
        db.log_action("conversation_created", Some("id=c1"))
            .unwrap();
        db.log_action("file_deleted", None).unwrap();
        // No getter — just verify it doesn't panic.
    }

    #[test]
    fn test_enterprise_memory() {
        let (db, _dir) = test_db();

        assert_eq!(db.get_memory("company_name").unwrap(), None);

        db.set_memory("company_name", "Acme Corp", Some("onboarding"))
            .unwrap();
        assert_eq!(
            db.get_memory("company_name").unwrap(),
            Some("Acme Corp".to_string())
        );

        // Update
        db.set_memory("company_name", "Acme Inc", None).unwrap();
        assert_eq!(
            db.get_memory("company_name").unwrap(),
            Some("Acme Inc".to_string())
        );
    }

    // -- Cascade delete: conversation → messages -------------------------------

    #[test]
    fn test_cascade_delete_conversation_removes_messages() {
        let (db, _dir) = test_db();

        db.create_conversation("c1", "Conv").unwrap();
        db.insert_message("m1", "c1", "user", r#"{"text":"hello"}"#)
            .unwrap();
        db.insert_message("m2", "c1", "assistant", r#"{"text":"hi"}"#)
            .unwrap();

        // Verify messages exist
        let msgs = db.get_messages("c1").unwrap();
        assert_eq!(msgs.len(), 2);

        // Delete the conversation
        db.delete_conversation("c1").unwrap();

        // Messages should be gone (cascade delete)
        let msgs = db.get_messages("c1").unwrap();
        assert_eq!(msgs.len(), 0);
    }

    // -- File conversation isolation -------------------------------------------

    #[test]
    fn test_uploaded_file_conversation_isolation() {
        let (db, _dir) = test_db();

        db.create_conversation("c1", "Conv 1").unwrap();
        db.create_conversation("c2", "Conv 2").unwrap();

        db.insert_uploaded_file(
            "uf1", "c1", "data1.csv", "/tmp/data1.csv", "csv", 100, None,
        )
        .unwrap();
        db.insert_uploaded_file(
            "uf2", "c2", "data2.csv", "/tmp/data2.csv", "csv", 200, None,
        )
        .unwrap();

        // Correct conversation_id should return the file
        let file = db.get_uploaded_file_for_conversation("uf1", "c1").unwrap();
        assert!(file.is_some());
        assert_eq!(file.unwrap()["originalName"], "data1.csv");

        // Wrong conversation_id should return None (isolation)
        let file = db.get_uploaded_file_for_conversation("uf1", "c2").unwrap();
        assert!(file.is_none(), "File from c1 should not be visible in c2");

        // get_uploaded_files_for_conversation returns only that conversation's files
        let c1_files = db.get_uploaded_files_for_conversation("c1").unwrap();
        assert_eq!(c1_files.len(), 1);
        assert_eq!(c1_files[0]["originalName"], "data1.csv");

        let c2_files = db.get_uploaded_files_for_conversation("c2").unwrap();
        assert_eq!(c2_files.len(), 1);
        assert_eq!(c2_files[0]["originalName"], "data2.csv");
    }

    // -- Generated file conversation isolation ----------------------------------

    #[test]
    fn test_generated_file_conversation_isolation() {
        let (db, _dir) = test_db();

        db.create_conversation("c1", "Conv 1").unwrap();
        db.create_conversation("c2", "Conv 2").unwrap();

        db.insert_generated_file(
            "gf1", "c1", None, "report.html", "reports/report.html",
            "html", 5000, "report", Some("Test report"), 1, true, None, Some(5), None,
        ).unwrap();

        // Correct conversation_id should return the file
        let file = db.get_generated_file_for_conversation("gf1", "c1").unwrap();
        assert!(file.is_some());
        assert_eq!(file.as_ref().unwrap()["fileName"], "report.html");
        assert_eq!(file.as_ref().unwrap()["storedPath"], "reports/report.html");

        // Wrong conversation_id should return None (isolation)
        let file = db.get_generated_file_for_conversation("gf1", "c2").unwrap();
        assert!(file.is_none(), "Generated file from c1 should not be visible in c2");

        // Non-existent file should return None
        let file = db.get_generated_file_for_conversation("nonexistent", "c1").unwrap();
        assert!(file.is_none());
    }

    // -- Conversation title update ---------------------------------------------

    #[test]
    fn test_update_conversation_title() {
        let (db, _dir) = test_db();

        db.create_conversation("c1", "Original Title").unwrap();
        let convs = db.get_conversations().unwrap();
        assert_eq!(convs[0]["title"], "Original Title");

        db.update_conversation_title("c1", "Updated Title").unwrap();
        let convs = db.get_conversations().unwrap();
        assert_eq!(convs[0]["title"], "Updated Title");
    }

    // -- Multiple conversations ------------------------------------------------

    #[test]
    fn test_multiple_conversations_ordered_by_update() {
        let (db, _dir) = test_db();

        db.create_conversation("c1", "First").unwrap();
        db.create_conversation("c2", "Second").unwrap();
        db.create_conversation("c3", "Third").unwrap();

        let convs = db.get_conversations().unwrap();
        assert_eq!(convs.len(), 3);
        // Most recent should be first
        assert_eq!(convs[0]["title"], "Third");
    }

    // -- File version supersession chain (v1 → v2 → v3) -----------------------

    #[test]
    fn test_file_supersession_chain() {
        let (db, _dir) = test_db();
        db.create_conversation("c1", "Conv").unwrap();

        // v1 (use 'data' category — DB CHECK constraint only allows: report, chart, data, temp, other)
        db.insert_generated_file(
            "f1", "c1", None, "cleaned.csv", "/analysis/cleaned.csv", "csv",
            1000, "data", Some("v1"), 1, true, None, Some(1), None,
        )
        .unwrap();

        // v2 supersedes v1
        db.insert_generated_file(
            "f2", "c1", None, "cleaned.csv", "/analysis/cleaned_v2.csv", "csv",
            1500, "data", Some("v2"), 2, true, None, Some(1), None,
        )
        .unwrap();
        db.mark_file_superseded("f1", "f2").unwrap();

        // v3 supersedes v2
        db.insert_generated_file(
            "f3", "c1", None, "cleaned.csv", "/analysis/cleaned_v3.csv", "csv",
            2000, "data", Some("v3"), 3, true, None, Some(1), None,
        )
        .unwrap();
        db.mark_file_superseded("f2", "f3").unwrap();

        // Only the latest should be returned
        let files = db.get_generated_files_for_conversation("c1").unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0]["id"], "f3");
        assert_eq!(files[0]["version"], 3);
    }

    // -- Generated files isolation by conversation_id --------------------------

    #[test]
    fn test_generated_files_conversation_isolation() {
        let (db, _dir) = test_db();
        db.create_conversation("c1", "Conv 1").unwrap();
        db.create_conversation("c2", "Conv 2").unwrap();

        db.insert_generated_file(
            "f1", "c1", None, "report1.pdf", "/reports/report1.pdf", "pdf",
            1024, "report", Some("Report for c1"), 1, true, None, None, None,
        )
        .unwrap();
        db.insert_generated_file(
            "f2", "c2", None, "report2.pdf", "/reports/report2.pdf", "pdf",
            2048, "report", Some("Report for c2"), 1, true, None, None, None,
        )
        .unwrap();

        let c1_files = db.get_generated_files_for_conversation("c1").unwrap();
        assert_eq!(c1_files.len(), 1);
        assert_eq!(c1_files[0]["fileName"], "report1.pdf");

        let c2_files = db.get_generated_files_for_conversation("c2").unwrap();
        assert_eq!(c2_files.len(), 1);
        assert_eq!(c2_files[0]["fileName"], "report2.pdf");
    }

    // -- Analysis state per conversation ---------------------------------------

    #[test]
    fn test_analysis_state_per_conversation() {
        let (db, _dir) = test_db();
        db.create_conversation("c1", "Conv 1").unwrap();
        db.create_conversation("c2", "Conv 2").unwrap();

        db.upsert_analysis_state("c1", 3, r#"{"step1":"done","step2":"done"}"#, r#"{}"#)
            .unwrap();
        db.upsert_analysis_state("c2", 1, r#"{"step1":"in_progress"}"#, r#"{}"#)
            .unwrap();

        let state1 = db.get_analysis_state("c1").unwrap().unwrap();
        assert_eq!(state1["currentStep"], 3);

        let state2 = db.get_analysis_state("c2").unwrap().unwrap();
        assert_eq!(state2["currentStep"], 1);
    }

    // -- Multiple settings CRUD ------------------------------------------------

    #[test]
    fn test_settings_overwrite() {
        let (db, _dir) = test_db();

        db.set_setting("api_key", "key_1").unwrap();
        assert_eq!(db.get_setting("api_key").unwrap(), Some("key_1".to_string()));

        db.set_setting("api_key", "key_2").unwrap();
        assert_eq!(db.get_setting("api_key").unwrap(), Some("key_2".to_string()));

        // Other settings should not be affected
        db.set_setting("model", "deepseek-v3").unwrap();
        assert_eq!(db.get_setting("api_key").unwrap(), Some("key_2".to_string()));
        assert_eq!(db.get_setting("model").unwrap(), Some("deepseek-v3".to_string()));
    }

    // -- Enterprise memory multiple keys ---------------------------------------

    #[test]
    fn test_enterprise_memory_multiple_keys() {
        let (db, _dir) = test_db();

        db.set_memory("industry", "manufacturing", Some("step2")).unwrap();
        db.set_memory("scale", "1000+", Some("step2")).unwrap();
        db.set_memory("job_families", "7 families", Some("step2")).unwrap();

        assert_eq!(db.get_memory("industry").unwrap(), Some("manufacturing".to_string()));
        assert_eq!(db.get_memory("scale").unwrap(), Some("1000+".to_string()));
        assert_eq!(db.get_memory("job_families").unwrap(), Some("7 families".to_string()));
        assert_eq!(db.get_memory("nonexistent").unwrap(), None);
    }

    // -- Message content JSON structure ----------------------------------------

    #[test]
    fn test_message_content_json_complex() {
        let (db, _dir) = test_db();
        db.create_conversation("c1", "Conv").unwrap();

        let complex_content = r#"{"text":"Analysis result","metrics":[{"id":"m1","label":"Mean","value":"5500"}],"tables":[{"id":"t1","headers":["Name","Salary"],"rows":[["A","5000"]]}]}"#;
        db.insert_message("m1", "c1", "assistant", complex_content).unwrap();

        let msgs = db.get_messages("c1").unwrap();
        assert_eq!(msgs[0]["content"]["text"], "Analysis result");
        assert_eq!(msgs[0]["content"]["metrics"][0]["label"], "Mean");
        assert_eq!(msgs[0]["content"]["tables"][0]["headers"][0], "Name");
    }

    // -- Delete conversation that doesn't exist (should not panic) -------------

    #[test]
    fn test_delete_nonexistent_conversation() {
        let (db, _dir) = test_db();
        // Should succeed or at least not panic
        let result = db.delete_conversation("nonexistent");
        assert!(result.is_ok());
    }

    // -- Insert message to nonexistent conversation (FK violation) --------------

    #[test]
    fn test_message_fk_violation() {
        let (db, _dir) = test_db();
        // No conversation created — inserting a message should fail due to FK
        let result = db.insert_message("m1", "no_conv", "user", r#"{"text":"test"}"#);
        assert!(result.is_err(), "Should fail due to foreign key constraint");
    }
}
