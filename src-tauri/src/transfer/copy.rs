//! The bytes: planning what a transfer touches, then moving one file at a time.
//!
//! Everything here is a plain async function over an `SftpSession` and the
//! local file system. The engine decides *when* and *whether*; this decides
//! *how*, and reports back through a [`Gate`] so that a pause or a cancel is
//! honoured between chunks rather than between files.

use std::future::Future;
use std::io::SeekFrom;
use std::path::{Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};

use russh_sftp::client::SftpSession;
use russh_sftp::protocol::{FileAttributes, FileType, OpenFlags};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};

use crate::error::{AppError, AppResult};
use crate::files::{posix_join, posix_parent};
use crate::transfer::Direction;

/// Bytes per read. Large enough that the round trip is not the cost, small
/// enough that a pause lands within a fraction of a second.
pub const CHUNK: usize = 256 * 1024;

/// How the engine hears from a copy in progress, and how it stops one.
pub trait Gate: Send + Sync {
    /// Called between chunks. Returns once the transfer may continue, or an
    /// error to abandon it - which is how a cancel arrives mid-file.
    fn wait(&self) -> impl Future<Output = AppResult<()>> + Send;
    fn progress(&self, bytes: u64);
}

/// One file the transfer will copy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileItem {
    pub source: String,
    pub destination: String,
    pub size: u64,
    /// Seconds since the epoch; copied onto the destination afterwards.
    pub modified: Option<i64>,
}

/// Everything a transfer will do, worked out before the first byte moves so
/// the totals are known and every directory exists before its files.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Plan {
    /// Destination directories to create, parents before children.
    pub directories: Vec<String>,
    pub files: Vec<FileItem>,
    pub total_bytes: u64,
}

fn fail(path: &str, err: impl std::fmt::Display) -> AppError {
    AppError::Files {
        path: path.to_string(),
        reason: err.to_string(),
    }
}

fn local_join(dir: &str, name: &str) -> String {
    Path::new(dir).join(name).display().to_string()
}

// ---------------------------------------------------------------------------
// Planning
// ---------------------------------------------------------------------------

pub async fn plan(
    sftp: &SftpSession,
    direction: Direction,
    source: &str,
    destination: &str,
) -> AppResult<Plan> {
    match direction {
        Direction::Download => plan_download(sftp, source, destination).await,
        Direction::Upload => {
            let source = source.to_string();
            let destination = destination.to_string();
            tauri::async_runtime::spawn_blocking(move || plan_upload(&source, &destination))
                .await
                .map_err(|err| AppError::Transfer(format!("planning failed: {err}")))?
        }
    }
}

/// Walks a remote tree. Symlinks are followed, so a linked directory is
/// copied as a directory; a dangling link is left out rather than failing the
/// whole transfer.
async fn plan_download(sftp: &SftpSession, source: &str, destination: &str) -> AppResult<Plan> {
    let mut plan = Plan::default();
    let root = sftp
        .metadata(source)
        .await
        .map_err(|err| fail(source, err))?;

    if !root.is_dir() {
        plan.total_bytes = root.size.unwrap_or(0);
        plan.files.push(FileItem {
            source: source.to_string(),
            destination: destination.to_string(),
            size: root.size.unwrap_or(0),
            modified: root.mtime.map(i64::from),
        });
        return Ok(plan);
    }

    plan.directories.push(destination.to_string());
    let mut pending = vec![(source.to_string(), destination.to_string())];
    while let Some((remote_dir, local_dir)) = pending.pop() {
        let entries = sftp
            .read_dir(&remote_dir)
            .await
            .map_err(|err| fail(&remote_dir, err))?;
        for entry in entries {
            let name = entry.file_name();
            let remote = posix_join(&remote_dir, &name);
            let local = local_join(&local_dir, &name);
            let meta = match entry.file_type() {
                FileType::Symlink => match sftp.metadata(&remote).await {
                    Ok(meta) => meta,
                    Err(_) => continue,
                },
                _ => entry.metadata(),
            };
            match meta.file_type() {
                FileType::Dir => {
                    plan.directories.push(local.clone());
                    pending.push((remote, local));
                }
                FileType::File => {
                    let size = meta.size.unwrap_or(0);
                    plan.total_bytes += size;
                    plan.files.push(FileItem {
                        source: remote,
                        destination: local,
                        size,
                        modified: meta.mtime.map(i64::from),
                    });
                }
                _ => {}
            }
        }
    }
    // Directories were pushed as they were discovered, which is parents
    // first; files sort for a stable, readable order of progress.
    plan.files.sort_by(|a, b| a.source.cmp(&b.source));
    Ok(plan)
}

