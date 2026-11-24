//! OpenBilling for phones and tablets.
//!
//! The UI and business logic are shared with the desktop app (`app-ui`,
//! `app-core`); this crate only provides the platform entry points and decides
//! where data and exported files live:
//!
//! | Platform | Database                                | Exports / imports (user-visible)            |
//! |----------|-----------------------------------------|---------------------------------------------|
//! | Android  | app-private internal storage            | `Android/data/com.openbilling.app/files/OpenBilling` |
//! | iOS      | `Library/Application Support/OpenBilling` | `Documents/OpenBilling` (Files app → On My iPhone → OpenBilling) |

use app_ui::{FileAccess, Platform};
use std::path::PathBuf;

/// Opens the database in `data_dir` and runs the app until it exits.
/// `export_dir` is where invoices, reports and backups are written, and where
/// files to import or restore are picked from.
pub fn run(
    data_dir: PathBuf,
    export_dir: PathBuf,
    window_size: Option<(f32, f32)>,
) -> Result<(), Box<dyn std::error::Error>> {
    let platform = Platform {
        data_dir,
        files: FileAccess::Folder(export_dir),
        mobile: true,
        window_size,
    };
    let app = app_core::App::open(&platform.data_dir)?;
    app_ui::run(platform, app)?;
    Ok(())
}

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
fn android_main(app: slint::android::AndroidApp) {
    let data_dir = app
        .internal_data_path()
        .unwrap_or_else(|| PathBuf::from("/data/local/tmp/openbilling"));
    let export_dir = app
        .external_data_path()
        .map(|p| p.join("OpenBilling"))
        .unwrap_or_else(|| data_dir.join("exports"));

    // Write panics next to the database so they can be pulled with
    // `adb shell run-as com.openbilling.app cat files/crash.log`.
    let crash_log = data_dir.join("crash.log");
    std::panic::set_hook(Box::new(move |info| {
        let _ = std::fs::write(&crash_log, format!("{info}\n"));
    }));

    slint::android::init(app).expect("failed to initialise the Slint Android backend");
    if let Err(e) = run(data_dir.clone(), export_dir, None) {
        let _ = std::fs::write(data_dir.join("startup-error.log"), e.to_string());
    }
}
