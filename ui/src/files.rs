//! Platform-specific file handling.
//!
//! * **Desktop** (`desktop` feature): native save/open dialogs, and previews
//!   open in the system's default app.
//! * **Mobile**: there are no native dialogs, so files are written to and read
//!   from one export folder that the user can reach (on iOS through the Files
//!   app, on Android under `Android/data/<app>/files`). Picking a file shows an
//!   in-app list of that folder.

use slint::ComponentHandle;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use crate::convert::model;
use crate::ctx::Ctx;
use crate::{AppState, FilePicker, FileRow};

/// Where and how the app stores and exchanges files on this platform.
pub struct Platform {
    /// Private folder for the database.
    pub data_dir: PathBuf,
    pub files: FileAccess,
    /// Phone/tablet build: touch-first UI tweaks.
    pub mobile: bool,
    /// Initial window size in logical pixels (used by the phone-sized preview).
    pub window_size: Option<(f32, f32)>,
}

pub enum FileAccess {
    /// OS file dialogs.
    #[cfg(feature = "desktop")]
    Native,
    /// Everything goes through this folder.
    Folder(PathBuf),
}

/// `("PDF document", &["pdf"])`
pub type Filter<'a> = (&'a str, &'a [&'a str]);

type PickAction = Box<dyn FnOnce(&Rc<Ctx>, PathBuf)>;

pub struct PendingPick {
    files: Vec<PathBuf>,
    action: PickAction,
}

impl Platform {
    /// Desktop defaults: OS data folder (or `OPENBILLING_DATA_DIR`) and native dialogs.
    #[cfg(feature = "desktop")]
    pub fn desktop() -> Self {
        let data_dir = std::env::var("OPENBILLING_DATA_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                directories::ProjectDirs::from("com", "OpenBilling", "OpenBilling")
                    .map(|d| d.data_dir().to_path_buf())
                    .unwrap_or_else(|| PathBuf::from("openbilling-data"))
            });
        Self { data_dir, files: FileAccess::Native, mobile: false, window_size: None }
    }

    pub fn export_folder(&self) -> Option<&Path> {
        match &self.files {
            FileAccess::Folder(dir) => Some(dir),
            #[cfg(feature = "desktop")]
            FileAccess::Native => None,
        }
    }
}

pub fn display_name(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()
}

/// `name.pdf` -> `name (2).pdf` until the name is free.
fn unique_path(dir: &Path, file_name: &str) -> PathBuf {
    let candidate = dir.join(file_name);
    if !candidate.exists() {
        return candidate;
    }
    let p = Path::new(file_name);
    let stem = p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let ext = p.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    (2..)
        .map(|n| dir.join(format!("{stem} ({n}){ext}")))
        .find(|p| !p.exists())
        .expect("some name is free")
}

/// Destination for a new file: a save dialog on desktop, a free name in the
/// export folder on mobile. `None` if the user cancelled.
pub fn choose_save_path(ctx: &Ctx, title: &str, file_name: &str, filter: Filter) -> Option<PathBuf> {
    Some(match &ctx.platform.files {
        #[cfg(feature = "desktop")]
        FileAccess::Native => {
            let mut dialog = rfd::FileDialog::new()
                .set_title(title)
                .set_file_name(file_name)
                .add_filter(filter.0, filter.1);
            if let Some(dir) = directories::UserDirs::new().and_then(|u| u.document_dir().map(Path::to_path_buf)) {
                dialog = dialog.set_directory(dir);
            }
            dialog.save_file()?
        }
        FileAccess::Folder(dir) => {
            let _ = (title, filter);
            if let Err(e) = std::fs::create_dir_all(dir) {
                ctx.toast("error", format!("Couldn't create the export folder: {e}"));
                return None;
            }
            unique_path(dir, file_name)
        }
    })
}

/// Short description of where a file went, for toasts.
pub fn saved_label(ctx: &Ctx, path: &Path) -> String {
    match &ctx.platform.files {
        FileAccess::Folder(_) => format!("OpenBilling/{}", display_name(path)),
        #[cfg(feature = "desktop")]
        FileAccess::Native => display_name(path),
    }
}

/// Saves `bytes` to a user-chosen place and shows a toast. Returns the path,
/// or `None` if cancelled or failed.
pub fn save(ctx: &Ctx, title: &str, file_name: &str, filter: Filter, bytes: &[u8]) -> Option<PathBuf> {
    let path = choose_save_path(ctx, title, file_name, filter)?;
    match std::fs::write(&path, bytes) {
        Ok(()) => {
            ctx.success(format!("Saved {}", saved_label(ctx, &path)));
            Some(path)
        }
        Err(e) => {
            ctx.toast("error", format!("Couldn't save the file: {e}"));
            None
        }
    }
}

