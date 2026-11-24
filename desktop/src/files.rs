//! Native file dialogs and file helpers (desktop only).

use std::path::{Path, PathBuf};

pub fn data_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("OPENBILLING_DATA_DIR") {
        return PathBuf::from(dir);
    }
    directories::ProjectDirs::from("com", "OpenBilling", "OpenBilling")
        .map(|d| d.data_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from("openbilling-data"))
}

fn documents_dir() -> Option<PathBuf> {
    directories::UserDirs::new().and_then(|u| u.document_dir().map(Path::to_path_buf))
}

pub fn save_dialog(title: &str, file_name: &str, filter: (&str, &[&str])) -> Option<PathBuf> {
    let mut dialog = rfd::FileDialog::new()
        .set_title(title)
        .set_file_name(file_name)
        .add_filter(filter.0, filter.1);
    if let Some(dir) = documents_dir() {
        dialog = dialog.set_directory(dir);
    }
    dialog.save_file()
}

pub fn open_dialog(title: &str, filter: (&str, &[&str])) -> Option<PathBuf> {
    let mut dialog = rfd::FileDialog::new().set_title(title).add_filter(filter.0, filter.1);
    if let Some(dir) = documents_dir() {
        dialog = dialog.set_directory(dir);
    }
    dialog.pick_file()
}

pub fn display_name(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()
}

/// Writes a file to a private temp folder and opens it with the default app.
pub fn open_temp(file_name: &str, bytes: &[u8]) -> std::io::Result<()> {
    let dir = std::env::temp_dir().join("OpenBilling");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(file_name);
    std::fs::write(&path, bytes)?;
    open::that_detached(&path)
}
