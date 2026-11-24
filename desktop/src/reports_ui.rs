use app_core::money::format_money;
use app_core::orders::OrderFilter;
use app_core::{customers, reports};
use chrono::{Datelike, Local, NaiveDate};
use slint::{ComponentHandle, SharedString};
use std::rc::Rc;

use crate::convert::{model, order_rows, strings};
use crate::ctx::{Ctx, status_from_index};
use crate::dashboard_ui::compact_money;
use crate::{Api, Bar, Kpi, NameTotal, ReportsState, files};

/// Rows shown on screen; the CSV export always contains everything.
const MAX_ROWS: usize = 300;

pub fn init(ctx: &Rc<Ctx>) {
    let ui = ctx.ui();
    let api = ui.global::<Api>();
    let c = ctx.clone();
    api.on_run_report(move || run(&c));
    let c = ctx.clone();
    api.on_report_preset(move |key| {
        apply_preset(&c, key.as_str());
        run(&c);
    });
    let c = ctx.clone();
    api.on_export_report(move || export(&c));
}

/// Called when the page is opened: refresh the customer list and default to this month.
pub fn open(ctx: &Rc<Ctx>) {
    let ui_ = ctx.ui();
    let s = ui_.global::<ReportsState>();
    let list = ctx
        .with_conn(|c| customers::list(c, ""))
        .unwrap_or_default();
    let selected = s.get_customer_index();
    let selected_id = (selected > 0)
        .then(|| {
            ctx.report_customer_ids
                .borrow()
                .get(selected as usize - 1)
                .copied()
        })
        .flatten();
    *ctx.report_customer_ids.borrow_mut() = list.iter().map(|c| c.id).collect();
    s.set_customers(strings(
        std::iter::once("All customers".to_string()).chain(list.iter().map(|c| c.name.clone())),
    ));
    s.set_customer_index(
        selected_id
            .and_then(|id| list.iter().position(|c| c.id == id))
            .map_or(0, |i| i as i32 + 1),
    );
    if s.get_from().is_empty() && s.get_to().is_empty() {
        apply_preset(ctx, "this-month");
    }
    run(ctx);
}

fn ymd(d: NaiveDate) -> SharedString {
    d.format("%Y-%m-%d").to_string().into()
}

fn apply_preset(ctx: &Ctx, key: &str) {
    let ui_ = ctx.ui();
    let s = ui_.global::<ReportsState>();
    let today = Local::now().date_naive();
    let first_of_month = today.with_day(1).unwrap();
    let (from, to) = match key {
        "this-month" => (Some(first_of_month), Some(today)),
        "last-month" => {
            let last_day = first_of_month.pred_opt().unwrap();
            (Some(last_day.with_day(1).unwrap()), Some(last_day))
        }
        "this-year" => (NaiveDate::from_ymd_opt(today.year(), 1, 1), Some(today)),
        _ => (None, None),
    };
    s.set_from(from.map(ymd).unwrap_or_default());
    s.set_to(to.map(ymd).unwrap_or_default());
}

fn current_filter(ctx: &Ctx) -> OrderFilter {
    let ui_ = ctx.ui();
    let s = ui_.global::<ReportsState>();
    let status_idx = s.get_status_index();
    let cust_idx = s.get_customer_index();
    OrderFilter {
        from: Some(s.get_from().to_string()),
        to: Some(s.get_to().to_string()),
        status: (status_idx > 0).then(|| status_from_index(status_idx - 1)),
        customer_id: (cust_idx > 0)
            .then(|| {
                ctx.report_customer_ids
                    .borrow()
                    .get(cust_idx as usize - 1)
                    .copied()
            })
            .flatten(),
        ..Default::default()
    }
}

fn kpi(label: &str, value: String, hint: String, tone: &str) -> Kpi {
    Kpi {
        label: label.into(),
        value: value.into(),
        hint: hint.into(),
        tone: tone.into(),
    }
}