/// Lets the user choose an existing file, then runs `action` with it.
pub fn pick(ctx: &Rc<Ctx>, title: &str, filter: Filter, action: impl FnOnce(&Rc<Ctx>, PathBuf) + 'static) {
    match &ctx.platform.files {
        #[cfg(feature = "desktop")]
        FileAccess::Native => {
            let mut dialog = rfd::FileDialog::new().set_title(title).add_filter(filter.0, filter.1);
            if let Some(dir) = directories::UserDirs::new().and_then(|u| u.document_dir().map(Path::to_path_buf)) {
                dialog = dialog.set_directory(dir);
            }
            if let Some(path) = dialog.pick_file() {
                action(ctx, path);
            }
        }
        FileAccess::Folder(dir) => {
            let _ = std::fs::create_dir_all(dir);
            let mut files: Vec<(PathBuf, std::fs::Metadata)> = std::fs::read_dir(dir)
                .map(|entries| {
                    entries
                        .filter_map(Result::ok)
                        .filter_map(|e| e.metadata().ok().map(|m| (e.path(), m)))
                        .filter(|(p, m)| {
                            m.is_file()
                                && p.extension().is_some_and(|ext| {
                                    filter.1.iter().any(|f| ext.eq_ignore_ascii_case(f))
                                })
                        })
                        .collect()
                })
                .unwrap_or_default();
            files.sort_by_key(|(_, m)| std::cmp::Reverse(m.modified().ok()));

            let rows: Vec<FileRow> = files
                .iter()
                .map(|(p, m)| FileRow {
                    name: display_name(p).into(),
                    detail: format!(
                        "{} · {}",
                        m.modified()
                            .ok()
                            .map(|t| chrono::DateTime::<chrono::Local>::from(t).format("%Y-%m-%d %H:%M").to_string())
                            .unwrap_or_default(),
                        human_size(m.len())
                    )
                    .into(),
                })
                .collect();

            let ui = ctx.ui();
            let fp = ui.global::<FilePicker>();
            fp.set_title(title.into());
            fp.set_files(model(rows));
            fp.set_empty_text(
                format!(
                    "No {} files yet. Copy one into the OpenBilling folder, then try again.",
                    filter.1.iter().map(|e| format!(".{e}")).collect::<Vec<_>>().join(" / ")
                )
                .into(),
            );
            fp.set_visible(true);
            *ctx.pending_pick.borrow_mut() = Some(PendingPick {
                files: files.into_iter().map(|(p, _)| p).collect(),
                action: Box::new(action),
            });
        }
    }
}

/// Called by the in-app picker.
pub fn picked(ctx: &Rc<Ctx>, index: i32) {
    ctx.ui().global::<FilePicker>().set_visible(false);
    let pending = ctx.pending_pick.borrow_mut().take();
    if let Some(PendingPick { files, action }) = pending
        && let Some(path) = usize::try_from(index).ok().and_then(|i| files.get(i)) {
            action(ctx, path.clone());
        }
}

pub fn cancel_pick(ctx: &Ctx) {
    ctx.ui().global::<FilePicker>().set_visible(false);
    ctx.pending_pick.borrow_mut().take();
}

/// Shows a generated file: opens it on desktop, saves it on mobile.
pub fn preview(ctx: &Ctx, file_name: &str, bytes: &[u8]) {
    match &ctx.platform.files {
        #[cfg(feature = "desktop")]
        FileAccess::Native => {
            let result = (|| {
                let dir = std::env::temp_dir().join("OpenBilling");
                std::fs::create_dir_all(&dir)?;
                let path = dir.join(file_name);
                std::fs::write(&path, bytes)?;
                open::that_detached(&path)
            })();
            if let Err(e) = result {
                ctx.toast("error", format!("Couldn't open the file: {e}"));
            }
        }
        FileAccess::Folder(_) => {
            save(ctx, "", file_name, ("", &[]), bytes);
        }
    }
}

/// Opens a folder in the OS file manager (desktop only).
pub fn open_folder(ctx: &Ctx, path: &Path) {
    #[cfg(feature = "desktop")]
    if let FileAccess::Native = ctx.platform.files {
        if let Err(e) = open::that_detached(path) {
            ctx.toast("error", format!("Couldn't open the folder: {e}"));
        }
        return;
    }
    ctx.toast("info", path.display().to_string());
}

fn human_size(bytes: u64) -> String {
    match bytes {
        b if b >= 1 << 20 => format!("{:.1} MB", b as f64 / (1 << 20) as f64),
        b if b >= 1 << 10 => format!("{:.0} KB", b as f64 / (1 << 10) as f64),
        b => format!("{b} B"),
    }
}

pub fn init_ui_state(ctx: &Ctx) {
    let ui = ctx.ui();
    let s = ui.global::<AppState>();
    s.set_mobile(ctx.platform.mobile);
    s.set_export_folder(
        ctx.platform
            .export_folder()
            .map(|p| p.display().to_string())
            .unwrap_or_default()
            .into(),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unique_names() {
        let dir = std::env::temp_dir().join(format!("ob-unique-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.pdf"), b"x").unwrap();
        assert_eq!(unique_path(&dir, "a.pdf"), dir.join("a (2).pdf"));
        assert_eq!(unique_path(&dir, "b.pdf"), dir.join("b.pdf"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn sizes() {
        assert_eq!(human_size(512), "512 B");
        assert_eq!(human_size(2048), "2 KB");
        assert_eq!(human_size(3 << 20), "3.0 MB");
    }
}