fn plan_upload(source: &str, destination: &str) -> AppResult<Plan> {
    let mut plan = Plan::default();
    let root = std::fs::metadata(source).map_err(|err| fail(source, err))?;

    if !root.is_dir() {
        plan.total_bytes = root.len();
        plan.files.push(FileItem {
            source: source.to_string(),
            destination: destination.to_string(),
            size: root.len(),
            modified: mtime_seconds(&root),
        });
        return Ok(plan);
    }

    plan.directories.push(destination.to_string());
    let mut pending = vec![(PathBuf::from(source), destination.to_string())];
    while let Some((local_dir, remote_dir)) = pending.pop() {
        let entries = std::fs::read_dir(&local_dir)
            .map_err(|err| fail(&local_dir.display().to_string(), err))?;
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let local = entry.path();
            let remote = posix_join(&remote_dir, &name);
            // `metadata` follows symlinks; a dangling one is skipped.
            let Ok(meta) = std::fs::metadata(&local) else {
                continue;
            };
            if meta.is_dir() {
                plan.directories.push(remote.clone());
                pending.push((local, remote));
            } else if meta.is_file() {
                plan.total_bytes += meta.len();
                plan.files.push(FileItem {
                    source: local.display().to_string(),
                    destination: remote,
                    size: meta.len(),
                    modified: mtime_seconds(&meta),
                });
            }
        }
    }
    plan.files.sort_by(|a, b| a.source.cmp(&b.source));
    Ok(plan)
}

fn mtime_seconds(meta: &std::fs::Metadata) -> Option<i64> {
    meta.modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|since| since.as_secs() as i64)
}

// ---------------------------------------------------------------------------
// Destinations
// ---------------------------------------------------------------------------

/// What is at a destination already: `(size, modified)`, or `None`.
///
/// A directory where a file is about to go is an error rather than a
/// conflict: no answer to "overwrite?" makes that right.
pub async fn existing(
    sftp: &SftpSession,
    direction: Direction,
    path: &str,
) -> AppResult<Option<(u64, Option<i64>)>> {
    match direction {
        Direction::Download => match std::fs::metadata(path) {
            Ok(meta) if meta.is_dir() => Err(fail(path, "a directory is in the way")),
            Ok(meta) => Ok(Some((meta.len(), mtime_seconds(&meta)))),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(err) => Err(fail(path, err)),
        },
        Direction::Upload => match sftp.metadata(path).await {
            Ok(meta) if meta.is_dir() => Err(fail(path, "a directory is in the way")),
            Ok(meta) => Ok(Some((meta.size.unwrap_or(0), meta.mtime.map(i64::from)))),
            // The protocol has one error for "no such file"; anything else
            // surfaces when the file is opened, with a better message.
            Err(_) => Ok(None),
        },
    }
}

/// `name (1).ext`, `name (2).ext`... - the first that is not taken.
pub async fn available_name(
    sftp: &SftpSession,
    direction: Direction,
    path: &str,
) -> AppResult<String> {
    let (parent, file) = match direction {
        Direction::Download => {
            let p = Path::new(path);
            (
                p.parent()
                    .map(|d| d.display().to_string())
                    .unwrap_or_default(),
                p.file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            )
        }
        Direction::Upload => (
            posix_parent(path).unwrap_or_else(|| "/".to_string()),
            path.rsplit('/').next().unwrap_or(path).to_string(),
        ),
    };
    let (stem, ext) = match file.rfind('.') {
        Some(dot) if dot > 0 => (&file[..dot], &file[dot..]),
        _ => (file.as_str(), ""),
    };

    for n in 1..1000u32 {
        let candidate_name = format!("{stem} ({n}){ext}");
        let candidate = match direction {
            Direction::Download => local_join(&parent, &candidate_name),
            Direction::Upload => posix_join(&parent, &candidate_name),
        };
        if existing(sftp, direction, &candidate).await?.is_none() {
            return Ok(candidate);
        }
    }
    Err(fail(path, "no free name after 999 tries"))
}

