use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::{
    error::{Result, WitnessError},
    event_bus::{Event, ProjectEvent},
    state::AppState,
    worker::{check_cancelled, Task},
};

const PROJECT_MARKER: &str = ".witness-project";
pub const PROJECT_EXTENSION: &str = "wns";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectInfo {
    pub name: String,
    pub path: PathBuf,
    pub display_path: PathBuf,
    pub archive_path: Option<PathBuf>,
    pub owned_path: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RecentProject {
    pub name: String,
    pub path: String,
    pub last_opened: String,
}

pub async fn start_autosave(state: AppState) {
    if let Some(previous) = state.project_autosave_cancellation.lock().await.take() {
        previous.cancel();
        tracing::info!(
            target: "witness_lib::project",
            operation = "autosave",
            phase = "previous_loop_cancelled",
            "previous project autosave loop cancelled"
        );
    }
    let cancellation = CancellationToken::new();
    *state.project_autosave_cancellation.lock().await = Some(cancellation.clone());
    let configured_interval = state.project.read().await.autosave_interval_seconds.max(1);
    tracing::info!(
        target: "witness_lib::project",
        operation = "autosave",
        phase = "started",
        interval_seconds = configured_interval,
        "project autosave loop started"
    );
    tauri::async_runtime::spawn(async move {
        loop {
            let seconds = state.project.read().await.autosave_interval_seconds.max(1);
            tokio::select! {
                _ = cancellation.cancelled() => {
                    tracing::info!(
                        target: "witness_lib::project",
                        operation = "autosave",
                        phase = "stopped",
                        "project autosave loop stopped"
                    );
                    break;
                },
                _ = tokio::time::sleep(std::time::Duration::from_secs(seconds)) => {}
            }
            if cancellation.is_cancelled() {
                tracing::info!(
                    target: "witness_lib::project",
                    operation = "autosave",
                    phase = "cancelled_before_save_lock",
                    "project autosave loop stopped before starting a cancelled tick"
                );
                break;
            }
            let project = state.project.read().await.clone();
            let Some(path) = project.current_project_path.clone() else {
                tracing::warn!(
                    target: "witness_lib::project",
                    operation = "autosave",
                    phase = "skipped_no_working_copy",
                    "project autosave tick skipped because no working copy is open"
                );
                continue;
            };
            // Dirty check: skip both checkpoint and export when clean.
            if !project.dirty {
                tracing::debug!(
                    target: "witness_lib::project",
                    operation = "autosave",
                    phase = "skipped_clean",
                    working_path = %path.display(),
                    "project autosave tick skipped because the project is clean"
                );
                continue;
            }
            tracing::info!(
                target: "witness_lib::project",
                operation = "autosave",
                phase = "tick",
                interval_seconds = seconds,
                working_path = %path.display(),
                archive_path = ?project.archive_path,
                dirty = project.dirty,
                "project autosave tick started"
            );
            let response = match state
                .task_queue
                .enqueue(
                    Task::SaveProject { path: path.clone() },
                    cancellation.child_token(),
                )
                .await
            {
                Ok(response) => response,
                Err(error) => {
                    tracing::error!(
                        target: "witness_lib::project",
                        operation = "autosave",
                        phase = "enqueue",
                        working_path = %path.display(),
                        error = %error,
                        "project autosave failed to enqueue its working-copy task"
                    );
                    continue;
                }
            };
            let task_result = match response.await {
                Ok(result) => result,
                Err(error) => {
                    tracing::error!(
                        target: "witness_lib::project",
                        operation = "autosave",
                        phase = "worker_response",
                        working_path = %path.display(),
                        error = %error,
                        "project autosave worker did not return a result"
                    );
                    continue;
                }
            };
            if let Err(error) = task_result {
                tracing::error!(
                    target: "witness_lib::project",
                    operation = "autosave",
                    phase = "working_copy_task",
                    working_path = %path.display(),
                    error = %error,
                    "project autosave working-copy task failed"
                );
                continue;
            }

            if cancellation.is_cancelled() {
                tracing::info!(
                    target: "witness_lib::project",
                    operation = "autosave",
                    phase = "cancelled_before_save",
                    working_path = %path.display(),
                    "project autosave tick cancelled before acquiring the save lock"
                );
                break;
            }

            let current_project = state.project.read().await.clone();
            if current_project.current_project_path.as_ref() != Some(&path) {
                tracing::info!(
                    target: "witness_lib::project",
                    operation = "autosave",
                    phase = "stale_working_copy",
                    working_path = %path.display(),
                    "project autosave tick skipped because the working copy changed"
                );
                continue;
            }
            // Checkpoint under a short save-lock; released before the blocking
            // export below so saves don't stall the worker.
            {
                let _save_guard = state.project_save_lock.lock().await;
                let database_guard = state.database.lock().await;
                if cancellation.is_cancelled() {
                    tracing::info!(
                        target: "witness_lib::project",
                        operation = "autosave",
                        phase = "cancelled_after_save_lock",
                        working_path = %path.display(),
                        "project autosave tick cancelled after acquiring the save lock"
                    );
                    break;
                }
                if let Some(database) = database_guard.as_ref() {
                    if let Err(error) = database.checkpoint() {
                        tracing::error!(
                            target: "witness_lib::project",
                            operation = "autosave",
                            phase = "checkpoint",
                            working_path = %path.display(),
                            error = %error,
                            "project autosave failed while checkpointing the database"
                        );
                        continue;
                    }
                    tracing::info!(
                        target: "witness_lib::project",
                        operation = "autosave",
                        phase = "checkpoint_completed",
                        working_path = %path.display(),
                        "project autosave database checkpoint completed"
                    );
                } else {
                    tracing::warn!(
                        target: "witness_lib::project",
                        operation = "autosave",
                        phase = "checkpoint_skipped",
                        working_path = %path.display(),
                        "project autosave is proceeding without an open database"
                    );
                }
            } // release save_lock + database guard before blocking export

            let archive_path = current_project.archive_path.clone();
            let archive_saved = if let Some(destination) = archive_path.clone() {
                let source = path.clone();
                let destination_for_task = destination.clone();
                let name = current_project
                    .name
                    .clone()
                    .unwrap_or_else(|| "Witness Project".into());
                let cancellation = cancellation.child_token();
                match tokio::task::spawn_blocking(move || {
                    crate::export::export_witness_archive_cancellable(
                        &source,
                        &destination_for_task,
                        &name,
                        &cancellation,
                    )
                })
                .await
                {
                    Ok(Ok(())) => true,
                    Ok(Err(error)) => {
                        tracing::error!(
                            target: "witness_lib::project",
                            operation = "autosave",
                            phase = "archive_export",
                            working_path = %path.display(),
                            destination = %destination.display(),
                            error = %error,
                            "project autosave failed while exporting the .wns archive"
                        );
                        false
                    }
                    Err(error) => {
                        tracing::error!(
                            target: "witness_lib::project",
                            operation = "autosave",
                            phase = "archive_task",
                            working_path = %path.display(),
                            destination = %destination.display(),
                            error = %error,
                            "project autosave archive task failed to complete"
                        );
                        false
                    }
                }
            } else {
                tracing::info!(
                    target: "witness_lib::project",
                    operation = "autosave",
                    phase = "archive_skipped",
                    working_path = %path.display(),
                    "project autosave has no .wns destination yet"
                );
                true
            };
            if cancellation.is_cancelled() {
                tracing::info!(
                    target: "witness_lib::project",
                    operation = "autosave",
                    phase = "cancelled_after_archive",
                    working_path = %path.display(),
                    "project autosave tick cancelled before publishing its result"
                );
                break;
            }
            if archive_saved {
                // Re-acquire save lock only for the flag update.
                let _save_guard = state.project_save_lock.lock().await;
                let mut current_project = state.project.write().await;
                current_project.archive_path = archive_path.clone();
                current_project.dirty = false;
                drop(current_project);
                state.event_bus.publish(Event::Project(ProjectEvent::Saved {
                    path: archive_path
                        .clone()
                        .unwrap_or(path.clone())
                        .display()
                        .to_string(),
                }));
                tracing::info!(
                    target: "witness_lib::project",
                    operation = "autosave",
                    phase = "completed",
                    working_path = %path.display(),
                    archive_path = ?archive_path,
                    "project autosave completed"
                );
            } else {
                tracing::error!(
                    target: "witness_lib::project",
                    operation = "autosave",
                    phase = "failed",
                    working_path = %path.display(),
                    archive_path = ?current_project.archive_path,
                    "project autosave did not produce a saved archive"
                );
            }
        }
    });
}

#[derive(Default)]
pub struct ProjectManager;

impl ProjectManager {
    pub async fn create_project(name: &str, path: &Path) -> Result<ProjectInfo> {
        if name.trim().is_empty() {
            return Err(WitnessError::Project("project name is required".into()));
        }
        if !is_witness_path(path) {
            return Err(WitnessError::Project(
                "Witness projects must use the .wns extension".into(),
            ));
        }
        Self::create_archive_project(name, path).await
    }

    async fn initialize_materialized_project(name: &str, path: &Path) -> Result<()> {
        if path.exists() && std::fs::read_dir(path)?.next().transpose()?.is_some() {
            return Err(WitnessError::Project(
                "the working directory must be empty".into(),
            ));
        }
        ensure_bodies_dirs(path).await?;
        tokio::fs::write(path.join(PROJECT_MARKER), name.as_bytes()).await?;
        Ok(())
    }

    async fn create_archive_project(name: &str, destination: &Path) -> Result<ProjectInfo> {
        if destination.exists() {
            return Err(WitnessError::Project(
                "the .wns project file already exists".into(),
            ));
        }
        let working_path = Self::new_materialized_path(".witness-new").await?;
        if let Err(error) = Self::initialize_materialized_project(name, &working_path).await {
            cleanup_materialized_path(&working_path).await;
            return Err(error);
        }
        let database = match crate::database::Database::open(&working_path) {
            Ok(database) => database,
            Err(error) => {
                cleanup_materialized_path(&working_path).await;
                return Err(error);
            }
        };
        if let Err(error) = database
            .register_project(name)
            .and_then(|_| database.checkpoint())
        {
            cleanup_materialized_path(&working_path).await;
            return Err(error);
        }
        if let Some(parent) = destination
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            if let Err(error) = tokio::fs::create_dir_all(parent).await {
                cleanup_materialized_path(&working_path).await;
                return Err(error.into());
            }
        }
        let source = working_path.clone();
        let target = destination.to_path_buf();
        let project_name = name.to_string();
        let archive_result = tokio::task::spawn_blocking(move || {
            crate::export::export_witness_archive(&source, &target, &project_name)
        })
        .await
        .map_err(|error| WitnessError::Project(error.to_string()));
        let archive_result = match archive_result {
            Ok(result) => result,
            Err(error) => {
                cleanup_materialized_path(&working_path).await;
                return Err(error);
            }
        };
        if let Err(error) = archive_result {
            cleanup_materialized_path(&working_path).await;
            return Err(error);
        }
        Ok(ProjectInfo {
            name: name.into(),
            path: working_path,
            display_path: destination.to_path_buf(),
            archive_path: Some(destination.to_path_buf()),
            owned_path: true,
        })
    }

    pub async fn load(path: &Path, cancellation: &CancellationToken) -> Result<ProjectInfo> {
        check_cancelled(cancellation)?;
        if !is_witness_path(path) {
            return Err(WitnessError::Project(
                "Witness projects must use the .wns extension".into(),
            ));
        }
        let working_path = Self::new_materialized_path(".witness-open").await?;
        let archive_path = path.to_path_buf();
        let destination = working_path.clone();
        let import_result = tokio::task::spawn_blocking(move || {
            crate::export::import_witness_archive(&archive_path, &destination)
        })
        .await
        .map_err(|error| WitnessError::Project(error.to_string()));
        let manifest = match import_result {
            Ok(Ok(manifest)) => manifest,
            Ok(Err(error)) => {
                cleanup_materialized_path(&working_path).await;
                return Err(error);
            }
            Err(error) => {
                cleanup_materialized_path(&working_path).await;
                return Err(error);
            }
        };
        if cancellation.is_cancelled() {
            cleanup_materialized_path(&working_path).await;
            return Err(WitnessError::Cancelled);
        }
        if !working_path.join(PROJECT_MARKER).is_file() {
            cleanup_materialized_path(&working_path).await;
            return Err(WitnessError::Project(
                "the .wns archive is missing its project marker".into(),
            ));
        }
        let manifest_name = manifest.project_name;
        // Prefer the materialized marker; fall back to the manifest name, then
        // a generic name. (Previous match had an unreachable Err arm; an
        // if/else chain keeps all cases reachable.)
        let marker_content = tokio::fs::read_to_string(working_path.join(PROJECT_MARKER))
            .await
            .ok();
        let name = match marker_content.as_deref().map(str::trim) {
            Some(value) if !value.is_empty() => value.to_owned(),
            _ if !manifest_name.trim().is_empty() => manifest_name.clone(),
            _ => "Imported project".into(),
        };
        if let Err(error) = ensure_bodies_dirs(&working_path).await {
            cleanup_materialized_path(&working_path).await;
            return Err(error);
        }
        Ok(ProjectInfo {
            name,
            path: working_path,
            display_path: path.to_path_buf(),
            archive_path: Some(path.to_path_buf()),
            owned_path: true,
        })
    }

    pub async fn delete_project(path: &Path) -> Result<()> {
        if !is_witness_path(path) {
            return Err(WitnessError::Project(
                "Witness projects must use the .wns extension".into(),
            ));
        }
        if !path.is_file() {
            return Err(WitnessError::Project(
                "the project archive file does not exist".into(),
            ));
        }
        tokio::fs::remove_file(path).await?;
        Ok(())
    }

    pub async fn delete_materialized_project(path: &Path) -> Result<()> {
        if !path.join(PROJECT_MARKER).is_file() {
            tracing::error!(
                target: "witness_lib::project",
                operation = "working_copy_cleanup",
                phase = "validate_marker",
                working_path = %path.display(),
                marker = %path.join(PROJECT_MARKER).display(),
                error = "the working project marker is missing",
                "refusing to remove a working copy without its project marker"
            );
            return Err(WitnessError::Project(
                "refusing to delete a working directory without an Witness project marker".into(),
            ));
        }
        tracing::info!(
            target: "witness_lib::project",
            operation = "working_copy_cleanup",
            phase = "remove_directory",
            working_path = %path.display(),
            "removing materialized project working copy"
        );
        if let Err(error) = tokio::fs::remove_dir_all(path).await {
            tracing::error!(
                target: "witness_lib::project",
                operation = "working_copy_cleanup",
                phase = "remove_directory",
                working_path = %path.display(),
                error = %error,
                "failed to remove materialized project working copy"
            );
            return Err(error.into());
        }
        tracing::info!(
            target: "witness_lib::project",
            operation = "working_copy_cleanup",
            phase = "completed",
            working_path = %path.display(),
            "materialized project working copy removed"
        );
        Ok(())
    }

    pub async fn rename_materialized_project(path: &Path, name: &str) -> Result<()> {
        if name.trim().is_empty() {
            return Err(WitnessError::Project("project name is required".into()));
        }
        if !path.join(PROJECT_MARKER).is_file() {
            return Err(WitnessError::Project(
                "the working project marker is missing".into(),
            ));
        }
        tokio::fs::write(path.join(PROJECT_MARKER), name.as_bytes()).await?;
        Ok(())
    }

    pub fn is_project(path: &Path) -> bool {
        is_witness_path(path) && path.is_file()
    }

    pub async fn create_temporary_project() -> Result<ProjectInfo> {
        let root = dirs::cache_dir()
            .or_else(dirs::data_local_dir)
            .unwrap_or_else(std::env::temp_dir)
            .join("witness")
            .join("sessions");
        tokio::fs::create_dir_all(&root).await?;
        let path = root.join(format!(".session-{}", Uuid::new_v4()));
        if let Err(error) = Self::initialize_materialized_project("Temporary session", &path).await
        {
            cleanup_materialized_path(&path).await;
            return Err(error);
        }
        Ok(ProjectInfo {
            name: "Temporary session".into(),
            path: path.clone(),
            display_path: path,
            archive_path: None,
            owned_path: true,
        })
    }

    async fn new_materialized_path(prefix: &str) -> Result<PathBuf> {
        let root = dirs::cache_dir()
            .or_else(dirs::data_local_dir)
            .unwrap_or_else(std::env::temp_dir)
            .join("witness")
            .join("projects");
        tokio::fs::create_dir_all(&root).await?;
        Ok(root.join(format!("{prefix}-{}", Uuid::new_v4())))
    }
}

