use anyhow::Result;
use rusqlite::{Connection, params};
use std::path::PathBuf;
use chrono::Utc;

pub struct ContextStore {
    conn: Connection,
}

impl ContextStore {
    pub fn open(db_path: &PathBuf) -> Result<Self> {
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(db_path)?;
        let store = Self { conn };
        store.initialize_schema()?;
        Ok(store)
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        let store = Self { conn };
        store.initialize_schema()?;
        Ok(store)
    }

    fn initialize_schema(&self) -> Result<()> {
        self.conn.execute_batch("
            CREATE TABLE IF NOT EXISTS sessions (
                id TEXT PRIMARY KEY,
                project_path TEXT NOT NULL,
                created_at TEXT,
                last_active TEXT,
                total_tokens_claude_sonnet INTEGER DEFAULT 0,
                total_tokens_claude_haiku INTEGER DEFAULT 0,
                total_tokens_gemini INTEGER DEFAULT 0,
                total_cost_usd REAL DEFAULT 0.0
            );

            CREATE TABLE IF NOT EXISTS messages (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT,
                role TEXT,
                content TEXT,
                model TEXT,
                tokens_in INTEGER,
                tokens_out INTEGER,
                created_at TEXT,
                FOREIGN KEY(session_id) REFERENCES sessions(id)
            );

            CREATE TABLE IF NOT EXISTS tasks (
                id TEXT PRIMARY KEY,
                session_id TEXT,
                description TEXT,
                task_type TEXT,
                model TEXT,
                status TEXT,
                retry_count INTEGER DEFAULT 0,
                tokens_used INTEGER DEFAULT 0,
                created_at TEXT,
                completed_at TEXT,
                FOREIGN KEY(session_id) REFERENCES sessions(id)
            );

            CREATE TABLE IF NOT EXISTS response_cache (
                hash TEXT PRIMARY KEY,
                response TEXT,
                model TEXT,
                created_at TEXT,
                ttl_hours INTEGER DEFAULT 24
            );

            CREATE TABLE IF NOT EXISTS file_summaries (
                file_path TEXT PRIMARY KEY,
                summary TEXT,
                language TEXT,
                last_modified TEXT,
                analyzed_at TEXT
            );

            CREATE TABLE IF NOT EXISTS agent_checkpoints (
                id TEXT PRIMARY KEY,
                task_id TEXT NOT NULL,
                session_id TEXT NOT NULL,
                agent_type TEXT NOT NULL,
                step TEXT NOT NULL,
                partial_output TEXT,
                files_read TEXT,
                created_at TEXT,
                FOREIGN KEY(task_id) REFERENCES tasks(id)
            );

            CREATE TABLE IF NOT EXISTS prompt_history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL,
                prompt TEXT NOT NULL,
                created_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS manual_context (
                path TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                content TEXT NOT NULL,
                added_at TEXT NOT NULL
            );
        ")?;
        Ok(())
    }

    // ── Sessions ──────────────────────────────────────────────────────────────

    pub fn create_session(&self, id: &str, project_path: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "INSERT INTO sessions (id, project_path, created_at, last_active) VALUES (?1, ?2, ?3, ?3)",
            params![id, project_path, now],
        )?;
        Ok(())
    }

