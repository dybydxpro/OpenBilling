//! Page routing and shell-level callbacks (toasts, confirmations, clipboard).

use app_core::templates;
use slint::ComponentHandle;
use std::rc::Rc;

use crate::ctx::Ctx;
use crate::{
    Api, AppState, customers_ui, dashboard_ui, invoices_ui, orders_ui, reports_ui, settings_ui,
    templates_ui,
};

pub fn init(ctx: &Rc<Ctx>) {
    let ui = ctx.ui();
    let api = ui.global::<Api>();

    let c = ctx.clone();
    api.on_navigate(move |page| navigate(&c, page.as_str()));
    let c = ctx.clone();
    api.on_dismiss_toast(move || c.ui().global::<AppState>().set_toast_visible(false));
    let c = ctx.clone();
    api.on_confirm_accepted(move || {
        c.ui().global::<AppState>().set_confirm_visible(false);
        let action = c.pending_confirm.borrow_mut().take();
        if let Some(action) = action {
            action(&c);
        }
    });
    let c = ctx.clone();
    api.on_confirm_cancelled(move || {
        c.ui().global::<AppState>().set_confirm_visible(false);
        c.pending_confirm.borrow_mut().take();
    });
    let c = ctx.clone();
    api.on_copy_text(move |text| c.copy_text(text.as_str()));
}

/// Loads fresh data for `page` and shows it.
pub fn navigate(ctx: &Rc<Ctx>, page: &str) {
    refresh_business_name(ctx);
    match page {
        "dashboard" => dashboard_ui::refresh(ctx),
        "orders" => orders_ui::refresh_list(ctx),
        "invoices" => invoices_ui::refresh(ctx),
        "customers" => customers_ui::refresh(ctx),
        "templates" => templates_ui::refresh(ctx),
        "reports" => reports_ui::open(ctx),
        "settings" => settings_ui::refresh(ctx),
        _ => {}
    }
    ctx.ui().global::<AppState>().set_page(page.into());
}

pub fn refresh_business_name(ctx: &Ctx) {
    if let Ok(t) = ctx.with_conn(templates::get_default) {
        ctx.ui()
            .global::<AppState>()
            .set_business_name(t.template.business.name.as_str().into());
    }
}