fn run(ctx: &Rc<Ctx>) {
    let ui_ = ctx.ui();
    let s = ui_.global::<ReportsState>();
    let filter = current_filter(ctx);
    let report = match ctx.with_conn(|c| reports::run(c, &filter)) {
        Ok(r) => r,
        Err(e) => {
            s.set_error(e.to_string().into());
            return;
        }
    };
    s.set_error(SharedString::new());
    let sym = ctx.symbol();
    let money = |c: i64| format_money(c, &sym);
    let sum = &report.summary;

    let from = filter.from.as_deref().filter(|v| !v.trim().is_empty());
    let to = filter.to.as_deref().filter(|v| !v.trim().is_empty());
    s.set_range_label(
        match (from, to) {
            (Some(f), Some(t)) => format!("{f} to {t}"),
            (Some(f), None) => format!("From {f}"),
            (None, Some(t)) => format!("Up to {t}"),
            (None, None) => "All time".into(),
        }
        .into(),
    );

    s.set_kpis(model(vec![
        kpi(
            "Sales",
            money(sum.sales_cents),
            "Unpaid + paid orders".into(),
            "neutral",
        ),
        kpi(
            "Paid",
            money(sum.paid_cents),
            "Money received".into(),
            "good",
        ),
        kpi(
            "Outstanding",
            money(sum.outstanding_cents),
            "Awaiting payment".into(),
            if sum.outstanding_cents > 0 {
                "warn"
            } else {
                "neutral"
            },
        ),
    ]));
    s.set_kpis_2(model(vec![
        kpi(
            "Orders",
            sum.order_count.to_string(),
            format!(
                "{} draft · {} cancelled",
                sum.draft_count, sum.cancelled_count
            ),
            "neutral",
        ),
        kpi(
            "Average order",
            money(sum.average_order_cents),
            "Per unpaid or paid order".into(),
            "neutral",
        ),
        kpi(
            "Tax",
            money(sum.tax_cents),
            "Included in sales".into(),
            "neutral",
        ),
    ]));

    let max = report
        .by_month
        .iter()
        .map(|(_, v)| *v)
        .max()
        .unwrap_or(0)
        .max(1);
    // Keep the chart readable: show the most recent 12 months in range.
    let months = &report.by_month[report.by_month.len().saturating_sub(12)..];
    s.set_by_month(model(
        months
            .iter()
            .map(|(ym, v)| Bar {
                label: NaiveDate::parse_from_str(&format!("{ym}-01"), "%Y-%m-%d")
                    .map(|d| d.format("%b %y").to_string())
                    .unwrap_or_else(|_| ym.clone())
                    .into(),
                value: compact_money(*v, &sym).into(),
                ratio: *v as f32 / max as f32,
            })
            .collect(),
    ));

    let top = report
        .by_customer
        .first()
        .map_or(1, |c| c.sales_cents.max(1));
    s.set_by_customer(model(
        report
            .by_customer
            .iter()
            .take(8)
            .map(|c| NameTotal {
                name: c.customer_name.as_str().into(),
                detail: format!(
                    "{} order{}",
                    c.order_count,
                    if c.order_count == 1 { "" } else { "s" }
                )
                .into(),
                total: money(c.sales_cents).into(),
                ratio: c.sales_cents as f32 / top as f32,
            })
            .collect(),
    ));

    let shown = &report.orders[..report.orders.len().min(MAX_ROWS)];
    s.set_rows(model(order_rows(shown, &sym)));
}

fn export(ctx: &Rc<Ctx>) {
    let filter = current_filter(ctx);
    let report = match ctx.with_conn(|c| reports::run(c, &filter)) {
        Ok(r) => r,
        Err(e) => return ctx.fail(&e),
    };
    let name = format!("OpenBilling-report-{}.csv", Local::now().format("%Y-%m-%d"));
    let Some(path) = files::save_dialog("Export report", &name, ("CSV spreadsheet", &["csv"]))
    else {
        return;
    };
    // BOM so spreadsheet apps detect UTF-8.
    let csv = format!("\u{feff}{}", reports::to_csv(&report));
    match std::fs::write(&path, csv) {
        Ok(()) => ctx.success(format!(
            "Exported {} order{} to {}",
            report.orders.len(),
            if report.orders.len() == 1 { "" } else { "s" },
            files::display_name(&path)
        )),
        Err(e) => ctx.toast("error", format!("Couldn't save the file: {e}")),
    }
}