    pub fn touch_session(&self, session_id: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "UPDATE sessions SET last_active = ?1 WHERE id = ?2",
            params![now, session_id],
        )?;
        Ok(())
    }

    pub fn last_session_id(&self) -> Result<Option<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT id FROM sessions ORDER BY last_active DESC LIMIT 1"
        )?;
        let id = stmt.query_row([], |row| row.get(0)).ok();
        Ok(id)
    }

    pub fn update_session_tokens(
        &self, session_id: &str,
        sonnet: i64, haiku: i64, gemini: i64, cost: f64,
    ) -> Result<()> {
        self.conn.execute(
            "UPDATE sessions SET
                total_tokens_claude_sonnet = total_tokens_claude_sonnet + ?1,
                total_tokens_claude_haiku  = total_tokens_claude_haiku  + ?2,
                total_tokens_gemini        = total_tokens_gemini        + ?3,
                total_cost_usd             = total_cost_usd             + ?4
            WHERE id = ?5",
            params![sonnet, haiku, gemini, cost, session_id],
        )?;
        Ok(())
    }

    pub fn get_session_cost(&self, session_id: &str) -> Result<f64> {
        let cost: f64 = self.conn.query_row(
            "SELECT total_cost_usd FROM sessions WHERE id = ?1",
            params![session_id],
            |row| row.get(0),
        )?;
        Ok(cost)
    }

    // ── Messages ──────────────────────────────────────────────────────────────

    pub fn add_message(
        &self, session_id: &str, role: &str, content: &str,
        model: &str, tokens_in: i64, tokens_out: i64,
    ) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "INSERT INTO messages (session_id, role, content, model, tokens_in, tokens_out, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![session_id, role, content, model, tokens_in, tokens_out, now],
        )?;
        Ok(())
    }

    pub fn get_messages(&self, session_id: &str, limit: usize) -> Result<Vec<StoredMessage>> {
        let mut stmt = self.conn.prepare(
            "SELECT role, content, model, tokens_in, tokens_out, created_at
             FROM messages WHERE session_id = ?1 ORDER BY id DESC LIMIT ?2"
        )?;
        let messages = stmt.query_map(params![session_id, limit as i64], |row| {
            Ok(StoredMessage {
                role: row.get(0)?,
                content: row.get(1)?,
                model: row.get(2)?,
                tokens_in: row.get(3)?,
                tokens_out: row.get(4)?,
                created_at: row.get(5)?,
            })
        })?.filter_map(|r| r.ok()).collect();
        Ok(messages)
    }

    pub fn clear_messages(&self, session_id: &str) -> Result<()> {
        self.conn.execute("DELETE FROM messages WHERE session_id = ?1", params![session_id])?;
        Ok(())
    }

    // ── Tasks ─────────────────────────────────────────────────────────────────

    pub fn upsert_task(&self, task: &StoredTask) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "INSERT OR REPLACE INTO tasks
             (id, session_id, description, task_type, model, status, retry_count, tokens_used, created_at, completed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                task.id, task.session_id, task.description,
                task.task_type, task.model, task.status,
                task.retry_count, task.tokens_used,
                task.created_at.as_deref().unwrap_or(&now),
                task.completed_at,
            ],
        )?;
        Ok(())
    }

    pub fn get_tasks(&self, session_id: &str) -> Result<Vec<StoredTask>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, session_id, description, task_type, model, status, retry_count, tokens_used, created_at, completed_at
             FROM tasks WHERE session_id = ?1 ORDER BY rowid"
        )?;
        let tasks = stmt.query_map(params![session_id], |row| {
            Ok(StoredTask {
                id: row.get(0)?,
                session_id: row.get(1)?,
                description: row.get(2)?,
                task_type: row.get(3)?,
                model: row.get(4)?,
                status: row.get(5)?,
                retry_count: row.get(6)?,
                tokens_used: row.get(7)?,
                created_at: row.get(8)?,
                completed_at: row.get(9)?,
            })
        })?.filter_map(|r| r.ok()).collect();
        Ok(tasks)
    }

    // ── Response cache ────────────────────────────────────────────────────────

    pub fn cache_get(&self, hash: &str) -> Result<Option<String>> {
        let result = self.conn.query_row(
            "SELECT response, created_at, ttl_hours FROM response_cache WHERE hash = ?1",
            params![hash],
            |row| {
                let response: String = row.get(0)?;
                let created_at: String = row.get(1)?;
                let ttl_hours: i64 = row.get(2)?;
                Ok((response, created_at, ttl_hours))
            },
        );

        match result {
            Ok((response, created_at, ttl_hours)) => {
                if let Ok(created) = chrono::DateTime::parse_from_rfc3339(&created_at) {
                    let age = Utc::now().signed_duration_since(created.with_timezone(&Utc));
                    if age.num_hours() < ttl_hours {
                        return Ok(Some(response));
                    }
                    self.conn.execute("DELETE FROM response_cache WHERE hash = ?1", params![hash])?;
                }
                Ok(None)
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub fn cache_set(&self, hash: &str, response: &str, model: &str, ttl_hours: i64) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "INSERT OR REPLACE INTO response_cache (hash, response, model, created_at, ttl_hours)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![hash, response, model, now, ttl_hours],
        )?;
        Ok(())
    }

    pub fn cache_clear(&self) -> Result<()> {
        self.conn.execute("DELETE FROM response_cache", [])?;
        Ok(())
    }

    // ── File summaries ────────────────────────────────────────────────────────

    // ── Agent checkpoints ─────────────────────────────────────────────────────

    pub fn save_checkpoint(&self, id: &str, task_id: &str, session_id: &str, agent_type: &str, step: &str, partial_output: &str, files_read: &[String]) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        let files_json = serde_json::to_string(files_read).unwrap_or_default();
        self.conn.execute(
            "INSERT OR REPLACE INTO agent_checkpoints (id, task_id, session_id, agent_type, step, partial_output, files_read, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![id, task_id, session_id, agent_type, step, partial_output, files_json, now],
        )?;
        Ok(())
    }

    pub fn load_checkpoint(&self, task_id: &str) -> Result<Option<CheckpointData>> {
        let result = self.conn.query_row(
            "SELECT id, task_id, session_id, agent_type, step, partial_output, files_read
             FROM agent_checkpoints WHERE task_id = ?1 ORDER BY rowid DESC LIMIT 1",
            params![task_id],
            |row| Ok(CheckpointData {
                id: row.get(0)?,
                task_id: row.get(1)?,
                session_id: row.get(2)?,
                agent_type: row.get(3)?,
                step: row.get(4)?,
                partial_output: row.get(5)?,
                files_read: row.get(6)?,
            }),
        );
        match result {
            Ok(c) => Ok(Some(c)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub fn delete_checkpoints(&self, task_id: &str) -> Result<()> {
        self.conn.execute("DELETE FROM agent_checkpoints WHERE task_id = ?1", params![task_id])?;
        Ok(())
    }

    // ── Prompt history ────────────────────────────────────────────────────────

    pub fn add_prompt(&self, session_id: &str, prompt: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "INSERT INTO prompt_history (session_id, prompt, created_at) VALUES (?1, ?2, ?3)",
            params![session_id, prompt, now],
        )?;
        Ok(())
    }

    pub fn get_prompts(&self, session_id: &str, limit: usize) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT prompt FROM prompt_history WHERE session_id = ?1 ORDER BY id DESC LIMIT ?2"
        )?;
        let prompts = stmt.query_map(params![session_id, limit as i64], |row| row.get(0))?
            .filter_map(|r| r.ok())
            .collect();
        Ok(prompts)
    }

    // ── Manual context ────────────────────────────────────────────────────────

    pub fn add_manual_context(&self, path: &str, session_id: &str, content: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "INSERT OR REPLACE INTO manual_context (path, session_id, content, added_at) VALUES (?1, ?2, ?3, ?4)",
            params![path, session_id, content, now],
        )?;
        Ok(())
    }

    pub fn get_manual_context(&self, session_id: &str) -> Result<Vec<(String, String)>> {
        let mut stmt = self.conn.prepare(
            "SELECT path, content FROM manual_context WHERE session_id = ?1"
        )?;
        let items = stmt.query_map(params![session_id], |row| Ok((row.get(0)?, row.get(1)?)))?
            .filter_map(|r| r.ok())
            .collect();
        Ok(items)
    }

    pub fn clear_manual_context(&self, session_id: &str) -> Result<()> {
        self.conn.execute("DELETE FROM manual_context WHERE session_id = ?1", params![session_id])?;
        Ok(())
    }

    pub fn remove_manual_context(&self, path: &str) -> Result<()> {
        self.conn.execute("DELETE FROM manual_context WHERE path = ?1", params![path])?;
        Ok(())
    }

    pub fn set_file_summary(&self, file_path: &str, summary: &str, language: &str, last_modified: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "INSERT OR REPLACE INTO file_summaries (file_path, summary, language, last_modified, analyzed_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![file_path, summary, language, last_modified, now],
        )?;
        Ok(())
    }

    pub fn get_file_summary(&self, file_path: &str) -> Result<Option<FileSummary>> {
        let result = self.conn.query_row(
            "SELECT file_path, summary, language, last_modified, analyzed_at FROM file_summaries WHERE file_path = ?1",
            params![file_path],
            |row| Ok(FileSummary {
                file_path: row.get(0)?,
                summary: row.get(1)?,
                language: row.get(2)?,
                last_modified: row.get(3)?,
                analyzed_at: row.get(4)?,
            }),
        );
        match result {
            Ok(s) => Ok(Some(s)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }
}

#[derive(Debug, Clone)]
pub struct StoredMessage {
    pub role: String,
    pub content: String,
    pub model: String,
    pub tokens_in: i64,
    pub tokens_out: i64,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct StoredTask {
    pub id: String,
    pub session_id: String,
    pub description: String,
    pub task_type: String,
    pub model: String,
    pub status: String,
    pub retry_count: i64,
    pub tokens_used: i64,
    pub created_at: Option<String>,
    pub completed_at: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CheckpointData {
    pub id: String,
    pub task_id: String,
    pub session_id: String,
    pub agent_type: String,
    pub step: String,
    pub partial_output: String,
    pub files_read: String,
}

#[derive(Debug, Clone)]
pub struct FileSummary {
    pub file_path: String,
    pub summary: String,
    pub language: String,
    pub last_modified: String,
    pub analyzed_at: String,
}
