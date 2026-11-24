//! Orders: list, editor, detail view and invoice generation.

use app_core::db::today_string;
use app_core::money::{
    format_amount, format_money, format_percent, format_quantity, parse_money, parse_percent,
    parse_quantity,
};
use app_core::orders::{self, OrderFilter, OrderInput, OrderItem, OrderStatus, compute_totals};
use app_core::{customers, invoices, pdf, templates};
use chrono::{Duration as Days, Local};
use slint::{ComponentHandle, Model, SharedString};
use std::rc::Rc;

use crate::convert::{model, order_rows, strings, version_rows};
use crate::ctx::{Ctx, ItemDraft, status_from_index, status_index};
use crate::{Api, ItemRow, OrderForm, OrderView, OrdersState, files, nav};

pub fn init(ctx: &Rc<Ctx>) {
    let ui = ctx.ui();
    let api = ui.global::<Api>();

    let c = ctx.clone();
    api.on_orders_search(move || refresh_list(&c));
    let c = ctx.clone();
    api.on_new_order(move || open_editor(&c, None));
    let c = ctx.clone();
    api.on_edit_order(move |id| open_editor(&c, Some(id as i64)));
    let c = ctx.clone();
    api.on_open_order(move |id| open_view(&c, id as i64));
    let c = ctx.clone();
    api.on_delete_order(move |id| delete_order(&c, id as i64));
    let c = ctx.clone();
    api.on_set_order_status(move |id, status| set_status(&c, id as i64, status.as_str()));

    let c = ctx.clone();
    api.on_order_item_edited(move |i, field, value| item_edited(&c, i as usize, &field, &value));
    let c = ctx.clone();
    api.on_order_add_item(move || {
        c.draft.borrow_mut().items.push(ItemDraft { quantity: "1".into(), ..Default::default() });
        c.items_model.push(ItemRow { quantity: "1".into(), ..Default::default() });
        recalc(&c);
    });
    let c = ctx.clone();
    api.on_order_remove_item(move |i| {
        let i = i as usize;
        if i < c.items_model.row_count() && c.items_model.row_count() > 1 {
            c.draft.borrow_mut().items.remove(i);
            c.items_model.remove(i);
            recalc(&c);
        }
    });
    let c = ctx.clone();
    api.on_order_recalc(move || recalc(&c));
    api.on_order_customer_chosen(|_| {});
    let c = ctx.clone();
    api.on_order_template_chosen(move |i| template_chosen(&c, i));
    let c = ctx.clone();
    api.on_order_save(move || save(&c));
    let c = ctx.clone();
    api.on_order_cancel(move || {
        let id = c.draft.borrow().id;
        match id {
            Some(id) => open_view(&c, id),
            None => nav::navigate(&c, "orders"),
        }
    });

    let c = ctx.clone();
    api.on_generate_invoice(move || generate_invoice(&c));
    let c = ctx.clone();
    api.on_download_version(move |id| download_version(&c, id as i64));
    let c = ctx.clone();
    api.on_open_version(move |id| open_version(&c, id as i64));
}

// ---------------------------------------------------------------------------
// List
// ---------------------------------------------------------------------------

pub fn refresh_list(ctx: &Rc<Ctx>) {
    let ui = ctx.ui();
    let s = ui.global::<OrdersState>();
    let filter_key = s.get_status_filter().to_string();
    let filter = OrderFilter {
        search: s.get_search().to_string(),
        status: match filter_key.as_str() {
            "overdue" => Some(OrderStatus::Confirmed),
            other => OrderStatus::parse(other),
        },
        ..Default::default()
    };
    let mut list = match ctx.with_conn(|c| orders::list(c, &filter)) {
        Ok(l) => l,
        Err(e) => return ctx.fail(&e),
    };
    if filter_key == "overdue" {
        let today = today_string();
        list.retain(|o| o.is_overdue(&today));
    }
    let sym = ctx.symbol();
    let total: i64 = list
        .iter()
        .filter(|o| matches!(o.status, OrderStatus::Confirmed | OrderStatus::Paid))
        .map(|o| o.total_cents)
        .sum();
    s.set_summary(
        format!(
            "{} order{} · {} in sales",
            list.len(),
            if list.len() == 1 { "" } else { "s" },
            format_money(total, &sym)
        )
        .into(),
    );
    s.set_rows(model(order_rows(&list, &sym)));
}

