// Prevent console window in addition to Slint window in Windows release builds when, e.g., starting the app via file manager. Ignored on other platforms.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use app_core::App;
use app_ui::Platform;
use std::error::Error;

fn main() -> Result<(), Box<dyn Error>> {
    let platform = Platform::desktop();
    let app = match App::open(&platform.data_dir) {
        Ok(app) => app,
        Err(e) => {
            rfd::MessageDialog::new()
                .set_level(rfd::MessageLevel::Error)
                .set_title("OpenBilling can't start")
                .set_description(format!(
                    "The database in {} couldn't be opened:\n\n{e}",
                    platform.data_dir.display()
                ))
                .show();
            return Err(e.into());
        }
    };
    app_ui::run(platform, app)?;
    Ok(())
}