/// Creates a destination directory, treating one that exists as success.
/// Returns whether anything was created, so a transfer that found every
/// directory already there and skipped every file can say it wrote nothing.
pub async fn ensure_directory(
    sftp: &SftpSession,
    direction: Direction,
    path: &str,
) -> AppResult<bool> {
    match direction {
        Direction::Download => {
            if Path::new(path).is_dir() {
                return Ok(false);
            }
            std::fs::create_dir_all(path).map_err(|err| fail(path, err))?;
            Ok(true)
        }
        Direction::Upload => {
            if let Ok(meta) = sftp.metadata(path).await {
                if meta.is_dir() {
                    return Ok(false);
                }
                return Err(fail(path, "a file is in the way"));
            }
            sftp.create_dir(path).await.map_err(|err| fail(path, err))?;
            Ok(true)
        }
    }
}

// ---------------------------------------------------------------------------
// Copying
// ---------------------------------------------------------------------------

/// Copies one file, starting `offset` bytes in. With an offset of zero the
/// destination is created or truncated; otherwise it is appended to, which is
/// what resuming a partial copy means.
pub async fn copy_file<G: Gate>(
    sftp: &SftpSession,
    direction: Direction,
    item: &FileItem,
    offset: u64,
    gate: &G,
) -> AppResult<()> {
    match direction {
        Direction::Download => download(sftp, item, offset, gate).await,
        Direction::Upload => upload(sftp, item, offset, gate).await,
    }
}

async fn download<G: Gate>(
    sftp: &SftpSession,
    item: &FileItem,
    offset: u64,
    gate: &G,
) -> AppResult<()> {
    let mut remote = sftp
        .open_with_flags(&item.source, OpenFlags::READ)
        .await
        .map_err(|err| fail(&item.source, err))?;
    if offset > 0 {
        remote
            .seek(SeekFrom::Start(offset))
            .await
            .map_err(|err| fail(&item.source, err))?;
    }

    let mut local = if offset > 0 {
        tokio::fs::OpenOptions::new()
            .append(true)
            .open(&item.destination)
            .await
    } else {
        tokio::fs::File::create(&item.destination).await
    }
    .map_err(|err| fail(&item.destination, err))?;

    let mut buffer = vec![0u8; CHUNK];
    loop {
        gate.wait().await?;
        let read = remote
            .read(&mut buffer)
            .await
            .map_err(|err| fail(&item.source, err))?;
        if read == 0 {
            break;
        }
        local
            .write_all(&buffer[..read])
            .await
            .map_err(|err| fail(&item.destination, err))?;
        gate.progress(read as u64);
    }

    local
        .flush()
        .await
        .map_err(|err| fail(&item.destination, err))?;
    drop(local);
    remote
        .close()
        .await
        .map_err(|err| fail(&item.source, err))?;

    if let Some(modified) = item.modified {
        // Best effort: a file system that will not take a time stamp is not a
        // failed transfer.
        let _ = set_local_modified(&item.destination, modified);
    }
    Ok(())
}