/// Shows orders for one customer (from the customers page).
pub fn show_for_customer(ctx: &Rc<Ctx>, name: &str) {
    let ui_ = ctx.ui();
    let s = ui_.global::<OrdersState>();
    s.set_search(name.into());
    s.set_status_filter("all".into());
    nav::navigate(ctx, "orders");
}

// ---------------------------------------------------------------------------
// Editor
// ---------------------------------------------------------------------------

/// Refreshes the customer list in the order form, keeping or selecting `select_id`.
pub fn load_customer_choices(ctx: &Ctx, select_id: Option<i64>) {
    let list = ctx.with_conn(|c| customers::list(c, "")).unwrap_or_default();
    let ui_ = ctx.ui();
    let form = ui_.global::<OrderForm>();
    let current = select_id.or_else(|| {
        let idx = form.get_customer_index();
        ctx.customer_ids.borrow().get(idx.max(0) as usize).copied().filter(|_| idx >= 0)
    });
    *ctx.customer_ids.borrow_mut() = list.iter().map(|c| c.id).collect();
    form.set_customers(strings(list.iter().map(|c| c.name.clone())));
    let index = current
        .and_then(|id| list.iter().position(|c| c.id == id))
        .map_or(-1, |i| i as i32);
    form.set_customer_index(index);
}

fn open_editor(ctx: &Rc<Ctx>, id: Option<i64>) {
    let ui = ctx.ui();
    let form = ui.global::<OrderForm>();

    let templates = ctx.with_conn(templates::list).unwrap_or_default();
    *ctx.template_ids.borrow_mut() = templates.iter().map(|t| t.id).collect();
    form.set_templates(strings(templates.iter().map(|t| {
        if t.is_default { format!("{} (default)", t.template.name) } else { t.template.name.clone() }
    })));

    let order = match id {
        Some(id) => match ctx.with_conn(|c| orders::get(c, id)) {
            Ok(o) => Some(o),
            Err(e) => return ctx.fail(&e),
        },
        None => None,
    };

    form.set_customer_index(-1);
    load_customer_choices(ctx, order.as_ref().map(|o| o.customer_id));

    let template_id = order.as_ref().and_then(|o| o.template_id);
    let t_index = template_id
        .and_then(|tid| templates.iter().position(|t| t.id == tid))
        .or_else(|| templates.iter().position(|t| t.is_default))
        .unwrap_or(0);
    form.set_template_index(t_index as i32);
    let template = templates.get(t_index).map(|t| t.template.clone()).unwrap_or_default();

    let mut draft = ctx.draft.borrow_mut();
    draft.id = id;
    draft.symbol = template.settings.currency_symbol.clone();
    form.set_tax_label(template.settings.tax_label.as_str().into());
    form.set_error(SharedString::new());

    match &order {
        Some(o) => {
            form.set_id(o.id as i32);
            form.set_title(format!("Edit {}", o.order_no).into());
            form.set_status_index(status_index(o.status));
            form.set_order_date(o.order_date.as_str().into());
            form.set_due_date(o.due_date.as_str().into());
            form.set_notes(o.notes.as_str().into());
            form.set_discount(if o.totals.discount_cents > 0 { format_amount(o.totals.discount_cents) } else { String::new() }.into());
            form.set_tax_rate(format_percent(o.tax_rate).into());
            draft.items = o
                .items
                .iter()
                .map(|i| ItemDraft {
                    description: i.description.clone(),
                    quantity: format_quantity(i.quantity),
                    price: format_amount(i.unit_price_cents).replace(',', ""),
                })
                .collect();
        }
        None => {
            let today = Local::now().date_naive();
            let due = today + Days::days(template.settings.payment_terms_days as i64);
            form.set_id(0);
            form.set_title("New order".into());
            form.set_status_index(status_index(OrderStatus::Draft));
            form.set_order_date(today.format("%Y-%m-%d").to_string().into());
            form.set_due_date(due.format("%Y-%m-%d").to_string().into());
            form.set_notes(SharedString::new());
            form.set_discount(SharedString::new());
            form.set_tax_rate(format_percent(template.settings.default_tax_rate).into());
            draft.items = vec![ItemDraft { quantity: "1".into(), ..Default::default() }];
        }
    }

    let rows: Vec<ItemRow> = draft
        .items
        .iter()
        .map(|d| ItemRow {
            description: d.description.as_str().into(),
            quantity: d.quantity.as_str().into(),
            price: d.price.as_str().into(),
            amount: SharedString::new(),
        })
        .collect();
    drop(draft);
    ctx.items_model.set_vec(rows);
    form.set_items(ctx.items_model.clone().into());
    form.set_generation(form.get_generation() + 1);
    recalc(ctx);
    ui.global::<crate::AppState>().set_page("order-edit".into());
}

