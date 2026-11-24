use app_core::invoices;
use slint::ComponentHandle;
use std::rc::Rc;

use crate::convert::{invoice_row, model};
use crate::ctx::Ctx;
use crate::{Api, InvoicesState, orders_ui};

pub fn init(ctx: &Rc<Ctx>) {
    let ui = ctx.ui();
    let api = ui.global::<Api>();
    let c = ctx.clone();
    api.on_invoices_search(move || refresh(&c));
    let c = ctx.clone();
    api.on_download_latest(move |invoice_id| {
        match c.with_conn(|conn| invoices::latest_version_id(conn, invoice_id as i64)) {
            Ok(vid) => orders_ui::download_version(&c, vid),
            Err(e) => c.fail(&e),
        }
    });
}

pub fn refresh(ctx: &Rc<Ctx>) {
    let ui_ = ctx.ui();
    let s = ui_.global::<InvoicesState>();
    let search = s.get_search().to_string();
    match ctx.with_conn(|c| invoices::list(c, &search)) {
        Ok(list) => {
            let sym = ctx.symbol();
            s.set_rows(model(list.iter().map(|i| invoice_row(i, &sym)).collect()));
        }
        Err(e) => ctx.fail(&e),
    }
}
