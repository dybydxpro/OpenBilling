//! OpenBilling core: all business logic, independent of any UI toolkit or
//! platform. Front-ends (desktop, mobile) hold an [`App`] and call the module
//! functions with [`App::conn`].

pub mod auth;
pub mod backup;
pub mod customers;
pub mod db;
pub mod error;
pub mod invoices;
pub mod money;
pub mod orders;
pub mod pdf;
pub mod reports;
pub mod templates;

pub use error::{CoreError, Result};
pub use rusqlite;

use rusqlite::Connection;
use std::path::{Path, PathBuf};

pub const APP_NAME: &str = "OpenBilling";
pub const DB_FILE: &str = "openbilling.db";

pub fn app_name() -> &'static str {
    APP_NAME
}

pub struct App {
    conn: Connection,
    db_path: Option<PathBuf>,
}

impl App {
    /// Opens (creating if needed) the database in `data_dir`.
    pub fn open(data_dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(data_dir)?;
        let db_path = data_dir.join(DB_FILE);
        let conn = db::open(&db_path)?;
        templates::default_id(&conn)?;
        Ok(Self { conn, db_path: Some(db_path) })
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = db::open_in_memory()?;
        templates::default_id(&conn)?;
        Ok(Self { conn, db_path: None })
    }

    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    pub fn db_path(&self) -> Option<&Path> {
        self.db_path.as_deref()
    }

    /// Replaces all data with the contents of a backup archive.
    ///
    /// The current database is kept next to the live one as
    /// `openbilling.db.before-restore` until the next restore. Afterwards the
    /// user must sign in with the PIN that belongs to the backup.
    pub fn restore_backup(&mut self, backup_file: &Path) -> Result<backup::BackupManifest> {
        let db_path = self
            .db_path
            .clone()
            .ok_or_else(|| CoreError::Validation("Restore needs an on-disk database.".into()))?;
        let staged = db_path.with_extension("db.restore-staging");
        let manifest = backup::extract_verified_db(backup_file, &staged)?;

        // Close the live connection so SQLite checkpoints and releases the WAL.
        let old = std::mem::replace(&mut self.conn, db::open_in_memory()?);
        if let Err((conn, e)) = old.close() {
            self.conn = conn;
            let _ = std::fs::remove_file(&staged);
            return Err(e.into());
        }

        let swap = (|| -> Result<()> {
            let safety = db_path.with_extension("db.before-restore");
            if db_path.exists() {
                std::fs::copy(&db_path, &safety)?;
            }
            for suffix in ["-wal", "-shm"] {
                let side = PathBuf::from(format!("{}{suffix}", db_path.display()));
                if side.exists() {
                    std::fs::remove_file(side)?;
                }
            }
            std::fs::copy(&staged, &db_path)?;
            Ok(())
        })();
        let _ = std::fs::remove_file(&staged);

        // Reopen whatever is now on disk (restored, or the original on failure)
        // and bring its schema up to date.
        self.conn = db::open(&db_path)?;
        templates::default_id(&self.conn)?;
        swap.map(|_| manifest)
    }
}

#[cfg(test)]
mod tests;
