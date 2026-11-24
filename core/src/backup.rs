//! Full backup and restore.
//!
//! A backup is a zip archive containing `manifest.json` and a consistent copy
//! of the SQLite database made with `VACUUM INTO`. Restoring replaces the
//! whole database, including the PIN and recovery key that were active when
//! the backup was taken.

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use zip::write::SimpleFileOptions;

use crate::db::{self, SCHEMA_VERSION, now_string};
use crate::error::{CoreError, Result, invalid};

pub const FILE_EXTENSION: &str = "obbackup";
const MANIFEST: &str = "manifest.json";
const DB_ENTRY: &str = "openbilling.db";
const FORMAT: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BackupManifest {
    pub app: String,
    pub format: u32,
    pub schema_version: i64,
    pub created_at: String,
    pub customers: i64,
    pub orders: i64,
    pub invoices: i64,
    pub templates: i64,
}

fn count(conn: &Connection, table: &str) -> Result<i64> {
    Ok(conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))?)
}

fn manifest_for(conn: &Connection) -> Result<BackupManifest> {
    Ok(BackupManifest {
        app: "OpenBilling".into(),
        format: FORMAT,
        schema_version: db::schema_version(conn)?,
        created_at: now_string(),
        customers: count(conn, "customers")?,
        orders: count(conn, "orders")?,
        invoices: count(conn, "invoices")?,
        templates: count(conn, "templates")?,
    })
}

/// Unique scratch file next to `near` (same volume, so renames are cheap).
fn scratch_path(near: &Path, tag: &str) -> PathBuf {
    let dir = near
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(std::env::temp_dir);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    dir.join(format!(
        ".openbilling-{tag}-{}-{nanos}.tmp",
        std::process::id()
    ))
}

/// Writes a backup archive to `dest`.
pub fn create_backup(conn: &Connection, dest: &Path) -> Result<BackupManifest> {
    let manifest = manifest_for(conn)?;
    let snapshot = scratch_path(dest, "snapshot");
    let result = (|| -> Result<()> {
        conn.execute("VACUUM INTO ?1", [snapshot.to_string_lossy().as_ref()])?;
        let partial = scratch_path(dest, "partial");
        {
            let mut zip = zip::ZipWriter::new(File::create(&partial)?);
            let opts =
                SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
            zip.start_file(MANIFEST, opts)?;
            zip.write_all(serde_json::to_string_pretty(&manifest)?.as_bytes())?;
            zip.start_file(DB_ENTRY, opts)?;
            std::io::copy(&mut File::open(&snapshot)?, &mut zip)?;
            zip.finish()?.sync_all()?;
        }
        // Only replace the destination once the archive is complete.
        std::fs::rename(&partial, dest).or_else(|_| {
            std::fs::copy(&partial, dest)?;
            std::fs::remove_file(&partial)
        })?;
        Ok(())
    })();
    let _ = std::fs::remove_file(&snapshot);
    result.map(|_| manifest)
}

/// Reads and checks a backup's manifest without changing anything.
pub fn inspect_backup(path: &Path) -> Result<BackupManifest> {
    let mut zip = zip::ZipArchive::new(File::open(path)?)
        .map_err(|_| CoreError::Validation("That file is not an OpenBilling backup.".into()))?;
    let mut text = String::new();
    zip.by_name(MANIFEST)
        .map_err(|_| CoreError::Validation("That file is not an OpenBilling backup.".into()))?
        .read_to_string(&mut text)?;
    let manifest: BackupManifest = serde_json::from_str(&text)?;
    if manifest.app != "OpenBilling" || manifest.format > FORMAT {
        return invalid("This backup was made by a newer or different app.");
    }
    if manifest.schema_version > SCHEMA_VERSION {
        return invalid(
            "This backup was made by a newer version of OpenBilling. Update the app first.",
        );
    }
    Ok(manifest)
}

/// Extracts the database from a backup into `out` and verifies it.
pub(crate) fn extract_verified_db(backup: &Path, out: &Path) -> Result<BackupManifest> {
    let manifest = inspect_backup(backup)?;
    let mut zip = zip::ZipArchive::new(File::open(backup)?)?;
    {
        let mut entry = zip
            .by_name(DB_ENTRY)
            .map_err(|_| CoreError::Validation("The backup is missing its database.".into()))?;
        let mut file = File::create(out)?;
        std::io::copy(&mut entry, &mut file)?;
        file.sync_all()?;
    }
    let check = (|| -> Result<()> {
        let conn = Connection::open(out)?;
        let ok: String = conn.query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
        if ok != "ok" {
            return invalid("The backup's database is damaged and can't be restored.");
        }
        let has_auth: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='settings')",
            [],
            |r| r.get(0),
        )?;
        if !has_auth {
            return invalid("The backup's database is not an OpenBilling database.");
        }
        Ok(())
    })();
    if let Err(e) = check {
        let _ = std::fs::remove_file(out);
        return Err(match e {
            CoreError::Db(_) => CoreError::Validation(
                "The backup's database is damaged and can't be restored.".into(),
            ),
            other => other,
        });
    }
    Ok(manifest)
}
