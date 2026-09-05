use std::{
    fs::{self, File},
    io::{Read, Write},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;
use zip::{write::SimpleFileOptions, CompressionMethod, ZipWriter};

use crate::error::{Result, WitnessError};

pub const WITNESS_FORMAT: &str = "witness-project";
pub const WITNESS_VERSION: u32 = 2;
const MAX_WITNESS_ENTRIES: usize = 100_000;
const MAX_WITNESS_UNCOMPRESSED_BYTES: u64 = 4 * 1024 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WitnessManifest {
    pub format: String,
    pub version: u32,
    pub project_name: String,
}

impl WitnessManifest {
    fn new(project_name: impl Into<String>) -> Self {
        Self {
            format: WITNESS_FORMAT.into(),
            version: WITNESS_VERSION,
            project_name: project_name.into(),
        }
    }
}

pub fn export_witness_archive(source: &Path, destination: &Path, project_name: &str) -> Result<()> {
    export_witness_archive_cancellable(source, destination, project_name, &CancellationToken::new())
}

pub fn export_witness_archive_cancellable(
    source: &Path,
    destination: &Path,
    project_name: &str,
    cancellation: &CancellationToken,
) -> Result<()> {
    let temporary = destination.with_extension(format!(
        "{}.part",
        destination
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("wns")
    ));
    tracing::info!(
        target: "witness_lib::project",
        operation = "archive_export",
        phase = "started",
        source = %source.display(),
        destination = %destination.display(),
        temporary = %temporary.display(),
        project_name,
        "project archive export started"
    );

    if let Err(error) = write_witness_archive(source, &temporary, project_name, cancellation) {
        tracing::error!(
            target: "witness_lib::project",
            operation = "archive_export",
            phase = "write_archive",
            source = %source.display(),
            destination = %destination.display(),
            temporary = %temporary.display(),
            error = %error,
            "project archive export failed while writing the temporary archive"
        );
        if let Err(cleanup_error) = fs::remove_file(&temporary) {
            if cleanup_error.kind() != std::io::ErrorKind::NotFound {
                tracing::warn!(
                    target: "witness_lib::project",
                    operation = "archive_export",
                    phase = "cleanup_temporary_archive",
                    path = %temporary.display(),
                    error = %cleanup_error,
                    "failed to remove the temporary project archive after export failure"
                );
            }
        }
        return Err(error);
    }

    match replace_file(temporary.clone(), destination) {
        Ok(()) => {
            tracing::info!(
                target: "witness_lib::project",
                operation = "archive_export",
                phase = "completed",
                source = %source.display(),
                destination = %destination.display(),
                "project archive export completed"
            );
            Ok(())
        }
        Err(error) => {
            tracing::error!(
                target: "witness_lib::project",
                operation = "archive_export",
                phase = "install_archive",
                source = %source.display(),
                destination = %destination.display(),
                temporary = %temporary.display(),
                error = %error,
                "project archive export failed while installing the archive"
            );
            if let Err(cleanup_error) = fs::remove_file(&temporary) {
                if cleanup_error.kind() != std::io::ErrorKind::NotFound {
                    tracing::warn!(
                        target: "witness_lib::project",
                        operation = "archive_export",
                        phase = "cleanup_temporary_archive",
                        path = %temporary.display(),
                        error = %cleanup_error,
                        "failed to remove the temporary project archive after install failure"
                    );
                }
            }
            Err(error)
        }
    }
}

fn write_witness_archive(
    source: &Path,
    destination: &Path,
    project_name: &str,
    cancellation: &CancellationToken,
) -> Result<()> {
    ensure_not_cancelled(cancellation)?;
    let file = File::create(destination)?;
    let mut archive = ZipWriter::new(file);
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .unix_permissions(0o644);
    let manifest = serde_json::to_vec_pretty(&WitnessManifest::new(project_name))?;
    archive
        .start_file("manifest.json", options)
        .map_err(|error| WitnessError::Other(error.into()))?;
    archive.write_all(&manifest)?;

    let mut files = Vec::new();
    collect_files(source, &mut files, cancellation)?;
    for path in files {
        ensure_not_cancelled(cancellation)?;
        if path == destination {
            continue;
        }
        let relative = path
            .strip_prefix(source)
            .map_err(|error| WitnessError::Other(error.into()))?;
        let name = relative.to_string_lossy().replace('\\', "/");
        if name == "manifest.json"
            || name.ends_with(".part")
            || name.starts_with(".workspace.json.backup-")
        {
            continue;
        }
        archive
            .start_file(name, options)
            .map_err(|error| WitnessError::Other(error.into()))?;
        let mut input = File::open(path)?;
        let mut buffer = [0_u8; 64 * 1024];
        loop {
            ensure_not_cancelled(cancellation)?;
            let read = input.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            archive.write_all(&buffer[..read])?;
        }
    }
    archive
        .finish()
        .map_err(|error| WitnessError::Other(error.into()))?;
    Ok(())
}

pub(crate) fn replace_file(temporary: PathBuf, destination: &Path) -> Result<()> {
    let first_error = match fs::rename(&temporary, destination) {
        Ok(()) => return Ok(()),
        Err(error) => error,
    };

    if !destination.exists() {
        tracing::error!(
            target: "witness_lib::project",
            operation = "archive_replace",
            phase = "install_without_existing_archive",
            temporary = %temporary.display(),
            destination = %destination.display(),
            error = %first_error,
            "failed to install project archive and no previous archive exists"
        );
        return Err(first_error.into());
    }

    tracing::warn!(
        target: "witness_lib::project",
        operation = "archive_replace",
        phase = "rename_over_existing_archive",
        temporary = %temporary.display(),
        destination = %destination.display(),
        error = %first_error,
        "direct project archive replacement failed; attempting safe backup replacement"
    );

    // Some platforms do not allow rename-over-existing. Move the old archive
    // aside first, then restore it if installing the new archive fails. This
    // keeps the last known-good project intact on every error path.
    let file_name = destination
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("project.wns");
    let backup = destination.with_file_name(format!(".{file_name}.backup-{}", Uuid::new_v4()));
    if let Err(error) = fs::rename(destination, &backup) {
        tracing::error!(
            target: "witness_lib::project",
            operation = "archive_replace",
            phase = "move_existing_archive_to_backup",
            destination = %destination.display(),
            backup = %backup.display(),
            error = %error,
            "failed to move the existing project archive to a backup"
        );
        return Err(WitnessError::Other(anyhow::anyhow!(
            "unable to replace {}: {error}",
            destination.display()
        )));
    }
    tracing::info!(
        target: "witness_lib::project",
        operation = "archive_replace",
        phase = "backup_created",
        destination = %destination.display(),
        backup = %backup.display(),
        "existing project archive moved to a temporary backup"
    );

    match fs::rename(&temporary, destination) {
        Ok(()) => {
            if let Err(error) = fs::remove_file(&backup) {
                if error.kind() != std::io::ErrorKind::NotFound {
                    tracing::warn!(
                        target: "witness_lib::project",
                        operation = "archive_replace",
                        phase = "cleanup_archive_backup",
                        path = %backup.display(),
                        error = %error,
                        "project archive installed but the previous archive backup could not be removed"
                    );
                }
            }
            Ok(())
        }
        Err(error) => {
            tracing::error!(
                target: "witness_lib::project",
                operation = "archive_replace",
                phase = "install_new_archive_after_backup",
                temporary = %temporary.display(),
                destination = %destination.display(),
                backup = %backup.display(),
                error = %error,
                "failed to install the new project archive after moving the previous archive"
            );
            let restore_result = fs::rename(&backup, destination);
            if let Err(restore_error) = restore_result {
                tracing::error!(
                    target: "witness_lib::project",
                    operation = "archive_replace",
                    phase = "restore_previous_archive",
                    destination = %destination.display(),
                    backup = %backup.display(),
                    error = %restore_error,
                    "failed to restore the previous project archive"
                );
                return Err(WitnessError::Other(anyhow::anyhow!(
                    "unable to install {}: {error}; unable to restore previous archive: {restore_error}",
                    destination.display()
                )));
            }
            tracing::info!(
                target: "witness_lib::project",
                operation = "archive_replace",
                phase = "previous_archive_restored",
                destination = %destination.display(),
                "previous project archive restored after replacement failure"
            );
            Err(error.into())
        }
    }
}

pub fn import_witness_archive(source: &Path, destination: &Path) -> Result<WitnessManifest> {
    if destination.exists() {
        return Err(WitnessError::Project(
            "the archive extraction directory already exists".into(),
        ));
    }
    fs::create_dir_all(destination)?;
    let result = (|| -> Result<WitnessManifest> {
        let file = File::open(source)?;
        let mut archive =
            zip::ZipArchive::new(file).map_err(|error| WitnessError::Other(error.into()))?;
        if archive.len() > MAX_WITNESS_ENTRIES {
            return Err(WitnessError::Project(
                "the .wns archive contains too many entries".into(),
            ));
        }
        let mut manifest = None;
        let mut total_size = 0_u64;
        for index in 0..archive.len() {
            let mut entry = archive
                .by_index(index)
                .map_err(|error| WitnessError::Other(error.into()))?;
            total_size = total_size.saturating_add(entry.size());
            if total_size > MAX_WITNESS_UNCOMPRESSED_BYTES {
                return Err(WitnessError::Project(
                    "the .wns archive is too large to extract".into(),
                ));
            }
            let relative = entry.enclosed_name().ok_or_else(|| {
                WitnessError::Project(format!("archive contains an unsafe path: {}", entry.name()))
            })?;
            if relative.as_os_str().is_empty() {
                continue;
            }
            if relative == Path::new("manifest.json") {
                let mut bytes = Vec::new();
                entry.read_to_end(&mut bytes)?;
                manifest = Some(serde_json::from_slice::<WitnessManifest>(&bytes)?);
                continue;
            }
            let target = destination.join(relative);
            if entry.is_dir() {
                fs::create_dir_all(&target)?;
                continue;
            }
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut output = File::create(&target)?;
            std::io::copy(&mut entry, &mut output)?;
        }
        let manifest = manifest.ok_or_else(|| {
            WitnessError::Project("the .wns archive is missing its format manifest".into())
        })?;
        if manifest.format != WITNESS_FORMAT || manifest.version != WITNESS_VERSION {
            return Err(WitnessError::Project(format!(
                "unsupported .wns archive format {} version {}",
                manifest.format, manifest.version
            )));
        }
        Ok(manifest)
    })();
    if result.is_err() {
        if let Err(error) = fs::remove_dir_all(destination) {
            if error.kind() != std::io::ErrorKind::NotFound {
                tracing::warn!(
                    target: "witness_lib::project",
                    operation = "archive_import",
                    phase = "cleanup_failed_extraction",
                    destination = %destination.display(),
                    error = %error,
                    "failed to remove the partial .wns extraction after import failed"
                );
            }
        }
    }
    result
}

/// Collects regular files under `directory` for archiving.
///
/// Precondition: callers must checkpoint the SQLite database before exporting
/// so the main database file is self-contained. WAL (`-wal`), shared-memory
/// (`-shm`) and rollback-journal (`-journal`) sidecars are intentionally
/// skipped because they only contain uncheckpointed pages.
///
/// Symlinks are never followed (uses `symlink_metadata`) to avoid cycles and
/// to keep archives portable; symlinked entries are skipped.
fn collect_files(
    directory: &Path,
    output: &mut Vec<PathBuf>,
    cancellation: &CancellationToken,
) -> Result<()> {
    for entry in fs::read_dir(directory)? {
        ensure_not_cancelled(cancellation)?;
        let path = entry?.path();
        // Do not follow symlinks: a symlinked dir could loop back to an
        // ancestor and a symlinked file would embed an out-of-project target.
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() {
            continue;
        }
        if metadata.is_dir() {
            collect_files(&path, output, cancellation)?;
        } else if metadata.is_file() {
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                // Skip SQLite sidecars; the checkpointed main db is enough.
                if name.ends_with("-wal") || name.ends_with("-shm") || name.ends_with("-journal") {
                    continue;
                }
            }
            output.push(path);
        }
    }
    Ok(())
}

