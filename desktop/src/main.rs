// Prevent console window in addition to Slint window in Windows release builds when, e.g., starting the app via file manager. Ignored on other platforms.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod auth_ui;
mod convert;
mod ctx;
mod customers_ui;
mod dashboard_ui;
mod files;
mod invoices_ui;
mod nav;
mod orders_ui;
mod reports_ui;
mod settings_ui;
mod templates_ui;

use app_core::App;
use std::error::Error;

slint::include_modules!();

fn main() -> Result<(), Box<dyn Error>> {
    let data_dir = files::data_dir();
    let app = match App::open(&data_dir) {
        Ok(app) => app,
        Err(e) => {
            rfd::MessageDialog::new()
                .set_level(rfd::MessageLevel::Error)
                .set_title("OpenBilling can't start")
                .set_description(format!(
                    "The database in {} couldn't be opened:\n\n{e}",
                    data_dir.display()
                ))
                .show();
            return Err(e.into());
        }
    };

    let ui = MainWindow::new()?;
    let ctx = ctx::Ctx::new(app, &ui, data_dir);

    nav::init(&ctx);
    auth_ui::init(&ctx);
    dashboard_ui::init(&ctx);
    orders_ui::init(&ctx);
    invoices_ui::init(&ctx);
    customers_ui::init(&ctx);
    templates_ui::init(&ctx);
    reports_ui::init(&ctx);
    settings_ui::init(&ctx);

    ui.run()?;
    Ok(())
}
