use app_core::customers::{self, CustomerInput};
use slint::{ComponentHandle, SharedString};
use std::rc::Rc;

use crate::convert::{customer_row, model};
use crate::ctx::Ctx;
use crate::{Api, CustomersState, orders_ui};

pub fn init(ctx: &Rc<Ctx>) {
    let ui = ctx.ui();
    let api = ui.global::<Api>();

    let c = ctx.clone();
    api.on_customers_search(move || refresh(&c));
    let c = ctx.clone();
    api.on_new_customer(move |from_order| open_dialog(&c, None, from_order));
    let c = ctx.clone();
    api.on_edit_customer(move |id| open_dialog(&c, Some(id as i64), false));
    let c = ctx.clone();
    api.on_customer_cancel(move || c.ui().global::<CustomersState>().set_dialog_visible(false));
    let c = ctx.clone();
    api.on_customer_save(move || save(&c));
    let c = ctx.clone();
    api.on_delete_customer(move |id| delete(&c, id as i64));
    let c = ctx.clone();
    api.on_customer_orders(move |id| {
        if let Ok(cust) = c.with_conn(|conn| customers::get(conn, id as i64)) {
            orders_ui::show_for_customer(&c, &cust.name);
        }
    });
}

pub fn refresh(ctx: &Rc<Ctx>) {
    let ui_ = ctx.ui();
    let s = ui_.global::<CustomersState>();
    let search = s.get_search().to_string();
    match ctx.with_conn(|c| customers::list(c, &search)) {
        Ok(list) => {
            let sym = ctx.symbol();
            s.set_rows(model(list.iter().map(|c| customer_row(c, &sym)).collect()));
        }
        Err(e) => ctx.fail(&e),
    }
}

fn open_dialog(ctx: &Rc<Ctx>, id: Option<i64>, from_order: bool) {
    let ui_ = ctx.ui();
    let s = ui_.global::<CustomersState>();
    let existing = match id {
        Some(id) => match ctx.with_conn(|c| customers::get(c, id)) {
            Ok(c) => Some(c),
            Err(e) => return ctx.fail(&e),
        },
        None => None,
    };
    s.set_edit_id(id.unwrap_or(0) as i32);
    s.set_from_order(from_order);
    s.set_name(
        existing
            .as_ref()
            .map(|c| c.name.clone())
            .unwrap_or_default()
            .into(),
    );
    s.set_email(
        existing
            .as_ref()
            .map(|c| c.email.clone())
            .unwrap_or_default()
            .into(),
    );
    s.set_phone(
        existing
            .as_ref()
            .map(|c| c.phone.clone())
            .unwrap_or_default()
            .into(),
    );
    s.set_address(
        existing
            .as_ref()
            .map(|c| c.address.clone())
            .unwrap_or_default()
            .into(),
    );
    s.set_error(SharedString::new());
    s.set_dialog_visible(true);
}

fn save(ctx: &Rc<Ctx>) {
    let ui_ = ctx.ui();
    let s = ui_.global::<CustomersState>();
    let input = CustomerInput {
        name: s.get_name().to_string(),
        email: s.get_email().to_string(),
        phone: s.get_phone().to_string(),
        address: s.get_address().to_string(),
    };
    let edit_id = s.get_edit_id() as i64;
    let result = ctx.with_conn(|c| {
        if edit_id == 0 {
            customers::create(c, &input)
        } else {
            customers::update(c, edit_id, &input).map(|_| edit_id)
        }
    });
    match result {
        Ok(id) => {
            s.set_dialog_visible(false);
            ctx.success(if edit_id == 0 {
                "Customer added"
            } else {
                "Customer updated"
            });
            if s.get_from_order() {
                orders_ui::load_customer_choices(ctx, Some(id));
            } else {
                refresh(ctx);
            }
        }
        Err(e) => s.set_error(e.to_string().into()),
    }
}

fn delete(ctx: &Rc<Ctx>, id: i64) {
    let name = ctx
        .with_conn(|c| customers::get(c, id))
        .map(|c| c.name)
        .unwrap_or_default();
    ctx.confirm(
        &format!("Delete {name}?"),
        "The customer will be removed permanently. Customers with orders can't be deleted.",
        "Delete",
        true,
        move |c| match c.with_conn(|conn| customers::delete(conn, id)) {
            Ok(()) => {
                c.success("Customer deleted");
                refresh(c);
            }
            Err(e) => c.fail(&e),
        },
    );
}
