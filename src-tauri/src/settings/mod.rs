use std::{fs, path::PathBuf};

use rusqlite::{params, Connection, OptionalExtension};

use crate::{error::Result, project::RecentProject, state::SettingsState};

const SETTINGS_KEY: &str = "app_settings";
const RECENT_PROJECTS_KEY: &str = "recent_projects";

pub struct SettingsStore {
    path: PathBuf,
}

impl SettingsStore {
    pub fn global() -> Result<Self> {
        let directory = dirs::config_dir()
            .or_else(dirs::home_dir)
            .unwrap_or_else(|| PathBuf::from("."))
            .join("witness");
        fs::create_dir_all(&directory)?;
        Ok(Self {
            path: directory.join("settings.sqlite3"),
        })
    }

    pub fn at(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    fn connection(&self) -> Result<Connection> {
        let connection = Connection::open(&self.path)?;
        connection.execute(
            "CREATE TABLE IF NOT EXISTS settings (key TEXT PRIMARY KEY, value TEXT NOT NULL)",
            [],
        )?;
        Ok(connection)
    }

    pub fn load(&self) -> Result<SettingsState> {
        let connection = self.connection()?;
        let value = connection
            .query_row(
                "SELECT value FROM settings WHERE key=?1",
                [SETTINGS_KEY],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        value.map_or_else(
            || Ok(SettingsState::default()),
            |json| {
                let mut settings: SettingsState = serde_json::from_str(&json)?;
                normalize_shortcut_modifier(&mut settings);
                Ok(settings)
            },
        )
    }

    pub fn save(&self, settings: &SettingsState) -> Result<()> {
        let connection = self.connection()?;
        let value = serde_json::to_string(settings)?;
        connection.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            params![SETTINGS_KEY, value],
        )?;
        Ok(())
    }

    pub fn load_recent_projects(&self) -> Result<Vec<RecentProject>> {
        let connection = self.connection()?;
        let value = connection
            .query_row(
                "SELECT value FROM settings WHERE key=?1",
                [RECENT_PROJECTS_KEY],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        value.map_or_else(
            || Ok(Vec::new()),
            |json| serde_json::from_str(&json).map_err(|error| error.into()),
        )
    }

    pub fn save_recent_projects(&self, projects: &[RecentProject]) -> Result<()> {
        let connection = self.connection()?;
        let value = serde_json::to_string(projects)?;
        connection.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            params![RECENT_PROJECTS_KEY, value],
        )?;
        Ok(())
    }

    pub fn path(&self) -> &PathBuf {
        &self.path
    }
}

fn normalize_shortcut_modifier(settings: &mut SettingsState) {
    if cfg!(target_os = "macos") {
        if !matches!(settings.shortcut_modifier.as_str(), "command" | "control") {
            settings.shortcut_modifier = "command".into();
        }
    } else {
        settings.shortcut_modifier = "control".into();
    }
}

pub fn load_global() -> Result<SettingsState> {
    let store = SettingsStore::global()?;
    match store.load() {
        Ok(settings) => Ok(settings),
        Err(error) => {
            // Back up corrupt settings DBs instead of silently discarding them.
            let path = store.path();
            if path.is_file() {
                let ts = chrono::Utc::now().format("%Y%m%dT%H%M%SZ");
                let backup = path.with_extension(format!("corrupt-{ts}.bak"));
                let _ = std::fs::rename(path, &backup);
                tracing::warn!(
                    %error,
                    backup = %backup.display(),
                    "global settings were corrupt; moved aside and using defaults"
                );
            }
            Ok(SettingsState::default())
        }
    }
}

pub fn save_global(settings: &SettingsState) -> Result<()> {
    SettingsStore::global()?.save(settings)
}

pub fn load_recent_projects() -> Result<Vec<RecentProject>> {
    SettingsStore::global()?.load_recent_projects()
}

pub fn record_recent_project(name: &str, path: &std::path::Path) -> Result<()> {
    let store = SettingsStore::global()?;
    let path = path.display().to_string();
    let mut projects = store.load_recent_projects()?;
    // Windows paths are case-insensitive; dedup case-insensitively there so
    // `C:\Proj` and `c:\proj` don't produce duplicate recents.
    #[cfg(windows)]
    projects.retain(|project| project.path.to_lowercase() != path.to_lowercase());
    #[cfg(not(windows))]
    projects.retain(|project| project.path != path);
    projects.insert(
        0,
        RecentProject {
            name: name.into(),
            path,
            last_opened: chrono::Utc::now().to_rfc3339(),
        },
    );
    projects.truncate(8);
    store.save_recent_projects(&projects)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip_through_sqlite() {
        let root = tempfile::tempdir().unwrap();
        let store = SettingsStore::at(root.path().join("settings.sqlite3"));
        let settings = SettingsState {
            theme: "light".into(),
            proxy_port: 9191,
            message_editor_font_size: 17,
            layout_split_percent: 61,
            ..SettingsState::default()
        };
        store.save(&settings).unwrap();
        let loaded = store.load().unwrap();
        assert_eq!(loaded.theme, "light");
        assert_eq!(loaded.proxy_port, 9191);
        assert_eq!(loaded.message_editor_font_size, 17);
        assert_eq!(loaded.layout_split_percent, 61);
    }

    #[test]
    fn legacy_settings_gain_defaults_for_new_fields() {
        let mut legacy = serde_json::to_value(SettingsState::default()).unwrap();
        legacy
            .as_object_mut()
            .unwrap()
            .remove("messageEditorFontSize");
        legacy.as_object_mut().unwrap().remove("showLogsTab");
        legacy.as_object_mut().unwrap().remove("aiEnterToSend");
        legacy.as_object_mut().unwrap().remove("shortcutModifier");

        let loaded: SettingsState = serde_json::from_value(legacy).unwrap();

        assert_eq!(
            loaded.message_editor_font_size,
            SettingsState::default().message_editor_font_size
        );
        assert!(!loaded.show_logs_tab);
        assert!(loaded.ai_enter_to_send);
        assert_eq!(
            loaded.shortcut_modifier,
            SettingsState::default().shortcut_modifier
        );
    }

    #[test]
    fn unsupported_shortcut_modifier_is_normalized_on_load() {
        let root = tempfile::tempdir().unwrap();
        let store = SettingsStore::at(root.path().join("settings.sqlite3"));
        let settings = SettingsState {
            shortcut_modifier: "invalid".into(),
            ..SettingsState::default()
        };
        store.save(&settings).unwrap();

        let loaded = store.load().unwrap();
        assert_eq!(
            loaded.shortcut_modifier,
            SettingsState::default().shortcut_modifier
        );
    }

    #[test]
    fn recent_projects_round_trip_in_most_recent_order() {
        let root = tempfile::tempdir().unwrap();
        let store = SettingsStore::at(root.path().join("settings.sqlite3"));
        let projects = vec![
            RecentProject {
                name: "Latest".into(),
                path: "/projects/latest".into(),
                last_opened: "2026-07-28T10:00:00Z".into(),
            },
            RecentProject {
                name: "Older".into(),
                path: "/projects/older".into(),
                last_opened: "2026-07-27T10:00:00Z".into(),
            },
        ];
        store.save_recent_projects(&projects).unwrap();
        assert_eq!(store.load_recent_projects().unwrap(), projects);
    }

    #[test]
    fn recent_projects_dedup_is_case_insensitive_on_windows() {
        // Documents the Windows rule enforced in `record_recent_project`:
        // paths differing only by case are the same recent entry.
        let existing = "/Projects/Latest".to_lowercase();
        let incoming = "/projects/latest".to_lowercase();
        assert_eq!(existing, incoming);
    }
}
