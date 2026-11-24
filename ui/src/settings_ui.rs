//! Backup & restore, PIN change and recovery key rotation.

use app_core::backup::{self, FILE_EXTENSION};
use app_core::db::{get_setting, now_string, set_setting};
use app_core::{auth, customers, invoices, orders, templates};
use chrono::Local;
use slint::{ComponentHandle, SharedString};
use std::rc::Rc;

use crate::ctx::Ctx;
use crate::{Api, SettingsState, auth_ui, files};

const LAST_BACKUP_KEY: &str = "backup.last";

pub fn init(ctx: &Rc<Ctx>) {
    let ui = ctx.ui();
    let api = ui.global::<Api>();
    let c = ctx.clone();
    api.on_create_backup(move || create_backup(&c));
    let c = ctx.clone();
    api.on_restore_backup(move || restore_backup(&c));
    let c = ctx.clone();
    api.on_change_pin(move || change_pin(&c));
    let c = ctx.clone();
    api.on_regenerate_key(move || regenerate_key(&c));
    let c = ctx.clone();
    api.on_open_data_dir(move || files::open_folder(&c, &c.platform.data_dir));
}

pub fn refresh(ctx: &Rc<Ctx>) {
    let ui_ = ctx.ui();
    let s = ui_.global::<SettingsState>();
    s.set_data_dir(ctx.platform.data_dir.display().to_string().into());
    let stats = ctx.with_conn(|c| -> app_core::Result<String> {
        let customers = customers::list(c, "")?.len();
        let orders = orders::list(c, &Default::default())?.len();
        let invoices = invoices::list(c, "")?.len();
        let templates = templates::list(c)?.len();
        Ok(format!(
            "{customers} customers · {orders} orders · {invoices} invoices · {templates} templates, stored in a local SQLite database."
        ))
    });
    s.set_stats(stats.unwrap_or_default().into());
    let last = ctx
        .with_conn(|c| get_setting(c, LAST_BACKUP_KEY))
        .ok()
        .flatten();
    s.set_last_backup(last.unwrap_or_else(|| "Never".into()).into());
}

/// Clears PIN fields and any recovery key on screen (used when locking).
pub fn clear_sensitive(ctx: &Ctx) {
    let ui_ = ctx.ui();
    let s = ui_.global::<SettingsState>();
    for set in [
        SettingsState::set_current_pin,
        SettingsState::set_new_pin,
        SettingsState::set_confirm_pin,
        SettingsState::set_key_pin,
        SettingsState::set_new_key,
        SettingsState::set_pin_error,
        SettingsState::set_key_error,
    ] {
        set(&s, SharedString::new());
    }
}

fn create_backup(ctx: &Rc<Ctx>) {
    let name = format!(
        "OpenBilling-backup-{}.{FILE_EXTENSION}",
        Local::now().format("%Y-%m-%d")
    );
    let Some(path) = files::choose_save_path(
        ctx,
        "Save backup",
        &name,
        ("OpenBilling backup", &[FILE_EXTENSION]),
    ) else {
        return;
    };
    match ctx.with_conn(|c| backup::create_backup(c, &path)) {
        Ok(m) => {
            let _ = ctx.with_conn(|c| set_setting(c, LAST_BACKUP_KEY, &now_string()));
            ctx.success(format!(
                "Backup saved to {}: {} orders, {} invoices, {} customers",
                files::saved_label(ctx, &path),
                m.orders,
                m.invoices,
                m.customers
            ));
            refresh(ctx);
        }
        Err(e) => ctx.fail(&e),
    }
}

fn restore_backup(ctx: &Rc<Ctx>) {
    files::pick(
        ctx,
        "Restore backup",
        ("OpenBilling backup", &[FILE_EXTENSION]),
        confirm_restore,
    );
}

fn confirm_restore(ctx: &Rc<Ctx>, path: std::path::PathBuf) {
    let manifest = match backup::inspect_backup(&path) {
        Ok(m) => m,
        Err(e) => return ctx.fail(&e),
    };
    let message = format!(
        "Backup from {} with {} customers, {} orders and {} invoices.\n\nAll current data will be replaced, including your PIN and recovery key. You'll sign in with the PIN that was active when this backup was made.",
        manifest.created_at, manifest.customers, manifest.orders, manifest.invoices
    );
    ctx.confirm(
        "Restore this backup?",
        &message,
        "Replace my data",
        true,
        move |c| {
            let result = c.app.borrow_mut().restore_backup(&path);
            match result {
                Ok(_) => {
                    auth_ui::lock(c);
                    c.success("Backup restored. Sign in with the backup's PIN.");
                }
                Err(e) => c.fail(&e),
            }
        },
    );
}

fn change_pin(ctx: &Rc<Ctx>) {
    let ui_ = ctx.ui();
    let s = ui_.global::<SettingsState>();
    let (current, new, confirm) = (s.get_current_pin(), s.get_new_pin(), s.get_confirm_pin());
    match ctx.with_conn(|c| auth::change_pin(c, &current, &new, &confirm)) {
        Ok(()) => {
            clear_sensitive(ctx);
            ctx.success("PIN updated");
        }
        Err(e) => s.set_pin_error(e.to_string().into()),
    }
}

fn regenerate_key(ctx: &Rc<Ctx>) {
    let ui_ = ctx.ui();
    let s = ui_.global::<SettingsState>();
    let pin = s.get_key_pin();
    match ctx.with_conn(|c| auth::regenerate_recovery_key(c, &pin)) {
        Ok(key) => {
            s.set_key_pin(SharedString::new());
            s.set_key_error(SharedString::new());
            s.set_new_key(key.into());
        }
        Err(e) => s.set_key_error(e.to_string().into()),
    }
}
