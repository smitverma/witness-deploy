use std::{
    fs,
    path::{Path, PathBuf},
};

use rusqlite::{params, params_from_iter, types::Value, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    error::{Result, WitnessError},
    history::HistoryEntry,
    scope::{ScopeEntry, ScopeSnapshot},
};

const DATABASE_NAME: &str = "witness.sqlite3";
const CURRENT_SCHEMA_VERSION: i64 = 6;

const MIGRATION_1: &str = r#"
CREATE TABLE IF NOT EXISTS projects (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    path TEXT NOT NULL UNIQUE,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS hosts (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    host TEXT NOT NULL UNIQUE,
    ip TEXT
);
CREATE TABLE IF NOT EXISTS requests (
    id TEXT PRIMARY KEY,
    url TEXT NOT NULL,
    method TEXT NOT NULL,
    host TEXT NOT NULL,
    path TEXT NOT NULL,
    ip TEXT,
    timestamp TEXT NOT NULL,
    headers BLOB NOT NULL,
    body_path TEXT NOT NULL,
    scoped INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS responses (
    id TEXT PRIMARY KEY,
    request_id TEXT NOT NULL UNIQUE REFERENCES requests(id) ON DELETE CASCADE,
    status INTEGER NOT NULL,
    mime_type TEXT NOT NULL,
    duration_ms INTEGER NOT NULL,
    size INTEGER NOT NULL,
    headers BLOB NOT NULL,
    body_path TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS scope (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    pattern TEXT NOT NULL UNIQUE,
    is_regex INTEGER NOT NULL DEFAULT 0,
    enabled INTEGER NOT NULL DEFAULT 1
);
CREATE TABLE IF NOT EXISTS settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS notes (
    id TEXT PRIMARY KEY,
    request_id TEXT REFERENCES requests(id) ON DELETE CASCADE,
    content TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_requests_host ON requests(host);
CREATE INDEX IF NOT EXISTS idx_requests_url ON requests(url);
CREATE INDEX IF NOT EXISTS idx_requests_timestamp ON requests(timestamp DESC);
CREATE INDEX IF NOT EXISTS idx_requests_method ON requests(method);
CREATE INDEX IF NOT EXISTS idx_responses_status ON responses(status);
"#;

const MIGRATION_2: &str = r#"
CREATE TABLE IF NOT EXISTS organizer_folders (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    parent_id TEXT REFERENCES organizer_folders(id) ON DELETE CASCADE,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS organizer_items (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    folder_id TEXT REFERENCES organizer_folders(id) ON DELETE SET NULL,
    request BLOB NOT NULL,
    response BLOB NOT NULL,
    tls INTEGER NOT NULL DEFAULT 1,
    source TEXT NOT NULL,
    method TEXT NOT NULL,
    host TEXT NOT NULL,
    path TEXT NOT NULL,
    status INTEGER,
    notes TEXT NOT NULL DEFAULT '',
    tags TEXT NOT NULL DEFAULT '[]',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_organizer_folders_parent ON organizer_folders(parent_id);
CREATE INDEX IF NOT EXISTS idx_organizer_items_folder ON organizer_items(folder_id);
CREATE INDEX IF NOT EXISTS idx_organizer_items_updated ON organizer_items(updated_at DESC);
"#;

const MIGRATION_3: &str = r#"
CREATE TABLE scope_new (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    pattern TEXT NOT NULL,
    is_regex INTEGER NOT NULL DEFAULT 0,
    include_subdomains INTEGER NOT NULL DEFAULT 0,
    is_in_scope INTEGER NOT NULL DEFAULT 1,
    UNIQUE(pattern, is_regex, include_subdomains, is_in_scope)
);
INSERT INTO scope_new (id, pattern, is_regex, include_subdomains, is_in_scope)
SELECT id, pattern, is_regex, 1, 1 FROM scope;
DROP TABLE scope;
ALTER TABLE scope_new RENAME TO scope;
DELETE FROM settings WHERE key='scope_enabled';
"#;

const MIGRATION_4: &str = r#"
CREATE TABLE IF NOT EXISTS identity_groups (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL COLLATE NOCASE UNIQUE,
    description TEXT NOT NULL DEFAULT '',
    injection_type TEXT NOT NULL CHECK(injection_type IN ('cookie', 'header', 'queryParameter')),
    injection_key TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS identities (
    id TEXT PRIMARY KEY,
    group_id TEXT NOT NULL REFERENCES identity_groups(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    color TEXT NOT NULL DEFAULT '',
    notes TEXT NOT NULL DEFAULT '',
    auth_value TEXT NOT NULL DEFAULT ''
);
CREATE INDEX IF NOT EXISTS idx_identities_group ON identities(group_id);
CREATE INDEX IF NOT EXISTS idx_identity_groups_name ON identity_groups(name COLLATE NOCASE);
"#;

const MIGRATION_5: &str = r#"
ALTER TABLE organizer_items ADD COLUMN stage_id TEXT;
CREATE INDEX IF NOT EXISTS idx_organizer_items_stage ON organizer_items(stage_id);
"#;

const MIGRATION_6: &str = r#"
CREATE TABLE IF NOT EXISTS fuzz_scans (
    id TEXT PRIMARY KEY,
    source_tab_id INTEGER NOT NULL,
    name TEXT NOT NULL CHECK(length(trim(name)) > 0),
    started_at TEXT NOT NULL,
    completed_at TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_fuzz_scans_source_tab ON fuzz_scans(source_tab_id);
CREATE INDEX IF NOT EXISTS idx_fuzz_scans_started ON fuzz_scans(started_at DESC);
"#;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrganizerFolder {
    pub id: String,
    pub name: String,
    pub parent_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrganizerItem {
    pub id: String,
    pub title: String,
    pub folder_id: Option<String>,
    #[serde(default)]
    pub stage_id: Option<String>,
    pub request: Vec<u8>,
    pub response: Vec<u8>,
    pub tls: bool,
    pub source: String,
    pub method: String,
    pub host: String,
    pub path: String,
    pub status: Option<u16>,
    pub notes: String,
    pub tags: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrganizerItemInput {
    pub title: String,
    pub folder_id: Option<String>,
    #[serde(default)]
    pub stage_id: Option<String>,
    pub request: Vec<u8>,
    #[serde(default)]
    pub response: Vec<u8>,
    #[serde(default = "default_true")]
    pub tls: bool,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrganizerBundle {
    pub version: u8,
    #[serde(default)]
    pub folders: Vec<OrganizerFolder>,
    #[serde(default)]
    pub items: Vec<OrganizerItem>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum IdentityInjectionType {
    Cookie,
    Header,
    QueryParameter,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IdentityGroup {
    pub id: String,
    pub name: String,
    pub description: String,
    pub injection_type: IdentityInjectionType,
    pub injection_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IdentityGroupInput {
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub injection_type: IdentityInjectionType,
    pub injection_key: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Identity {
    pub id: String,
    pub group_id: String,
    pub name: String,
    pub color: String,
    pub notes: String,
    pub auth_value: String,
}

impl std::fmt::Debug for Identity {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Identity")
            .field("id", &self.id)
            .field("group_id", &self.group_id)
            .field("name", &self.name)
            .field("color", &self.color)
            .field("notes", &self.notes)
            .field("auth_value", &"[REDACTED]")
            .finish()
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IdentityInput {
    pub group_id: String,
    pub name: String,
    #[serde(default)]
    pub color: String,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub auth_value: String,
}

impl std::fmt::Debug for IdentityInput {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("IdentityInput")
            .field("group_id", &self.group_id)
            .field("name", &self.name)
            .field("color", &self.color)
            .field("notes", &self.notes)
            .field("auth_value", &"[REDACTED]")
            .finish()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IdentityBundle {
    pub version: u8,
    #[serde(default)]
    pub groups: Vec<IdentityGroup>,
    #[serde(default)]
    pub identities: Vec<Identity>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IdentityInjectionDescriptor {
    pub injection_type: IdentityInjectionType,
    pub injection_key: String,
    pub auth_value: String,
}

impl std::fmt::Debug for IdentityInjectionDescriptor {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("IdentityInjectionDescriptor")
            .field("injection_type", &self.injection_type)
            .field("injection_key", &self.injection_key)
            .field("auth_value", &"[REDACTED]")
            .finish()
    }
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestMeta {
    pub id: String,
    pub url: String,
    pub method: String,
    pub host: String,
    pub path: String,
    pub ip: Option<String>,
    pub timestamp: String,
    pub headers: Vec<u8>,
    pub body_path: PathBuf,
    pub scoped: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResponseMeta {
    pub id: String,
    pub request_id: String,
    pub status: u16,
    pub mime_type: String,
    pub duration_ms: u64,
    pub size: usize,
    pub headers: Vec<u8>,
    pub body_path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuzzScanRecord {
    pub id: String,
    pub source_tab_id: i64,
    pub name: String,
    pub started_at: String,
    pub completed_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct HistoryFilter {
    pub method: Option<String>,
    pub host: Option<String>,
    pub status_min: Option<u16>,
    pub status_max: Option<u16>,
    pub mime_type: Option<String>,
    pub search: Option<String>,
    pub in_scope_only: bool,
    pub sort_by: Option<String>,
    pub sort_descending: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryDetail {
    pub entry: HistoryEntry,
    pub request: Vec<u8>,
    pub response: Vec<u8>,
}

#[derive(Debug, Clone, Copy)]
pub enum BodyKind {
    Request,
    Response,
}

pub struct BodyStore {
    project_path: PathBuf,
}

/// NOTE: `BodyStore` and `Database` use blocking `std::fs` / `rusqlite` calls.
/// They are `Send` but not async: callers on the Tauri async runtime must wrap
/// `open`, `get_detail`, `query_history`, `prune_history`, etc. in
/// `tokio::task::spawn_blocking` (see `ui_bridge::get_history_detail`) so the
/// async executor is never stalled by file or SQLite I/O.
impl BodyStore {
    pub fn new(project_path: impl Into<PathBuf>) -> Result<Self> {
        let project_path = project_path.into();
        fs::create_dir_all(project_path.join("bodies/requests"))?;
        fs::create_dir_all(project_path.join("bodies/responses"))?;
        Ok(Self { project_path })
    }

    pub fn path(&self, kind: BodyKind, id: &str) -> PathBuf {
        let directory = match kind {
            BodyKind::Request => "requests",
            BodyKind::Response => "responses",
        };
        self.project_path
            .join("bodies")
            .join(directory)
            .join(format!("{id}.bin"))
    }

    pub fn write_body(&self, kind: BodyKind, id: &str, bytes: &[u8]) -> Result<PathBuf> {
        let path = self.path(kind, id);
        fs::write(&path, bytes)?;
        Ok(path)
    }

    pub fn read_body(&self, kind: BodyKind, id: &str) -> Result<Vec<u8>> {
        Ok(fs::read(self.path(kind, id))?)
    }

    pub fn delete_body(&self, kind: BodyKind, id: &str) -> Result<()> {
        let path = self.path(kind, id);
        if path.exists() {
            fs::remove_file(path)?;
        }
        Ok(())
    }
}

pub struct Database {
    connection: Connection,
    project_path: PathBuf,
    bodies: BodyStore,
}

impl Database {
    /// Opens (or creates) the project database. Blocking: call via
    /// `spawn_blocking` from async contexts.
    pub fn open(project_path: impl Into<PathBuf>) -> Result<Self> {
        let project_path = project_path.into();
        fs::create_dir_all(&project_path)?;
        let connection = Connection::open(project_path.join(DATABASE_NAME))?;
        connection.execute_batch(
            "PRAGMA foreign_keys=ON; PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;",
        )?;
        let mut database = Self {
            connection,
            bodies: BodyStore::new(&project_path)?,
            project_path,
        };
        database.run_migrations()?;
        Ok(database)
    }

    fn run_migrations(&mut self) -> Result<()> {
        let version: i64 = self
            .connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))?;
        if version < 1 {
            let transaction = self.connection.transaction()?;
            transaction.execute_batch(MIGRATION_1)?;
            transaction.pragma_update(None, "user_version", 1)?;
            transaction.commit()?;
        }
        if version < 2 {
            let transaction = self.connection.transaction()?;
            transaction.execute_batch(MIGRATION_2)?;
            transaction.pragma_update(None, "user_version", 2)?;
            transaction.commit()?;
        }
        if version < 3 {
            let transaction = self.connection.transaction()?;
            transaction.execute_batch(MIGRATION_3)?;
            transaction.pragma_update(None, "user_version", 3)?;
            transaction.commit()?;
        }
        if version < 4 {
            let transaction = self.connection.transaction()?;
            transaction.execute_batch(MIGRATION_4)?;
            transaction.pragma_update(None, "user_version", 4)?;
            transaction.commit()?;
        }
        if version < 5 {
            let organizer_items_exists: bool = self.connection.query_row(
                "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE type='table' AND name='organizer_items')",
                [],
                |row| row.get(0),
            )?;
            let organizer_items_has_stage = if organizer_items_exists {
                self.connection.query_row(
                    "SELECT COUNT(*) FROM pragma_table_info('organizer_items') WHERE name='stage_id'",
                    [],
                    |row| row.get::<_, i64>(0),
                )? > 0
            } else {
                false
            };
            let transaction = self.connection.transaction()?;
            if !organizer_items_exists {
                transaction.execute_batch(MIGRATION_2)?;
            }
            if !organizer_items_has_stage {
                transaction.execute_batch(MIGRATION_5)?;
            } else {
                transaction.execute_batch(
                    "CREATE INDEX IF NOT EXISTS idx_organizer_items_stage ON organizer_items(stage_id);",
                )?;
            }
            transaction.pragma_update(None, "user_version", 5)?;
            transaction.commit()?;
        }
        if version < 6 {
            let transaction = self.connection.transaction()?;
            transaction.execute_batch(MIGRATION_6)?;
            transaction.pragma_update(None, "user_version", CURRENT_SCHEMA_VERSION)?;
            transaction.commit()?;
        }
        Ok(())
    }

    pub fn schema_version(&self) -> Result<i64> {
        Ok(self
            .connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))?)
    }

    pub fn register_project(&self, name: &str) -> Result<()> {
        let now = chrono::Utc::now().to_rfc3339();
        self.connection.execute(
            "INSERT INTO projects (id, name, path, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?4)
             ON CONFLICT(path) DO UPDATE SET name=excluded.name, updated_at=excluded.updated_at",
            params![
                Uuid::new_v4().to_string(),
                name,
                self.project_path.display().to_string(),
                now
            ],
        )?;
        Ok(())
    }

    /// Rewrites body references to the directories owned by this database.
    ///
    /// Archive files can be moved between machines, so absolute paths from
    /// the project that created the archive cannot be trusted after extraction.
    /// Body filenames are generated from stable exchange IDs; retaining only
    /// the filename lets an extracted project become portable without changing
    /// the on-disk body layout used by existing projects.
    pub fn rebase_body_paths_to_project(&self) -> Result<()> {
        let request_paths = self
            .connection
            .prepare("SELECT id, body_path FROM requests")?
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let response_paths = self
            .connection
            .prepare("SELECT id, body_path FROM responses")?
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let transaction = self.connection.unchecked_transaction()?;
        let request_root = self
            .project_path
            .join("bodies/requests")
            .display()
            .to_string();
        for (id, path) in request_paths {
            let filename = Path::new(&path)
                .file_name()
                .ok_or_else(|| WitnessError::Database(rusqlite::Error::InvalidQuery))?
                .to_owned();
            transaction.execute(
                "UPDATE requests SET body_path=?2 WHERE id=?1",
                params![
                    id,
                    Path::new(&request_root)
                        .join(filename)
                        .display()
                        .to_string()
                ],
            )?;
        }
        let response_root = self
            .project_path
            .join("bodies/responses")
            .display()
            .to_string();
        for (id, path) in response_paths {
            let filename = Path::new(&path)
                .file_name()
                .ok_or_else(|| WitnessError::Database(rusqlite::Error::InvalidQuery))?
                .to_owned();
            transaction.execute(
                "UPDATE responses SET body_path=?2 WHERE id=?1",
                params![
                    id,
                    Path::new(&response_root)
                        .join(filename)
                        .display()
                        .to_string()
                ],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn insert_exchange(
        &mut self,
        request: &RequestMeta,
        response: &ResponseMeta,
    ) -> Result<()> {
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "INSERT OR IGNORE INTO hosts (host, ip) VALUES (?1, ?2)",
            params![request.host, request.ip],
        )?;
        transaction.execute(
            "INSERT INTO requests
             (id, url, method, host, path, ip, timestamp, headers, body_path, scoped)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                request.id,
                request.url,
                request.method,
                request.host,
                request.path,
                request.ip,
                request.timestamp,
                request.headers,
                request.body_path.display().to_string(),
                request.scoped,
            ],
        )?;
        transaction.execute(
            "INSERT INTO responses
             (id, request_id, status, mime_type, duration_ms, size, headers, body_path)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                response.id,
                response.request_id,
                response.status,
                response.mime_type,
                response.duration_ms as i64,
                response.size as i64,
                response.headers,
                response.body_path.display().to_string(),
            ],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn query_history(
        &self,
        filter: &HistoryFilter,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<HistoryEntry>> {
        let mut sql = String::from(
            "SELECT q.rowid, q.id, q.url, q.method, q.host, q.path, p.status, p.size, p.mime_type,
                    p.duration_ms, q.timestamp, q.scoped
             FROM requests q JOIN responses p ON p.request_id = q.id WHERE 1=1",
        );
        let mut values = Vec::<Value>::new();
        if let Some(method) = filter.method.as_ref().filter(|value| !value.is_empty()) {
            sql.push_str(" AND q.method = ?");
            values.push(method.clone().into());
        }
        if let Some(host) = filter.host.as_ref().filter(|value| !value.is_empty()) {
            sql.push_str(" AND q.host LIKE ? ESCAPE '\\'");
            values.push(format!("%{}%", escape_like(host)).into());
        }
        if let Some(status) = filter.status_min {
            sql.push_str(" AND p.status >= ?");
            values.push(i64::from(status).into());
        }
        if let Some(status) = filter.status_max {
            sql.push_str(" AND p.status <= ?");
            values.push(i64::from(status).into());
        }
        if let Some(mime) = filter.mime_type.as_ref().filter(|value| !value.is_empty()) {
            sql.push_str(" AND p.mime_type LIKE ? ESCAPE '\\'");
            values.push(format!("%{}%", escape_like(mime)).into());
        }
        if filter.in_scope_only {
            sql.push_str(" AND q.scoped = 1");
        }
        if let Some(search) = filter.search.as_ref().filter(|value| !value.is_empty()) {
            sql.push_str(
                " AND (
                    q.url LIKE ? ESCAPE '\\'
                    OR CAST(q.headers AS TEXT) LIKE ? ESCAPE '\\'
                    OR CAST(p.headers AS TEXT) LIKE ? ESCAPE '\\'
                )",
            );
            for _ in 0..3 {
                values.push(format!("%{}%", escape_like(search)).into());
            }
        }
        let sort_column = match filter.sort_by.as_deref() {
            Some("method") => "q.method",
            Some("host") => "q.host",
            Some("path") => "q.path",
            Some("status") => "p.status",
            Some("length") => "p.size",
            Some("mimeType") => "p.mime_type",
            Some("durationMs") => "p.duration_ms",
            _ => "q.timestamp",
        };
        sql.push_str(" ORDER BY ");
        sql.push_str(sort_column);
        sql.push_str(if filter.sort_descending {
            " DESC"
        } else {
            " ASC"
        });
        sql.push_str(" LIMIT ? OFFSET ?");
        values.push((limit as i64).into());
        values.push((offset as i64).into());

        let mut entries = {
            let mut statement = self.connection.prepare_cached(&sql)?;
            let rows = statement.query_map(params_from_iter(values), map_history_entry)?;
            rows.collect::<std::result::Result<Vec<_>, _>>()?
        };
        // N+1 avoidance: only touch body files when a search term is present.
        // Without search there is no snippet to build, so skip per-row
        // `get_detail` (file I/O) entirely.
        if let Some(search) = filter.search.as_ref().filter(|value| !value.is_empty()) {
            for entry in &mut entries {
                let metadata = format!(
                    "{} {} {} {} {}",
                    entry.method, entry.url, entry.host, entry.status, entry.mime_type
                );
                let detail = self.get_detail(&entry.id)?;
                let searchable = detail.map_or(metadata.clone(), |detail| {
                    format!(
                        "{metadata}\n{}\n{}",
                        String::from_utf8_lossy(&detail.request),
                        String::from_utf8_lossy(&detail.response)
                    )
                });
                entry.match_snippet = make_search_snippet(&searchable, search);
            }
        }
        Ok(entries)
    }

    /// Blocking (`std::fs::read`): call via `spawn_blocking` from async code.
    /// Missing body files (e.g. pruned or manually deleted) fall back to
    /// headers-only output instead of erroring, so history stays viewable.
    pub fn get_detail(&self, id: &str) -> Result<Option<HistoryDetail>> {
        let entry = self
            .connection
            .query_row(
                "SELECT q.rowid, q.id, q.url, q.method, q.host, q.path, p.status, p.size, p.mime_type,
                        p.duration_ms, q.timestamp, q.scoped
                 FROM requests q JOIN responses p ON p.request_id=q.id WHERE q.id=?1",
                [id],
                map_history_entry,
            )
            .optional()?;
        let Some(entry) = entry else { return Ok(None) };
        let (request_path, response_path, request_headers, response_headers): (String, String, Vec<u8>, Vec<u8>) = self.connection.query_row(
            "SELECT q.body_path, p.body_path, q.headers, p.headers FROM requests q JOIN responses p ON p.request_id=q.id WHERE q.id=?1",
            [id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )?;
        let mut request = request_headers;
        request.extend_from_slice(&read_body_file(&request_path)?);
        let mut response = response_headers;
        response.extend_from_slice(&read_body_file(&response_path)?);
        Ok(Some(HistoryDetail {
            entry,
            request,
            response,
        }))
    }

    pub fn delete_exchange(&mut self, id: &str) -> Result<bool> {
        let paths: Option<(String, String)> = self.connection.query_row(
            "SELECT q.body_path, p.body_path FROM requests q LEFT JOIN responses p ON p.request_id=q.id WHERE q.id=?1",
            [id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        ).optional()?;
        let deleted = self
            .connection
            .execute("DELETE FROM requests WHERE id=?1", [id])?
            > 0;
        if let Some((request, response)) = paths {
            let _ = fs::remove_file(request);
            let _ = fs::remove_file(response);
        }
        Ok(deleted)
    }

    pub fn clear_history(&mut self) -> Result<()> {
        let transaction = self.connection.transaction()?;
        transaction.execute("DELETE FROM responses", [])?;
        transaction.execute("DELETE FROM requests", [])?;
        transaction.commit()?;
        for directory in ["bodies/requests", "bodies/responses"] {
            let directory = self.project_path.join(directory);
            if directory.exists() {
                for entry in fs::read_dir(directory)? {
                    let path = entry?.path();
                    if path.is_file() {
                        fs::remove_file(path)?;
                    }
                }
            }
        }
        Ok(())
    }

    pub fn create_fuzz_scan(
        &self,
        id: &str,
        source_tab_id: i64,
        name: &str,
        started_at: &str,
    ) -> Result<FuzzScanRecord> {
        let name = name.trim();
        if name.is_empty() {
            return Err(WitnessError::Project("scan name is required".into()));
        }
        let now = chrono::Utc::now().to_rfc3339();
        self.connection.execute(
            "INSERT INTO fuzz_scans
             (id, source_tab_id, name, started_at, completed_at, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, NULL, ?5, ?5)
             ON CONFLICT(id) DO UPDATE SET
                source_tab_id=excluded.source_tab_id,
                name=excluded.name,
                started_at=excluded.started_at,
                updated_at=excluded.updated_at",
            params![id, source_tab_id, name, started_at, now],
        )?;
        self.connection
            .query_row(
                "SELECT id, source_tab_id, name, started_at, completed_at, created_at, updated_at
                 FROM fuzz_scans WHERE id=?1",
                [id],
                |row| {
                    Ok(FuzzScanRecord {
                        id: row.get(0)?,
                        source_tab_id: row.get(1)?,
                        name: row.get(2)?,
                        started_at: row.get(3)?,
                        completed_at: row.get(4)?,
                        created_at: row.get(5)?,
                        updated_at: row.get(6)?,
                    })
                },
            )
            .map_err(Into::into)
    }

    pub fn complete_fuzz_scan(&self, id: &str, completed_at: &str) -> Result<FuzzScanRecord> {
        let now = chrono::Utc::now().to_rfc3339();
        let updated = self.connection.execute(
            "UPDATE fuzz_scans SET completed_at=?2, updated_at=?3 WHERE id=?1",
            params![id, completed_at, now],
        )?;
        if updated == 0 {
            return Err(WitnessError::Project("fuzz scan was not found".into()));
        }
        self.connection
            .query_row(
                "SELECT id, source_tab_id, name, started_at, completed_at, created_at, updated_at
                 FROM fuzz_scans WHERE id=?1",
                [id],
                |row| {
                    Ok(FuzzScanRecord {
                        id: row.get(0)?,
                        source_tab_id: row.get(1)?,
                        name: row.get(2)?,
                        started_at: row.get(3)?,
                        completed_at: row.get(4)?,
                        created_at: row.get(5)?,
                        updated_at: row.get(6)?,
                    })
                },
            )
            .map_err(Into::into)
    }

    pub fn prune_history(&mut self, limit: usize) -> Result<usize> {
        // Batched prune: single cutoff lookup + single batched DELETE.
        // Previously this looped per-row DELETEs (N round-trips); now one
        // `DELETE WHERE timestamp < (SELECT ... OFFSET)` removes all stale
        // rows at once. Body files are collected first for post-delete cleanup.
        // Cutoff is the last-kept row (OFFSET limit-1), so `< cutoff` drops
        // exactly the older rows.
        let keep_offset = limit.saturating_sub(1) as i64;
        let cutoff: Option<String> = self
            .connection
            .query_row(
                "SELECT timestamp FROM requests ORDER BY timestamp DESC LIMIT 1 OFFSET ?1",
                [keep_offset],
                |row| row.get(0),
            )
            .optional()?;
        let Some(cutoff) = cutoff else {
            return Ok(0);
        };
        let stale: Vec<(String, Option<String>)> = {
            let mut statement = self.connection.prepare(
                "SELECT q.body_path, p.body_path
                 FROM requests q LEFT JOIN responses p ON p.request_id=q.id
                 WHERE q.timestamp < ?1",
            )?;
            let rows = statement
                .query_map([cutoff.as_str()], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            rows
        };
        if stale.is_empty() {
            return Ok(0);
        }
        let deleted = self.connection.execute(
            "DELETE FROM requests WHERE timestamp < (
                 SELECT timestamp FROM requests ORDER BY timestamp DESC LIMIT 1 OFFSET ?1
             )",
            [keep_offset],
        )?;
        for (request, response) in &stale {
            let _ = fs::remove_file(request);
            if let Some(response) = response {
                let _ = fs::remove_file(response);
            }
        }
        Ok(deleted)
    }

    pub fn load_scope(&self) -> Result<ScopeSnapshot> {
        let mut statement = self.connection.prepare_cached(
            "SELECT id, pattern, is_regex, include_subdomains, is_in_scope FROM scope ORDER BY id",
        )?;
        let entries = statement
            .query_map([], |row| {
                Ok(ScopeEntry {
                    id: row.get(0)?,
                    pattern: row.get(1)?,
                    is_regex: row.get(2)?,
                    include_subdomains: row.get(3)?,
                    is_in_scope: row.get(4)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(ScopeSnapshot { entries })
    }

    pub fn add_scope(
        &self,
        pattern: &str,
        is_regex: bool,
        include_subdomains: bool,
        is_in_scope: bool,
    ) -> Result<ScopeEntry> {
        self.connection.execute(
            "INSERT INTO scope (pattern, is_regex, include_subdomains, is_in_scope) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(pattern, is_regex, include_subdomains, is_in_scope) DO NOTHING",
            params![pattern, is_regex, include_subdomains, is_in_scope],
        )?;
        self.connection
            .query_row(
                "SELECT id, pattern, is_regex, include_subdomains, is_in_scope FROM scope
                 WHERE pattern=?1 AND is_regex=?2 AND include_subdomains=?3 AND is_in_scope=?4",
                params![pattern, is_regex, include_subdomains, is_in_scope],
                |row| {
                    Ok(ScopeEntry {
                        id: row.get(0)?,
                        pattern: row.get(1)?,
                        is_regex: row.get(2)?,
                        include_subdomains: row.get(3)?,
                        is_in_scope: row.get(4)?,
                    })
                },
            )
            .map_err(Into::into)
    }

    pub fn remove_scope(&self, id: i64) -> Result<bool> {
        Ok(self
            .connection
            .execute("DELETE FROM scope WHERE id=?1", [id])?
            > 0)
    }

    pub fn update_scope(
        &self,
        id: i64,
        pattern: &str,
        is_regex: bool,
        include_subdomains: bool,
        is_in_scope: bool,
    ) -> Result<ScopeEntry> {
        let changed = self.connection.execute(
            "UPDATE scope
             SET pattern=?2, is_regex=?3, include_subdomains=?4, is_in_scope=?5
             WHERE id=?1",
            params![id, pattern, is_regex, include_subdomains, is_in_scope],
        )?;
        if changed == 0 {
            return Err(WitnessError::Other(anyhow::anyhow!(
                "scope entry was not found"
            )));
        }
        self.connection
            .query_row(
                "SELECT id, pattern, is_regex, include_subdomains, is_in_scope
                 FROM scope WHERE id=?1",
                [id],
                |row| {
                    Ok(ScopeEntry {
                        id: row.get(0)?,
                        pattern: row.get(1)?,
                        is_regex: row.get(2)?,
                        include_subdomains: row.get(3)?,
                        is_in_scope: row.get(4)?,
                    })
                },
            )
            .map_err(Into::into)
    }

    pub fn organizer_snapshot(&self) -> Result<OrganizerBundle> {
        let folders = {
            let mut statement = self.connection.prepare(
                "SELECT id, name, parent_id, created_at, updated_at
                 FROM organizer_folders ORDER BY lower(name), created_at",
            )?;
            let folders = statement
                .query_map([], |row| {
                    Ok(OrganizerFolder {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        parent_id: row.get(2)?,
                        created_at: row.get(3)?,
                        updated_at: row.get(4)?,
                    })
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            folders
        };
        let items = {
            let mut statement = self.connection.prepare(
                "SELECT id, title, folder_id, stage_id, request, response, tls, source, method, host,
                        path, status, notes, tags, created_at, updated_at
                 FROM organizer_items ORDER BY updated_at DESC",
            )?;
            let items = statement
                .query_map([], map_organizer_item)?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            items
        };
        Ok(OrganizerBundle {
            version: 1,
            folders,
            items,
        })
    }

    pub fn create_organizer_folder(
        &self,
        name: &str,
        parent_id: Option<&str>,
    ) -> Result<OrganizerFolder> {
        let name = name.trim();
        if name.is_empty() {
            return Err(WitnessError::Organizer("folder name is required".into()));
        }
        if self.organizer_parent_depth(parent_id)? >= 4 {
            return Err(WitnessError::Organizer(
                "folders are limited to four levels".into(),
            ));
        }
        let now = chrono::Utc::now().to_rfc3339();
        let folder = OrganizerFolder {
            id: Uuid::new_v4().to_string(),
            name: name.into(),
            parent_id: parent_id.map(str::to_owned),
            created_at: now.clone(),
            updated_at: now,
        };
        self.connection.execute(
            "INSERT INTO organizer_folders (id, name, parent_id, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                folder.id,
                folder.name,
                folder.parent_id,
                folder.created_at,
                folder.updated_at
            ],
        )?;
        Ok(folder)
    }

    pub fn update_organizer_folder(
        &self,
        id: &str,
        name: &str,
        parent_id: Option<&str>,
    ) -> Result<OrganizerFolder> {
        let name = name.trim();
        if name.is_empty() {
            return Err(WitnessError::Organizer("folder name is required".into()));
        }
        if parent_id == Some(id) {
            return Err(WitnessError::Organizer(
                "a folder cannot contain itself".into(),
            ));
        }
        let mut ancestor = parent_id.map(str::to_owned);
        let mut parent_depth = 0;
        while let Some(current) = ancestor {
            if current == id {
                return Err(WitnessError::Organizer(
                    "a folder cannot be moved into its descendant".into(),
                ));
            }
            parent_depth += 1;
            ancestor = self
                .connection
                .query_row(
                    "SELECT parent_id FROM organizer_folders WHERE id=?1",
                    [&current],
                    |row| row.get(0),
                )
                .optional()?
                .flatten();
        }
        let subtree_depth = self.organizer_subtree_depth(id)?;
        if parent_depth + subtree_depth > 4 {
            return Err(WitnessError::Organizer(
                "this move would exceed four folder levels".into(),
            ));
        }
        let now = chrono::Utc::now().to_rfc3339();
        let changed = self.connection.execute(
            "UPDATE organizer_folders SET name=?2, parent_id=?3, updated_at=?4 WHERE id=?1",
            params![id, name, parent_id, now],
        )?;
        if changed == 0 {
            return Err(WitnessError::Organizer("folder was not found".into()));
        }
        Ok(self
            .organizer_snapshot()?
            .folders
            .into_iter()
            .find(|folder| folder.id == id)
            .expect("updated folder must exist"))
    }

    pub fn delete_organizer_folder(&self, id: &str) -> Result<bool> {
        Ok(self
            .connection
            .execute("DELETE FROM organizer_folders WHERE id=?1", [id])?
            > 0)
    }

    pub fn create_organizer_item(&self, input: &OrganizerItemInput) -> Result<OrganizerItem> {
        let now = chrono::Utc::now().to_rfc3339();
        let id = Uuid::new_v4().to_string();
        let (method, host, path) = organizer_request_metadata(&input.request);
        let status = organizer_response_status(&input.response);
        let title = input.title.trim().to_owned();
        let tags = normalized_tags(&input.tags);
        self.connection.execute(
            "INSERT INTO organizer_items
             (id, title, folder_id, stage_id, request, response, tls, source, method, host, path,
              status, notes, tags, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?15)",
            params![
                id,
                title,
                input.folder_id,
                input.stage_id,
                input.request,
                input.response,
                input.tls,
                input.source,
                method,
                host,
                path,
                status,
                input.notes,
                serde_json::to_string(&tags)?,
                now,
            ],
        )?;
        self.organizer_item(&id)?
            .ok_or_else(|| WitnessError::Organizer("saved item was not found".into()))
    }

    pub fn update_organizer_item(
        &self,
        id: &str,
        input: &OrganizerItemInput,
    ) -> Result<OrganizerItem> {
        let (method, host, path) = organizer_request_metadata(&input.request);
        let status = organizer_response_status(&input.response);
        let title = input.title.trim().to_owned();
        let changed = self.connection.execute(
            "UPDATE organizer_items
             SET title=?2, folder_id=?3, stage_id=?4, request=?5, response=?6, tls=?7, source=?8,
                 method=?9, host=?10, path=?11, status=?12, notes=?13, tags=?14, updated_at=?15
             WHERE id=?1",
            params![
                id,
                title,
                input.folder_id,
                input.stage_id,
                input.request,
                input.response,
                input.tls,
                input.source,
                method,
                host,
                path,
                status,
                input.notes,
                serde_json::to_string(&normalized_tags(&input.tags))?,
                chrono::Utc::now().to_rfc3339(),
            ],
        )?;
        if changed == 0 {
            return Err(WitnessError::Organizer("saved item was not found".into()));
        }
        self.organizer_item(id)?
            .ok_or_else(|| WitnessError::Organizer("saved item was not found".into()))
    }

    pub fn delete_organizer_item(&self, id: &str) -> Result<bool> {
        Ok(self
            .connection
            .execute("DELETE FROM organizer_items WHERE id=?1", [id])?
            > 0)
    }

    pub fn import_organizer(&mut self, bundle: &OrganizerBundle) -> Result<usize> {
        if bundle.version != 1 {
            return Err(WitnessError::Organizer(format!(
                "unsupported JSON version {}",
                bundle.version
            )));
        }
        validate_bundle_folders(&bundle.folders)?;
        let transaction = self.connection.transaction()?;
        let now = chrono::Utc::now().to_rfc3339();
        let folder_ids = bundle
            .folders
            .iter()
            .map(|folder| (folder.id.clone(), Uuid::new_v4().to_string()))
            .collect::<std::collections::HashMap<_, _>>();
        let mut pending = bundle.folders.iter().collect::<Vec<_>>();
        let mut inserted = std::collections::HashSet::new();
        while !pending.is_empty() {
            let before = pending.len();
            let mut next = Vec::new();
            for folder in pending {
                let parent_ready = folder.parent_id.as_ref().is_none_or(|parent| {
                    !bundle.folders.iter().any(|item| &item.id == parent)
                        || inserted.contains(parent)
                });
                if !parent_ready {
                    next.push(folder);
                    continue;
                }
                let parent = folder
                    .parent_id
                    .as_ref()
                    .and_then(|id| folder_ids.get(id))
                    .cloned();
                transaction.execute(
                    "INSERT INTO organizer_folders (id, name, parent_id, created_at, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?4)",
                    params![folder_ids[&folder.id], folder.name.trim(), parent, now],
                )?;
                inserted.insert(folder.id.clone());
            }
            if next.len() == before {
                return Err(WitnessError::Organizer(
                    "folder hierarchy contains a cycle".into(),
                ));
            }
            pending = next;
        }
        for item in &bundle.items {
            let id = Uuid::new_v4().to_string();
            let folder_id = item
                .folder_id
                .as_ref()
                .and_then(|folder| folder_ids.get(folder))
                .cloned();
            let (method, host, path) = organizer_request_metadata(&item.request);
            transaction.execute(
                "INSERT INTO organizer_items
                 (id, title, folder_id, stage_id, request, response, tls, source, method, host, path,
                  status, notes, tags, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?15)",
                params![
                    id,
                    item.title.trim(),
                    folder_id,
                    item.stage_id,
                    item.request,
                    item.response,
                    item.tls,
                    item.source,
                    method,
                    host,
                    path,
                    organizer_response_status(&item.response),
                    item.notes,
                    serde_json::to_string(&normalized_tags(&item.tags))?,
                    now,
                ],
            )?;
        }
        transaction.commit()?;
        Ok(bundle.items.len())
    }

    pub fn identity_snapshot(&self) -> Result<IdentityBundle> {
        let groups = {
            let mut statement = self.connection.prepare(
                "SELECT id, name, description, injection_type, injection_key
                 FROM identity_groups ORDER BY lower(name), id",
            )?;
            let rows = statement
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                    ))
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            rows.into_iter()
                .map(|(id, name, description, injection_type, injection_key)| {
                    Ok(IdentityGroup {
                        id,
                        name,
                        description,
                        injection_type: identity_injection_type_from_db(&injection_type)?,
                        injection_key,
                    })
                })
                .collect::<Result<Vec<_>>>()?
        };
        let identities = {
            let mut statement = self.connection.prepare(
                "SELECT id, group_id, name, color, notes, auth_value
                 FROM identities ORDER BY group_id, lower(name), id",
            )?;
            let identities = statement
                .query_map([], map_identity)?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            identities
        };
        Ok(IdentityBundle {
            version: 1,
            groups,
            identities,
        })
    }

    pub fn create_identity_group(&self, input: &IdentityGroupInput) -> Result<IdentityGroup> {
        validate_identity_group_input(input)?;
        let group = IdentityGroup {
            id: Uuid::new_v4().to_string(),
            name: unique_identity_group_name(&self.connection, &input.name, None)?,
            description: input.description.trim().to_owned(),
            injection_type: input.injection_type,
            injection_key: input.injection_key.trim().to_owned(),
        };
        self.connection.execute(
            "INSERT INTO identity_groups (id, name, description, injection_type, injection_key)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                group.id,
                group.name,
                group.description,
                identity_injection_type_to_db(group.injection_type),
                group.injection_key,
            ],
        )?;
        Ok(group)
    }

    pub fn update_identity_group(
        &self,
        id: &str,
        input: &IdentityGroupInput,
    ) -> Result<IdentityGroup> {
        validate_identity_group_input(input)?;
        let name = unique_identity_group_name(&self.connection, &input.name, Some(id))?;
        let changed = self.connection.execute(
            "UPDATE identity_groups
             SET name=?2, description=?3, injection_type=?4, injection_key=?5
             WHERE id=?1",
            params![
                id,
                name,
                input.description.trim(),
                identity_injection_type_to_db(input.injection_type),
                input.injection_key.trim(),
            ],
        )?;
        if changed == 0 {
            return Err(WitnessError::Identity(
                "identity group was not found".into(),
            ));
        }
        self.identity_group(id)?
            .ok_or_else(|| WitnessError::Identity("identity group was not found".into()))
    }

    pub fn delete_identity_group(&self, id: &str) -> Result<bool> {
        Ok(self
            .connection
            .execute("DELETE FROM identity_groups WHERE id=?1", [id])?
            > 0)
    }

    pub fn create_identity(&self, input: &IdentityInput) -> Result<Identity> {
        validate_identity_input(input)?;
        if self.identity_group(&input.group_id)?.is_none() {
            return Err(WitnessError::Identity(
                "identity group was not found".into(),
            ));
        }
        let identity = Identity {
            id: Uuid::new_v4().to_string(),
            group_id: input.group_id.clone(),
            name: input.name.trim().to_owned(),
            color: input.color.trim().to_owned(),
            notes: input.notes.clone(),
            auth_value: input.auth_value.clone(),
        };
        self.connection.execute(
            "INSERT INTO identities (id, group_id, name, color, notes, auth_value)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                identity.id,
                identity.group_id,
                identity.name,
                identity.color,
                identity.notes,
                identity.auth_value,
            ],
        )?;
        Ok(identity)
    }

    pub fn update_identity(&self, id: &str, input: &IdentityInput) -> Result<Identity> {
        validate_identity_input(input)?;
        if self.identity_group(&input.group_id)?.is_none() {
            return Err(WitnessError::Identity(
                "identity group was not found".into(),
            ));
        }
        let changed = self.connection.execute(
            "UPDATE identities SET group_id=?2, name=?3, color=?4, notes=?5, auth_value=?6
             WHERE id=?1",
            params![
                id,
                input.group_id,
                input.name.trim(),
                input.color.trim(),
                input.notes,
                input.auth_value,
            ],
        )?;
        if changed == 0 {
            return Err(WitnessError::Identity("identity was not found".into()));
        }
        self.identity(id)?
            .ok_or_else(|| WitnessError::Identity("identity was not found".into()))
    }

    pub fn delete_identity(&self, id: &str) -> Result<bool> {
        Ok(self
            .connection
            .execute("DELETE FROM identities WHERE id=?1", [id])?
            > 0)
    }

    pub fn identity_injection_descriptor(
        &self,
        identity_id: &str,
    ) -> Result<IdentityInjectionDescriptor> {
        let row = self
            .connection
            .query_row(
                "SELECT g.injection_type, g.injection_key, i.auth_value
                 FROM identities i JOIN identity_groups g ON g.id=i.group_id
                 WHERE i.id=?1",
                [identity_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                },
            )
            .optional()?;
        let Some((injection_type, injection_key, auth_value)) = row else {
            return Err(WitnessError::Identity("identity was not found".into()));
        };
        Ok(IdentityInjectionDescriptor {
            injection_type: identity_injection_type_from_db(&injection_type)?,
            injection_key,
            auth_value,
        })
    }

    pub fn import_identities(&mut self, bundle: &IdentityBundle) -> Result<usize> {
        if bundle.version != 1 {
            return Err(WitnessError::Identity(format!(
                "unsupported JSON version {}",
                bundle.version
            )));
        }
        let mut imported_group_ids = std::collections::HashSet::new();
        for group in &bundle.groups {
            if !imported_group_ids.insert(&group.id) {
                return Err(WitnessError::Identity(
                    "imported group IDs must be unique".into(),
                ));
            }
            validate_identity_group_input(&IdentityGroupInput {
                name: group.name.clone(),
                description: group.description.clone(),
                injection_type: group.injection_type,
                injection_key: group.injection_key.clone(),
            })?;
        }
        for identity in &bundle.identities {
            validate_identity_input(&IdentityInput {
                group_id: identity.group_id.clone(),
                name: identity.name.clone(),
                color: identity.color.clone(),
                notes: identity.notes.clone(),
                auth_value: identity.auth_value.clone(),
            })?;
            if !imported_group_ids.contains(&identity.group_id) {
                return Err(WitnessError::Identity(
                    "every imported identity must reference an imported group".into(),
                ));
            }
        }
        let transaction = self.connection.transaction()?;
        let group_ids = bundle
            .groups
            .iter()
            .map(|group| (group.id.clone(), Uuid::new_v4().to_string()))
            .collect::<std::collections::HashMap<_, _>>();
        for group in &bundle.groups {
            let name = unique_identity_group_name(&transaction, &group.name, None)?;
            transaction.execute(
                "INSERT INTO identity_groups (id, name, description, injection_type, injection_key)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    group_ids[&group.id],
                    name,
                    group.description.trim(),
                    identity_injection_type_to_db(group.injection_type),
                    group.injection_key.trim(),
                ],
            )?;
        }
        for identity in &bundle.identities {
            transaction.execute(
                "INSERT INTO identities (id, group_id, name, color, notes, auth_value)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    Uuid::new_v4().to_string(),
                    group_ids[&identity.group_id],
                    identity.name.trim(),
                    identity.color.trim(),
                    identity.notes,
                    identity.auth_value,
                ],
            )?;
        }
        transaction.commit()?;
        Ok(bundle.identities.len())
    }

    fn identity_group(&self, id: &str) -> Result<Option<IdentityGroup>> {
        let row = self
            .connection
            .query_row(
                "SELECT id, name, description, injection_type, injection_key
                 FROM identity_groups WHERE id=?1",
                [id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                    ))
                },
            )
            .optional()?;
        row.map(|(id, name, description, injection_type, injection_key)| {
            Ok(IdentityGroup {
                id,
                name,
                description,
                injection_type: identity_injection_type_from_db(&injection_type)?,
                injection_key,
            })
        })
        .transpose()
    }

    fn identity(&self, id: &str) -> Result<Option<Identity>> {
        self.connection
            .query_row(
                "SELECT id, group_id, name, color, notes, auth_value FROM identities WHERE id=?1",
                [id],
                map_identity,
            )
            .optional()
            .map_err(Into::into)
    }

    fn organizer_item(&self, id: &str) -> Result<Option<OrganizerItem>> {
        self.connection
            .query_row(
                "SELECT id, title, folder_id, stage_id, request, response, tls, source, method, host,
                        path, status, notes, tags, created_at, updated_at
                 FROM organizer_items WHERE id=?1",
                [id],
                map_organizer_item,
            )
            .optional()
            .map_err(Into::into)
    }

    fn organizer_parent_depth(&self, parent_id: Option<&str>) -> Result<usize> {
        let mut depth = 0;
        let mut current = parent_id.map(str::to_owned);
        while let Some(id) = current {
            depth += 1;
            current = self
                .connection
                .query_row(
                    "SELECT parent_id FROM organizer_folders WHERE id=?1",
                    [&id],
                    |row| row.get(0),
                )
                .optional()?
                .flatten();
        }
        Ok(depth)
    }

    fn organizer_subtree_depth(&self, id: &str) -> Result<usize> {
        let depth: i64 = self.connection.query_row(
            "WITH RECURSIVE descendants(id, depth) AS (
                SELECT id, 1 FROM organizer_folders WHERE id=?1
                UNION ALL
                SELECT f.id, descendants.depth + 1
                FROM organizer_folders f JOIN descendants ON f.parent_id=descendants.id
             ) SELECT COALESCE(MAX(depth), 1) FROM descendants",
            [id],
            |row| row.get(0),
        )?;
        Ok(depth as usize)
    }

    pub fn body_store(&self) -> &BodyStore {
        &self.bodies
    }

    pub fn checkpoint(&self) -> Result<()> {
        self.connection
            .execute_batch("PRAGMA wal_checkpoint(PASSIVE)")?;
        Ok(())
    }

    pub fn integrity_check(&self) -> Result<bool> {
        let result: String = self
            .connection
            .query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
        Ok(result == "ok")
    }
}

fn map_identity(row: &rusqlite::Row<'_>) -> rusqlite::Result<Identity> {
    Ok(Identity {
        id: row.get(0)?,
        group_id: row.get(1)?,
        name: row.get(2)?,
        color: row.get(3)?,
        notes: row.get(4)?,
        auth_value: row.get(5)?,
    })
}

fn identity_injection_type_to_db(injection_type: IdentityInjectionType) -> &'static str {
    match injection_type {
        IdentityInjectionType::Cookie => "cookie",
        IdentityInjectionType::Header => "header",
        IdentityInjectionType::QueryParameter => "queryParameter",
    }
}

fn identity_injection_type_from_db(value: &str) -> Result<IdentityInjectionType> {
    match value {
        "cookie" => Ok(IdentityInjectionType::Cookie),
        "header" => Ok(IdentityInjectionType::Header),
        "queryParameter" => Ok(IdentityInjectionType::QueryParameter),
        _ => Err(WitnessError::Identity(
            "invalid stored injection type".into(),
        )),
    }
}

fn validate_identity_group_input(input: &IdentityGroupInput) -> Result<()> {
    let name = input.name.trim();
    let key = input.injection_key.trim();
    if name.is_empty() {
        return Err(WitnessError::Identity("group name is required".into()));
    }
    if key.is_empty() {
        return Err(WitnessError::Identity("injection key is required".into()));
    }
    match input.injection_type {
        IdentityInjectionType::Cookie => {
            if key
                .bytes()
                .any(|byte| byte.is_ascii_whitespace() || matches!(byte, b';' | b'=' | b','))
            {
                return Err(WitnessError::Identity(
                    "cookie injection key is invalid".into(),
                ));
            }
        }
        IdentityInjectionType::Header => {
            ::http::header::HeaderName::from_bytes(key.as_bytes())
                .map_err(|_| WitnessError::Identity("header injection key is invalid".into()))?;
        }
        IdentityInjectionType::QueryParameter => {
            if key.contains('\r') || key.contains('\n') {
                return Err(WitnessError::Identity(
                    "query parameter injection key is invalid".into(),
                ));
            }
        }
    }
    Ok(())
}

fn validate_identity_input(input: &IdentityInput) -> Result<()> {
    if input.group_id.trim().is_empty() {
        return Err(WitnessError::Identity("identity group is required".into()));
    }
    if input.name.trim().is_empty() {
        return Err(WitnessError::Identity("identity name is required".into()));
    }
    Ok(())
}

fn unique_identity_group_name(
    connection: &Connection,
    requested_name: &str,
    exclude_id: Option<&str>,
) -> Result<String> {
    let base = requested_name.trim();
    let mut suffix = 0usize;
    loop {
        let candidate = if suffix == 0 {
            base.to_owned()
        } else {
            format!("{base}-{suffix}")
        };
        let exists: bool = connection.query_row(
            "SELECT EXISTS(
                SELECT 1 FROM identity_groups
                WHERE name=?1 COLLATE NOCASE AND (?2 IS NULL OR id != ?2)
             )",
            params![candidate, exclude_id],
            |row| row.get(0),
        )?;
        if !exists {
            return Ok(candidate);
        }
        suffix += 1;
    }
}

fn map_organizer_item(row: &rusqlite::Row<'_>) -> rusqlite::Result<OrganizerItem> {
    let tags: String = row.get(13)?;
    Ok(OrganizerItem {
        id: row.get(0)?,
        title: row.get(1)?,
        folder_id: row.get(2)?,
        stage_id: row.get(3)?,
        request: row.get(4)?,
        response: row.get(5)?,
        tls: row.get(6)?,
        source: row.get(7)?,
        method: row.get(8)?,
        host: row.get(9)?,
        path: row.get(10)?,
        status: row.get(11)?,
        notes: row.get(12)?,
        tags: serde_json::from_str(&tags).unwrap_or_default(),
        created_at: row.get(14)?,
        updated_at: row.get(15)?,
    })
}

fn organizer_request_metadata(raw: &[u8]) -> (String, String, String) {
    let head = String::from_utf8_lossy(&raw[..raw.len().min(16_384)]);
    let mut lines = head.lines();
    let start = lines.next().unwrap_or_default();
    let mut parts = start.split_whitespace();
    let method = parts.next().unwrap_or("REQUEST").to_owned();
    let target = parts.next().unwrap_or("/").to_owned();
    let host = lines
        .find_map(|line| {
            line.split_once(':')
                .filter(|(name, _)| name.eq_ignore_ascii_case("host"))
                .map(|(_, value)| value.trim().to_owned())
        })
        .unwrap_or_default();
    (method, host, target)
}

fn organizer_response_status(raw: &[u8]) -> Option<u16> {
    String::from_utf8_lossy(&raw[..raw.len().min(256)])
        .lines()
        .next()?
        .split_whitespace()
        .nth(1)?
        .parse()
        .ok()
}

fn normalized_tags(tags: &[String]) -> Vec<String> {
    let mut tags = tags
        .iter()
        .map(|tag| tag.trim().to_owned())
        .filter(|tag| !tag.is_empty())
        .collect::<Vec<_>>();
    tags.sort_by_key(|tag| tag.to_ascii_lowercase());
    tags.dedup_by(|left, right| left.eq_ignore_ascii_case(right));
    tags.truncate(32);
    tags
}

fn validate_bundle_folders(folders: &[OrganizerFolder]) -> Result<()> {
    let by_id = folders
        .iter()
        .map(|folder| (folder.id.as_str(), folder))
        .collect::<std::collections::HashMap<_, _>>();
    for folder in folders {
        if folder.name.trim().is_empty() {
            return Err(WitnessError::Organizer(
                "imported folders must have names".into(),
            ));
        }
        let mut depth = 1;
        let mut current = folder.parent_id.as_deref();
        let mut visited = std::collections::HashSet::new();
        while let Some(id) = current {
            if !visited.insert(id) {
                return Err(WitnessError::Organizer(
                    "folder hierarchy contains a cycle".into(),
                ));
            }
            depth += 1;
            if depth > 4 {
                return Err(WitnessError::Organizer(
                    "folders are limited to four levels".into(),
                ));
            }
            current = by_id.get(id).and_then(|parent| parent.parent_id.as_deref());
        }
    }
    Ok(())
}

fn map_history_entry(row: &rusqlite::Row<'_>) -> rusqlite::Result<HistoryEntry> {
    Ok(HistoryEntry {
        sequence: row.get::<_, i64>(0)? as u64,
        id: row.get(1)?,
        url: row.get(2)?,
        method: row.get(3)?,
        host: row.get(4)?,
        path: row.get(5)?,
        status: row.get(6)?,
        length: row.get::<_, i64>(7)? as usize,
        mime_type: row.get(8)?,
        duration_ms: row.get::<_, i64>(9)? as u64,
        timestamp: row.get(10)?,
        scoped: row.get(11)?,
        match_snippet: None,
    })
}

/// Escapes SQLite LIKE wildcards (`%`, `_`) and the escape char itself so
/// user-supplied filters match literally. Callers must use `ESCAPE '\'`.
pub(crate) fn escape_like(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        if matches!(ch, '\\' | '%' | '_') {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

/// Reads a body file, returning empty bytes when the file is missing (pruned
/// or manually deleted) instead of erroring. Other I/O errors propagate.
fn read_body_file(path: &str) -> Result<Vec<u8>> {
    match fs::read(path) {
        Ok(bytes) => Ok(bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(error.into()),
    }
}

fn make_search_snippet(value: &str, query: &str) -> Option<String> {
    let lowered = value.to_ascii_lowercase();
    let needle = query.to_ascii_lowercase();
    let start = lowered.find(&needle)?;
    let range = (start, start + needle.len());
    let mut start = range.0.saturating_sub(48);
    let mut end = (range.1 + 72).min(value.len());
    while start < value.len() && !value.is_char_boundary(start) {
        start += 1;
    }
    while end > start && !value.is_char_boundary(end) {
        end -= 1;
    }
    Some(
        value[start..end]
            .replace(['\r', '\n', '\t'], " ")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" "),
    )
}

pub fn split_http_message(raw: &[u8]) -> (&[u8], &[u8]) {
    let Some(index) = raw.windows(4).position(|window| window == b"\r\n\r\n") else {
        return (raw, &[]);
    };
    (&raw[..index + 4], &raw[index + 4..])
}

pub fn project_database_path(project: &Path) -> PathBuf {
    project.join(DATABASE_NAME)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metadata(root: &Path) -> (RequestMeta, ResponseMeta) {
        let store = BodyStore::new(root).unwrap();
        let request_id = Uuid::new_v4().to_string();
        let response_id = Uuid::new_v4().to_string();
        let request_path = store
            .write_body(BodyKind::Request, &request_id, b"request")
            .unwrap();
        let response_path = store
            .write_body(BodyKind::Response, &response_id, b"response")
            .unwrap();
        (
            RequestMeta {
                id: request_id.clone(),
                url: "http://example.test/a".into(),
                method: "GET".into(),
                host: "example.test".into(),
                path: "/a".into(),
                ip: None,
                timestamp: chrono::Utc::now().to_rfc3339(),
                headers: b"Host: example.test".to_vec(),
                body_path: request_path,
                scoped: true,
            },
            ResponseMeta {
                id: response_id,
                request_id,
                status: 200,
                mime_type: "text/plain".into(),
                duration_ms: 12,
                size: 8,
                headers: b"Content-Type: text/plain".to_vec(),
                body_path: response_path,
            },
        )
    }

    #[test]
    fn migration_and_metadata_crud_work() {
        let root = tempfile::tempdir().unwrap();
        let mut database = Database::open(root.path()).unwrap();
        assert_eq!(database.schema_version().unwrap(), CURRENT_SCHEMA_VERSION);
        assert!(database.integrity_check().unwrap());
        let (request, response) = metadata(root.path());
        database.insert_exchange(&request, &response).unwrap();
        let entries = database
            .query_history(&HistoryFilter::default(), 0, 10)
            .unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].sequence, 1);
        let detail = database.get_detail(&request.id).unwrap().unwrap();
        assert!(detail.request.ends_with(b"request"));
        assert!(detail.response.ends_with(b"response"));
        assert!(database.delete_exchange(&request.id).unwrap());
        assert!(database
            .query_history(&HistoryFilter::default(), 0, 10)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn body_storage_round_trip_and_delete_work() {
        let root = tempfile::tempdir().unwrap();
        let store = BodyStore::new(root.path()).unwrap();
        let id = Uuid::new_v4().to_string();
        store
            .write_body(BodyKind::Request, &id, b"binary\0body")
            .unwrap();
        assert_eq!(
            store.read_body(BodyKind::Request, &id).unwrap(),
            b"binary\0body"
        );
        store.delete_body(BodyKind::Request, &id).unwrap();
        assert!(!store.path(BodyKind::Request, &id).exists());
    }

    #[test]
    fn filters_are_executed_by_sqlite() {
        let root = tempfile::tempdir().unwrap();
        let mut database = Database::open(root.path()).unwrap();
        let (request, response) = metadata(root.path());
        database.insert_exchange(&request, &response).unwrap();
        let filter = HistoryFilter {
            method: Some("POST".into()),
            ..HistoryFilter::default()
        };
        assert!(database.query_history(&filter, 0, 10).unwrap().is_empty());
        let filter = HistoryFilter {
            host: Some("example".into()),
            status_min: Some(200),
            status_max: Some(299),
            ..HistoryFilter::default()
        };
        let results = database.query_history(&filter, 0, 10).unwrap();
        assert_eq!(results.len(), 1);
        let filter = HistoryFilter {
            search: Some("Content-Type".into()),
            ..HistoryFilter::default()
        };
        let results = database.query_history(&filter, 0, 10).unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0]
            .match_snippet
            .as_deref()
            .is_some_and(|snippet| snippet.contains("Content-Type")));
    }

    #[test]
    fn history_limit_prunes_metadata_and_body_files() {
        let root = tempfile::tempdir().unwrap();
        let mut database = Database::open(root.path()).unwrap();
        let (mut first_request, first_response) = metadata(root.path());
        first_request.timestamp = "2025-01-01T00:00:00Z".into();
        let first_body = first_request.body_path.clone();
        database
            .insert_exchange(&first_request, &first_response)
            .unwrap();
        let (mut second_request, second_response) = metadata(root.path());
        second_request.timestamp = "2025-01-02T00:00:00Z".into();
        database
            .insert_exchange(&second_request, &second_response)
            .unwrap();
        assert_eq!(database.prune_history(1).unwrap(), 1);
        assert_eq!(
            database
                .query_history(&HistoryFilter::default(), 0, 10)
                .unwrap()
                .len(),
            1
        );
        assert!(!first_body.exists());
        assert!(second_request.body_path.exists());
    }

    #[test]
    fn organizer_entries_folders_and_import_round_trip() {
        let root = tempfile::tempdir().unwrap();
        let mut database = Database::open(root.path()).unwrap();
        let first = database.create_organizer_folder("Workspace", None).unwrap();
        let second = database
            .create_organizer_folder("Auth", Some(&first.id))
            .unwrap();
        let third = database
            .create_organizer_folder("Tokens", Some(&second.id))
            .unwrap();
        let fourth = database
            .create_organizer_folder("Refresh", Some(&third.id))
            .unwrap();
        assert!(database
            .create_organizer_folder("Too deep", Some(&fourth.id))
            .is_err());

        let created = database
            .create_organizer_item(&OrganizerItemInput {
                title: "Refresh token".into(),
                folder_id: Some(fourth.id.clone()),
                stage_id: None,
                request: b"POST /refresh HTTP/1.1\r\nHost: example.test\r\n\r\n".to_vec(),
                response: b"HTTP/1.1 200 OK\r\n\r\n{}".to_vec(),
                tls: true,
                source: "test".into(),
                notes: "Original note".into(),
                tags: vec!["Auth".into(), "auth".into(), " regression ".into()],
            })
            .unwrap();
        assert_eq!(created.method, "POST");
        assert_eq!(created.host, "example.test");
        assert_eq!(created.status, Some(200));
        assert_eq!(created.tags, vec!["Auth", "regression"]);

        let updated = database
            .update_organizer_item(
                &created.id,
                &OrganizerItemInput {
                    title: "Edited refresh token".into(),
                    folder_id: Some(third.id),
                    stage_id: None,
                    request: b"PUT /refresh HTTP/1.1\r\nHost: example.test\r\n\r\n".to_vec(),
                    response: Vec::new(),
                    tls: false,
                    source: "test".into(),
                    notes: "Edited note".into(),
                    tags: vec!["edited".into()],
                },
            )
            .unwrap();
        assert_eq!(updated.method, "PUT");
        assert_eq!(updated.notes, "Edited note");
        assert_eq!(updated.status, None);

        let exported = database.organizer_snapshot().unwrap();
        assert_eq!(exported.folders.len(), 4);
        assert_eq!(exported.items.len(), 1);
        let json = serde_json::to_vec_pretty(&exported).unwrap();
        let imported: OrganizerBundle = serde_json::from_slice(&json).unwrap();
        assert_eq!(database.import_organizer(&imported).unwrap(), 1);
        let after_import = database.organizer_snapshot().unwrap();
        assert_eq!(after_import.folders.len(), 8);
        assert_eq!(after_import.items.len(), 2);
        assert_ne!(after_import.items[0].id, after_import.items[1].id);
    }

    #[test]
    fn scope_migration_preserves_legacy_rules_and_removes_global_toggle() {
        let root = tempfile::tempdir().unwrap();
        let legacy = rusqlite::Connection::open(root.path().join(DATABASE_NAME)).unwrap();
        legacy
            .execute_batch(
                "CREATE TABLE scope (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    pattern TEXT NOT NULL UNIQUE,
                    is_regex INTEGER NOT NULL DEFAULT 0,
                    enabled INTEGER NOT NULL DEFAULT 1
                 );
                 CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
                 INSERT INTO scope (pattern, is_regex, enabled) VALUES ('example.test', 0, 1);
                 INSERT INTO settings (key, value) VALUES ('scope_enabled', 'false');
                 PRAGMA user_version = 2;",
            )
            .unwrap();
        drop(legacy);

        let database = Database::open(root.path()).unwrap();
        let scope = database.load_scope().unwrap();
        assert_eq!(database.schema_version().unwrap(), CURRENT_SCHEMA_VERSION);
        assert_eq!(scope.entries.len(), 1);
        assert!(scope.entries[0].is_in_scope);
        assert!(scope.entries[0].include_subdomains);
        assert_eq!(
            database
                .connection
                .query_row(
                    "SELECT COUNT(*) FROM settings WHERE key='scope_enabled'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            0
        );
    }

    #[test]
    fn like_wildcards_are_escaped() {
        assert_eq!(escape_like("100%_\\"), "100\\%\\_\\\\");
        assert_eq!(escape_like("plain"), "plain");
        // A literal `%` filter must not match arbitrary hosts.
        let root = tempfile::tempdir().unwrap();
        let mut database = Database::open(root.path()).unwrap();
        let (request, response) = metadata(root.path());
        database.insert_exchange(&request, &response).unwrap();
        let filter = HistoryFilter {
            host: Some("%".into()),
            ..HistoryFilter::default()
        };
        assert!(database.query_history(&filter, 0, 10).unwrap().is_empty());
        let filter = HistoryFilter {
            host: Some("example.test".into()),
            ..HistoryFilter::default()
        };
        assert_eq!(database.query_history(&filter, 0, 10).unwrap().len(), 1);
    }

    #[test]
    fn missing_body_files_fall_back_to_headers_only() {
        let root = tempfile::tempdir().unwrap();
        let mut database = Database::open(root.path()).unwrap();
        let (request, response) = metadata(root.path());
        let id = request.id.clone();
        database.insert_exchange(&request, &response).unwrap();
        std::fs::remove_file(&request.body_path).unwrap();
        std::fs::remove_file(&response.body_path).unwrap();
        let detail = database.get_detail(&id).unwrap().unwrap();
        // Headers remain; missing bodies contribute nothing instead of Err.
        assert!(detail.request.len() >= request.headers.len());
        assert!(detail.response.len() >= response.headers.len());
    }
}