fn is_witness_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case(PROJECT_EXTENSION))
}

/// Shared helper deduplicating `create_archive_project` and `load`: ensures
/// the file-backed body directories exist.
async fn ensure_bodies_dirs(path: &Path) -> Result<()> {
    tokio::fs::create_dir_all(path.join("bodies/requests")).await?;
    tokio::fs::create_dir_all(path.join("bodies/responses")).await?;
    Ok(())
}

async fn cleanup_materialized_path(path: &Path) {
    if let Err(error) = tokio::fs::remove_dir_all(path).await {
        if error.kind() != std::io::ErrorKind::NotFound {
            tracing::warn!(
                target: "witness_lib::project",
                operation = "working_copy_cleanup",
                phase = "materialization_failure",
                working_path = %path.display(),
                error = %error,
                "failed to remove a partial materialized project after an earlier operation failed"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        database::{BodyKind, Database, RequestMeta, ResponseMeta},
        event_bus::EventCategory,
    };

    #[tokio::test]
    async fn project_create_open_close_and_delete_work() {
        let parent = tempfile::tempdir().unwrap();
        let path = parent.path().join("project.wns");
        let created = ProjectManager::create_project("Test", &path).await.unwrap();
        assert_eq!(created.name, "Test");
        let opened = ProjectManager::load(&path, &CancellationToken::new())
            .await
            .unwrap();
        assert_ne!(opened.path, path);
        ProjectManager::delete_materialized_project(&created.path)
            .await
            .unwrap();
        ProjectManager::delete_materialized_project(&opened.path)
            .await
            .unwrap();
        ProjectManager::delete_project(&path).await.unwrap();
        assert!(!path.exists());
    }

    #[tokio::test]
    async fn non_witness_project_paths_are_rejected() {
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join("project");
        tokio::fs::create_dir_all(&folder).await.unwrap();
        tokio::fs::write(folder.join(PROJECT_MARKER), "Test")
            .await
            .unwrap();
        assert!(!ProjectManager::is_project(&folder));
        assert!(ProjectManager::create_project("Test", &folder)
            .await
            .is_err());
        assert!(ProjectManager::load(&folder, &CancellationToken::new())
            .await
            .is_err());

        let legacy_path = root.path().join("project.witness");
        assert!(!ProjectManager::is_project(&legacy_path));
        assert!(ProjectManager::create_project("Test", &legacy_path)
            .await
            .is_err());
        assert!(
            ProjectManager::load(&legacy_path, &CancellationToken::new())
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn temporary_project_can_be_saved_as_a_portable_project() {
        let source = ProjectManager::create_temporary_project().await.unwrap();
        assert!(source
            .path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with(".session-"));
        tokio::fs::write(source.path.join("captured.txt"), b"traffic")
            .await
            .unwrap();
        let destination_root = tempfile::tempdir().unwrap();
        let destination = destination_root.path().join("permanent.wns");
        ProjectManager::rename_materialized_project(&source.path, "Permanent")
            .await
            .unwrap();
        crate::export::export_witness_archive(&source.path, &destination, "Permanent").unwrap();

        let opened = ProjectManager::load(&destination, &CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(opened.name, "Permanent");
        assert_eq!(
            tokio::fs::read(opened.path.join("captured.txt"))
                .await
                .unwrap(),
            b"traffic"
        );
        ProjectManager::delete_materialized_project(&source.path)
            .await
            .unwrap();
        ProjectManager::delete_materialized_project(&opened.path)
            .await
            .unwrap();
        ProjectManager::delete_project(&destination).await.unwrap();
    }

    #[tokio::test]
    async fn single_file_projects_create_and_materialize_a_working_copy() {
        let root = tempfile::tempdir().unwrap();
        let archive = root.path().join("review.wns");
        let created = ProjectManager::create_project("Review", &archive)
            .await
            .unwrap();
        assert!(archive.is_file());
        assert!(created.archive_path.as_deref() == Some(archive.as_path()));
        assert!(created.owned_path);
        assert_ne!(created.path, archive);

        let opened = ProjectManager::load(&archive, &CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(opened.display_path, archive);
        assert!(opened.path.is_dir());
        assert!(opened.owned_path);
        let database = Database::open(&opened.path).unwrap();
        assert!(database.integrity_check().unwrap());

        ProjectManager::delete_materialized_project(&created.path)
            .await
            .unwrap();
        ProjectManager::delete_materialized_project(&opened.path)
            .await
            .unwrap();
        ProjectManager::delete_project(&archive).await.unwrap();
    }

    #[tokio::test]
    async fn single_file_projects_rebase_file_backed_bodies_on_restore() {
        let root = tempfile::tempdir().unwrap();
        let source_info = ProjectManager::create_temporary_project().await.unwrap();
        let source = source_info.path;
        let mut database = Database::open(&source).unwrap();
        let request_id = "request-1".to_string();
        let response_id = "response-1".to_string();
        let request_path = database
            .body_store()
            .write_body(BodyKind::Request, &request_id, b"request-body")
            .unwrap();
        let response_path = database
            .body_store()
            .write_body(BodyKind::Response, &response_id, b"response-body")
            .unwrap();
        database
            .insert_exchange(
                &RequestMeta {
                    id: request_id.clone(),
                    url: "https://example.test/".into(),
                    method: "GET".into(),
                    host: "example.test".into(),
                    path: "/".into(),
                    ip: None,
                    timestamp: chrono::Utc::now().to_rfc3339(),
                    headers: b"GET / HTTP/1.1\r\n\r\n".to_vec(),
                    body_path: request_path,
                    scoped: true,
                },
                &ResponseMeta {
                    id: response_id,
                    request_id: request_id.clone(),
                    status: 200,
                    mime_type: "text/plain".into(),
                    duration_ms: 1,
                    size: 13,
                    headers: b"HTTP/1.1 200 OK\r\n\r\n".to_vec(),
                    body_path: response_path,
                },
            )
            .unwrap();
        database.checkpoint().unwrap();
        drop(database);

        let archive = root.path().join("bodies.wns");
        crate::export::export_witness_archive(&source, &archive, "Bodies").unwrap();
        let opened = ProjectManager::load(&archive, &CancellationToken::new())
            .await
            .unwrap();
        let imported = Database::open(&opened.path).unwrap();
        imported.rebase_body_paths_to_project().unwrap();
        let detail = imported.get_detail("request-1").unwrap().unwrap();
        assert!(detail.request.ends_with(b"request-body"));
        assert!(detail.response.ends_with(b"response-body"));

        ProjectManager::delete_materialized_project(&source)
            .await
            .unwrap();
        ProjectManager::delete_materialized_project(&opened.path)
            .await
            .unwrap();
        ProjectManager::delete_project(&archive).await.unwrap();
    }

    #[tokio::test]
    async fn autosave_checkpoints_project_on_interval() {
        let root = tempfile::tempdir().unwrap();
        let state = AppState::new();
        state.project.write().await.current_project_path = Some(root.path().to_path_buf());
        state.project.write().await.autosave_interval_seconds = 1;
        state.project.write().await.dirty = true;
        *state.database.lock().await = Some(Database::open(root.path()).unwrap());
        let mut events = state.event_bus.subscribe(Some(EventCategory::Project));
        start_autosave(state.clone()).await;
        tokio::time::timeout(std::time::Duration::from_secs(3), events.recv())
            .await
            .unwrap()
            .unwrap();
        assert!(!state.project.read().await.dirty);
        state
            .project_autosave_cancellation
            .lock()
            .await
            .take()
            .unwrap()
            .cancel();
    }

    #[test]
    fn witness_path_matching_is_case_insensitive() {
        assert!(is_witness_path(std::path::Path::new("project.wns")));
        assert!(is_witness_path(std::path::Path::new("project.WNS")));
        assert!(!is_witness_path(std::path::Path::new("project.zip")));
    }

    #[tokio::test]
    async fn bodies_dirs_helper_creates_request_and_response_dirs() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("work");
        ensure_bodies_dirs(&path).await.unwrap();
        assert!(path.join("bodies/requests").is_dir());
        assert!(path.join("bodies/responses").is_dir());
    }
}