fn item_edited(ctx: &Rc<Ctx>, index: usize, field: &str, value: &str) {
    {
        let mut draft = ctx.draft.borrow_mut();
        let Some(item) = draft.items.get_mut(index) else { return };
        match field {
            "description" => item.description = value.to_string(),
            "quantity" => item.quantity = value.to_string(),
            "price" => item.price = value.to_string(),
            _ => return,
        }
    }
    recalc(ctx);
}

/// Lenient parse for live totals while typing.
fn draft_items(ctx: &Ctx) -> Vec<OrderItem> {
    ctx.draft
        .borrow()
        .items
        .iter()
        .map(|d| OrderItem {
            description: d.description.clone(),
            quantity: parse_quantity(&d.quantity).unwrap_or(0.0),
            unit_price_cents: parse_money(&d.price).unwrap_or(0),
        })
        .collect()
}

fn recalc(ctx: &Rc<Ctx>) {
    let ui_ = ctx.ui();
    let form = ui_.global::<OrderForm>();
    let sym = ctx.draft.borrow().symbol.clone();
    let items = draft_items(ctx);
    for (i, item) in items.iter().enumerate() {
        if let Some(mut row) = ctx.items_model.row_data(i) {
            let amount: SharedString = format_money(item.line_total_cents(), &sym).into();
            if row.amount != amount {
                row.amount = amount;
                ctx.items_model.set_row_data(i, row);
            }
        }
    }
    let discount = parse_money(&form.get_discount()).unwrap_or(0);
    let tax_rate = parse_percent(&form.get_tax_rate()).unwrap_or(0.0);
    let t = compute_totals(&items, discount, tax_rate);
    form.set_subtotal(format_money(t.subtotal_cents, &sym).into());
    form.set_discount_total(
        if t.discount_cents > 0 { format!("-{}", format_money(t.discount_cents, &sym)) } else { format_money(0, &sym) }.into(),
    );
    form.set_tax_total(format_money(t.tax_cents, &sym).into());
    form.set_total(format_money(t.total_cents, &sym).into());
}

fn template_chosen(ctx: &Rc<Ctx>, index: i32) {
    let Some(id) = ctx.template_ids.borrow().get(index.max(0) as usize).copied() else { return };
    let Ok(t) = ctx.with_conn(|c| templates::get(c, id)) else { return };
    let ui_ = ctx.ui();
    let form = ui_.global::<OrderForm>();
    let s = &t.template.settings;
    ctx.draft.borrow_mut().symbol = s.currency_symbol.clone();
    form.set_tax_label(s.tax_label.as_str().into());
    if form.get_id() == 0 {
        form.set_tax_rate(format_percent(s.default_tax_rate).into());
        if let Ok(date) = orders::parse_date(&form.get_order_date(), "Order date") {
            let due = date + Days::days(s.payment_terms_days as i64);
            form.set_due_date(due.format("%Y-%m-%d").to_string().into());
        }
    }
    recalc(ctx);
}