fn ensure_not_cancelled(cancellation: &CancellationToken) -> Result<()> {
    if cancellation.is_cancelled() {
        Err(WitnessError::Cancelled)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn witness_archive_round_trips_manifest_and_workspace() {
        let source = tempfile::tempdir().unwrap();
        fs::create_dir_all(source.path().join("bodies/requests")).unwrap();
        fs::create_dir_all(source.path().join("bodies/responses")).unwrap();
        fs::write(source.path().join(".witness-project"), b"Round trip").unwrap();
        fs::write(source.path().join("witness.sqlite3"), b"database").unwrap();
        fs::write(source.path().join("workspace.json"), b"{\"version\":1}").unwrap();
        fs::write(source.path().join("bodies/requests/one.bin"), b"body").unwrap();
        let archive = tempfile::NamedTempFile::new().unwrap();

        export_witness_archive(source.path(), archive.path(), "Round trip").unwrap();
        let destination_root = tempfile::tempdir().unwrap();
        let destination = destination_root.path().join("unpacked");
        let manifest = import_witness_archive(archive.path(), &destination).unwrap();

        assert_eq!(manifest.format, WITNESS_FORMAT);
        assert_eq!(manifest.version, WITNESS_VERSION);
        assert_eq!(manifest.project_name, "Round trip");
        assert_eq!(
            fs::read_to_string(destination.join("workspace.json")).unwrap(),
            "{\"version\":1}"
        );
        assert_eq!(
            fs::read(destination.join("bodies/requests/one.bin")).unwrap(),
            b"body"
        );
    }

    #[test]
    fn invalid_witness_archive_is_removed_after_failed_import() {
        let archive = tempfile::NamedTempFile::new().unwrap();
        fs::write(archive.path(), b"not an archive").unwrap();
        let destination_root = tempfile::tempdir().unwrap();
        let destination = destination_root.path().join("unpacked");

        assert!(import_witness_archive(archive.path(), &destination).is_err());
        assert!(!destination.exists());
    }

    #[test]
    fn archive_without_manifest_is_not_accepted_as_witness() {
        let archive = tempfile::NamedTempFile::new().unwrap();
        let file = File::create(archive.path()).unwrap();
        let mut writer = ZipWriter::new(file);
        writer
            .start_file(".witness-project", SimpleFileOptions::default())
            .unwrap();
        writer.write_all(b"Project").unwrap();
        writer.finish().unwrap();

        let destination_root = tempfile::tempdir().unwrap();
        let destination = destination_root.path().join("unpacked");
        assert!(import_witness_archive(archive.path(), &destination).is_err());
        assert!(!destination.exists());
    }

    #[test]
    fn collect_files_skips_sqlite_sidecars_and_symlinks() {
        let source = tempfile::tempdir().unwrap();
        fs::write(source.path().join("witness.sqlite3"), b"db").unwrap();
        fs::write(source.path().join("witness.sqlite3-wal"), b"wal").unwrap();
        fs::write(source.path().join("witness.sqlite3-shm"), b"shm").unwrap();
        fs::write(source.path().join("witness.sqlite3-journal"), b"journal").unwrap();
        fs::write(source.path().join("keep.txt"), b"keep").unwrap();
        let mut files = Vec::new();
        collect_files(source.path(), &mut files, &CancellationToken::new()).unwrap();
        let names: Vec<String> = files
            .iter()
            .filter_map(|p| p.file_name().and_then(|n| n.to_str()).map(str::to_owned))
            .collect();
        assert!(names.contains(&"witness.sqlite3".to_string()));
        assert!(names.contains(&"keep.txt".to_string()));
        assert!(!names.iter().any(|n| n.ends_with("-wal")));
        assert!(!names.iter().any(|n| n.ends_with("-shm")));
        assert!(!names.iter().any(|n| n.ends_with("-journal")));

        // Symlinked entries must be skipped, never followed.
        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            let target = source.path().join("keep.txt");
            let link = source.path().join("link.txt");
            symlink(&target, &link).unwrap();
            let mut files = Vec::new();
            collect_files(source.path(), &mut files, &CancellationToken::new()).unwrap();
            assert!(!files.iter().any(|p| p == &link));
        }
    }
}
