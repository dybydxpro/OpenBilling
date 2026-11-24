//! Core data -> Slint structs.

use app_core::customers::Customer;
use app_core::db::today_string;
use app_core::invoices::{InvoiceSummary, InvoiceVersion};
use app_core::money::format_money;
use app_core::orders::OrderSummary;
use app_core::templates::{TemplateRecord, parse_hex_color};
use slint::{Color, ModelRc, SharedString, VecModel};

use crate::{CustomerRow, InvoiceRow, OrderRow, TemplateRow, VersionRow};

pub fn model<T: Clone + 'static>(items: Vec<T>) -> ModelRc<T> {
    ModelRc::new(VecModel::from(items))
}

pub fn strings<I: IntoIterator<Item = S>, S: Into<SharedString>>(items: I) -> ModelRc<SharedString> {
    model(items.into_iter().map(Into::into).collect())
}

pub fn order_row(o: &OrderSummary, symbol: &str, today: &str) -> OrderRow {
    OrderRow {
        id: o.id as i32,
        order_no: o.order_no.as_str().into(),
        customer: o.customer_name.as_str().into(),
        date: o.order_date.as_str().into(),
        due: o.due_date.as_str().into(),
        status: o.status.as_str().into(),
        status_label: o.status.label().into(),
        overdue: o.is_overdue(today),
        total: format_money(o.total_cents, symbol).into(),
        invoice_no: o.invoice_no.clone().unwrap_or_default().into(),
        versions: o.invoice_versions as i32,
    }
}

pub fn order_rows(list: &[OrderSummary], symbol: &str) -> Vec<OrderRow> {
    let today = today_string();
    list.iter().map(|o| order_row(o, symbol, &today)).collect()
}

pub fn initials(name: &str) -> String {
    let mut words = name.split_whitespace().filter_map(|w| w.chars().next());
    let first = words.next();
    let second = words.next();
    [first, second].into_iter().flatten().flat_map(char::to_uppercase).collect()
}

pub fn customer_row(c: &Customer, symbol: &str) -> CustomerRow {
    CustomerRow {
        id: c.id as i32,
        name: c.name.as_str().into(),
        initials: initials(&c.name).into(),
        email: c.email.as_str().into(),
        phone: c.phone.as_str().into(),
        address: c.address.as_str().into(),
        orders: c.order_count as i32,
        billed: format_money(c.billed_cents, symbol).into(),
    }
}

pub fn hex_color(hex: &str) -> Option<Color> {
    parse_hex_color(hex).map(|(r, g, b)| Color::from_rgb_u8(r, g, b))
}

pub fn template_row(t: &TemplateRecord) -> TemplateRow {
    let s = &t.template.settings;
    TemplateRow {
        id: t.id as i32,
        name: t.template.name.as_str().into(),
        business: t.template.business.name.as_str().into(),
        prefix: s.invoice_prefix.as_str().into(),
        currency: format!("{} {}", s.currency_symbol, s.currency_code).trim().into(),
        tax: format!("{} {}%", s.tax_label, app_core::money::format_percent(s.default_tax_rate)).into(),
        accent: hex_color(&s.accent_color).unwrap_or(Color::from_rgb_u8(79, 70, 229)),
        is_default: t.is_default,
        updated: t.updated_at.get(..10).unwrap_or(&t.updated_at).into(),
    }
}

pub fn invoice_row(i: &InvoiceSummary, symbol: &str) -> InvoiceRow {
    InvoiceRow {
        id: i.id as i32,
        invoice_no: i.invoice_no.as_str().into(),
        order_id: i.order_id as i32,
        order_no: i.order_no.as_str().into(),
        customer: i.customer_name.as_str().into(),
        status: i.status.as_str().into(),
        status_label: i.status.label().into(),
        version: i.latest_version as i32,
        total: format_money(i.latest_total_cents, symbol).into(),
        updated: short_time(&i.updated_at).into(),
    }
}

pub fn version_rows(versions: &[InvoiceVersion], symbol: &str) -> Vec<VersionRow> {
    versions
        .iter()
        .enumerate()
        .map(|(i, v)| VersionRow {
            id: v.id as i32,
            version: v.version as i32,
            created: short_time(&v.created_at).into(),
            total: format_money(v.total_cents, symbol).into(),
            note: v.note.as_str().into(),
            latest: i == 0,
        })
        .collect()
}

/// `2026-11-24 13:05:09` -> `2026-11-24 13:05`
pub fn short_time(s: &str) -> String {
    s.get(..16).unwrap_or(s).to_string()
}

#[cfg(test)]
mod tests {
    #[test]
    fn initials() {
        assert_eq!(super::initials("ocean view hotel"), "OV");
        assert_eq!(super::initials("Acme"), "A");
        assert_eq!(super::initials(""), "");
    }
}