fn build_input(ctx: &Ctx) -> app_core::Result<OrderInput> {
    let ui_ = ctx.ui();
    let form = ui_.global::<OrderForm>();
    let customer_id = ctx
        .customer_ids
        .borrow()
        .get(form.get_customer_index().max(0) as usize)
        .copied()
        .filter(|_| form.get_customer_index() >= 0)
        .ok_or_else(|| app_core::CoreError::Validation("Choose a customer for this order.".into()))?;
    let template_id =
        ctx.template_ids.borrow().get(form.get_template_index().max(0) as usize).copied();

    let mut items = Vec::new();
    for (i, d) in ctx.draft.borrow().items.iter().enumerate() {
        let blank = d.description.trim().is_empty() && d.price.trim().is_empty();
        if blank && ctx.draft.borrow().items.len() > 1 {
            continue; // ignore empty trailing rows
        }
        let n = i + 1;
        let wrap = |e: app_core::CoreError| {
            app_core::CoreError::Validation(format!("Item {n}: {e}"))
        };
        items.push(OrderItem {
            description: d.description.trim().to_string(),
            quantity: parse_quantity(&d.quantity).map_err(wrap)?,
            unit_price_cents: parse_money(&d.price).map_err(wrap)?,
        });
    }

    Ok(OrderInput {
        customer_id,
        template_id,
        status: status_from_index(form.get_status_index()),
        order_date: form.get_order_date().to_string(),
        due_date: form.get_due_date().to_string(),
        notes: form.get_notes().to_string(),
        discount_cents: parse_money(&form.get_discount())?,
        tax_rate: parse_percent(&form.get_tax_rate())?,
        items,
    })
}

fn save(ctx: &Rc<Ctx>) {
    let ui_ = ctx.ui();
    let form = ui_.global::<OrderForm>();
    let id = ctx.draft.borrow().id;
    let result = build_input(ctx).and_then(|input| {
        ctx.with_conn(|c| match id {
            Some(id) => orders::update(c, id, &input).map(|_| id),
            None => orders::create(c, &input),
        })
    });
    match result {
        Ok(saved_id) => {
            form.set_error(SharedString::new());
            ctx.success(if id.is_some() { "Order updated" } else { "Order created" });
            open_view(ctx, saved_id);
        }
        Err(e) => {
            form.set_error(e.to_string().into());
        }
    }
}

// ---------------------------------------------------------------------------
// Detail view
// ---------------------------------------------------------------------------

pub fn open_view(ctx: &Rc<Ctx>, id: i64) {
    let order = match ctx.with_conn(|c| orders::get(c, id)) {
        Ok(o) => o,
        Err(e) => return ctx.fail(&e),
    };
    let template = ctx
        .with_conn(|c| match order.template_id {
            Some(tid) => templates::get(c, tid),
            None => templates::get_default(c),
        })
        .map(|t| t.template)
        .unwrap_or_default();
    let customer = ctx.with_conn(|c| customers::get(c, order.customer_id)).ok();
    let sym = template.settings.currency_symbol.clone();
    let money = |c: i64| format_money(c, &sym);

    let ui = ctx.ui();
    let v = ui.global::<OrderView>();
    v.set_id(order.id as i32);
    v.set_order_no(order.order_no.as_str().into());
    v.set_status(order.status.as_str().into());
    v.set_status_label(order.status.label().into());
    v.set_overdue(order.status == OrderStatus::Confirmed && order.due_date < today_string());
    v.set_customer(order.customer_name.as_str().into());
    v.set_customer_contact(
        customer
            .map(|c| if c.email.is_empty() { c.phone } else { c.email })
            .unwrap_or_default()
            .into(),
    );
    v.set_date(order.order_date.as_str().into());
    v.set_due(order.due_date.as_str().into());
    v.set_template_name(template.name.as_str().into());
    v.set_items(model(
        order
            .items
            .iter()
            .map(|i| ItemRow {
                description: i.description.as_str().into(),
                quantity: format_quantity(i.quantity).into(),
                price: money(i.unit_price_cents).into(),
                amount: money(i.line_total_cents()).into(),
            })
            .collect(),
    ));
    v.set_subtotal(money(order.totals.subtotal_cents).into());
    v.set_has_discount(order.totals.discount_cents > 0);
    v.set_discount(format!("-{}", money(order.totals.discount_cents)).into());
    v.set_tax_label(format!("{} ({}%)", template.settings.tax_label, format_percent(order.tax_rate)).into());
    v.set_tax(money(order.totals.tax_cents).into());
    v.set_total(money(order.totals.total_cents).into());
    v.set_notes(order.notes.as_str().into());
    v.set_invoice_no(order.invoice_no.clone().unwrap_or_default().into());
    let versions = order
        .invoice_id
        .map(|iid| ctx.with_conn(|c| invoices::versions(c, iid)).unwrap_or_default())
        .unwrap_or_default();
    v.set_versions(model(version_rows(&versions, &sym)));
    ui.global::<crate::AppState>().set_page("order-view".into());
}

