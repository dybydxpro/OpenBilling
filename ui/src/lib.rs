//! OpenBilling user interface, shared by every platform.
//!
//! The Slint markup lives in `ui/`, and each `*_ui` module connects one area of
//! it to `app-core`. Front-ends only build a [`Platform`] and call [`run`]:
//!
//! ```ignore
//! let platform = app_ui::Platform::desktop();
//! let app = app_core::App::open(&platform.data_dir)?;
//! app_ui::run(platform, app)?;
//! ```

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

pub use files::{FileAccess, Platform};

slint::include_modules!();

/// Builds the main window for `app` and runs the event loop until it closes.
pub fn run(platform: Platform, app: app_core::App) -> Result<(), slint::PlatformError> {
    let ui = MainWindow::new()?;
    if let Some((w, h)) = platform.window_size {
        ui.window().set_size(slint::LogicalSize::new(w, h));
    }
    let ctx = ctx::Ctx::new(app, &ui, platform);

    files::init_ui_state(&ctx);
    nav::init(&ctx);
    auth_ui::init(&ctx);
    dashboard_ui::init(&ctx);
    orders_ui::init(&ctx);
    invoices_ui::init(&ctx);
    customers_ui::init(&ctx);
    templates_ui::init(&ctx);
    reports_ui::init(&ctx);
    settings_ui::init(&ctx);

    ui.run()
}