async fn upload<G: Gate>(
    sftp: &SftpSession,
    item: &FileItem,
    offset: u64,
    gate: &G,
) -> AppResult<()> {
    let mut local = tokio::fs::File::open(&item.source)
        .await
        .map_err(|err| fail(&item.source, err))?;
    if offset > 0 {
        local
            .seek(SeekFrom::Start(offset))
            .await
            .map_err(|err| fail(&item.source, err))?;
    }

    let flags = if offset > 0 {
        OpenFlags::WRITE | OpenFlags::APPEND
    } else {
        OpenFlags::CREATE | OpenFlags::TRUNCATE | OpenFlags::WRITE
    };
    let mut remote = sftp
        .open_with_flags(&item.destination, flags)
        .await
        .map_err(|err| fail(&item.destination, err))?;
    if offset > 0 {
        // APPEND is advisory on some servers; writing at an explicit offset
        // is what every one of them honours.
        remote
            .seek(SeekFrom::Start(offset))
            .await
            .map_err(|err| fail(&item.destination, err))?;
    }

    let mut buffer = vec![0u8; CHUNK];
    loop {
        gate.wait().await?;
        let read = local
            .read(&mut buffer)
            .await
            .map_err(|err| fail(&item.source, err))?;
        if read == 0 {
            break;
        }
        remote
            .write_all(&buffer[..read])
            .await
            .map_err(|err| fail(&item.destination, err))?;
        gate.progress(read as u64);
    }

    remote
        .flush()
        .await
        .map_err(|err| fail(&item.destination, err))?;
    remote
        .close()
        .await
        .map_err(|err| fail(&item.destination, err))?;

    if let Some(modified) = item.modified {
        let stamp = u32::try_from(modified).unwrap_or(0);
        let attrs = FileAttributes {
            atime: Some(stamp),
            mtime: Some(stamp),
            ..FileAttributes::empty()
        };
        let _ = sftp.set_metadata(&item.destination, attrs).await;
    }
    Ok(())
}

fn set_local_modified(path: &str, seconds: i64) -> std::io::Result<()> {
    let when = UNIX_EPOCH + Duration::from_secs(u64::try_from(seconds).unwrap_or(0));
    std::fs::File::options()
        .write(true)
        .open(path)?
        .set_modified(when)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plans_a_local_tree_parents_first_with_posix_destinations() {
        let dir = std::env::temp_dir().join(format!("harbour-plan-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(dir.join("src").join("nested")).unwrap();
        std::fs::write(dir.join("a.txt"), "aaaa").unwrap();
        std::fs::write(dir.join("src").join("nested").join("b.txt"), "bb").unwrap();

        let plan = plan_upload(&dir.display().to_string(), "/remote/project").unwrap();
        std::fs::remove_dir_all(&dir).ok();

        assert_eq!(plan.total_bytes, 6);
        assert_eq!(
            plan.directories,
            vec![
                "/remote/project",
                "/remote/project/src",
                "/remote/project/src/nested"
            ]
        );
        let destinations: Vec<_> = plan.files.iter().map(|f| f.destination.as_str()).collect();
        assert_eq!(
            destinations,
            vec!["/remote/project/a.txt", "/remote/project/src/nested/b.txt"]
        );
        assert!(plan.files.iter().all(|f| f.modified.is_some()));
    }

    #[test]
    fn a_single_file_plans_as_itself() {
        let dir = std::env::temp_dir().join(format!("harbour-plan1-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("one.bin");
        std::fs::write(&file, [0u8; 10]).unwrap();

        let plan = plan_upload(&file.display().to_string(), "/remote/one.bin").unwrap();
        std::fs::remove_dir_all(&dir).ok();

        assert!(plan.directories.is_empty());
        assert_eq!(plan.files.len(), 1);
        assert_eq!(plan.files[0].size, 10);
        assert_eq!(plan.total_bytes, 10);
    }

    #[test]
    fn a_missing_source_is_a_files_error() {
        let err = plan_upload("/nowhere/at/all", "/remote").unwrap_err();
        assert_eq!(err.code(), "FILES_ERROR");
    }

    #[test]
    fn local_time_stamps_round_trip() {
        let dir = std::env::temp_dir().join(format!("harbour-mtime-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("stamped");
        std::fs::write(&file, "x").unwrap();

        set_local_modified(&file.display().to_string(), 1_600_000_000).unwrap();
        let meta = std::fs::metadata(&file).unwrap();
        std::fs::remove_dir_all(&dir).ok();

        assert_eq!(mtime_seconds(&meta), Some(1_600_000_000));
    }
}