fn set_status(ctx: &Rc<Ctx>, id: i64, status: &str) {
    let Some(status) = OrderStatus::parse(status) else { return };
    let apply = move |c: &Rc<Ctx>| match c.with_conn(|conn| orders::set_status(conn, id, status)) {
        Ok(()) => {
            c.success(format!("Order marked as {}", status.label().to_lowercase()));
            open_view(c, id);
        }
        Err(e) => c.fail(&e),
    };
    if status == OrderStatus::Cancelled {
        ctx.confirm(
            "Cancel this order?",
            "Cancelled orders are excluded from sales and can't be invoiced. You can reopen it later.",
            "Cancel order",
            true,
            apply,
        );
    } else {
        apply(ctx);
    }
}

fn delete_order(ctx: &Rc<Ctx>, id: i64) {
    ctx.confirm(
        "Delete this order?",
        "This permanently removes the order and its items. This can't be undone.",
        "Delete",
        true,
        move |c| match c.with_conn(|conn| orders::delete(conn, id)) {
            Ok(()) => {
                c.success("Order deleted");
                nav::navigate(c, "orders");
            }
            Err(e) => c.fail(&e),
        },
    );
}

fn generate_invoice(ctx: &Rc<Ctx>) {
    let ui = ctx.ui();
    let v = ui.global::<OrderView>();
    let order_id = v.get_id() as i64;
    let note = v.get_version_note().to_string();
    match ctx.with_conn(|c| invoices::generate(c, order_id, &note)) {
        Ok(generated) => {
            v.set_generate_visible(false);
            open_view(ctx, order_id);
            let snap = &generated.snapshot;
            ctx.success(format!("{} version {} created", snap.invoice_no, snap.version));
            // Offer to download straight away.
            save_snapshot(ctx, snap);
        }
        Err(e) => {
            v.set_generate_visible(false);
            ctx.fail(&e);
        }
    }
}

fn save_snapshot(ctx: &Ctx, snap: &invoices::InvoiceSnapshot) {
    let bytes = match pdf::render_invoice(snap) {
        Ok(b) => b,
        Err(e) => return ctx.fail(&e),
    };
    let Some(path) = files::save_dialog("Save invoice PDF", &snap.file_name(), ("PDF document", &["pdf"]))
    else {
        return;
    };
    match std::fs::write(&path, bytes) {
        Ok(()) => ctx.success(format!("Saved {}", files::display_name(&path))),
        Err(e) => ctx.toast("error", format!("Couldn't save the file: {e}")),
    }
}

pub fn download_version(ctx: &Rc<Ctx>, version_id: i64) {
    match ctx.with_conn(|c| invoices::load_version(c, version_id)) {
        Ok(snap) => save_snapshot(ctx, &snap),
        Err(e) => ctx.fail(&e),
    }
}

fn open_version(ctx: &Rc<Ctx>, version_id: i64) {
    let result = ctx
        .with_conn(|c| invoices::load_version(c, version_id))
        .and_then(|snap| pdf::render_invoice(&snap).map(|b| (snap, b)));
    match result {
        Ok((snap, bytes)) => {
            if let Err(e) = files::open_temp(&snap.file_name(), &bytes) {
                ctx.toast("error", format!("Couldn't open the PDF: {e}"));
            }
        }
        Err(e) => ctx.fail(&e),
    }
}
