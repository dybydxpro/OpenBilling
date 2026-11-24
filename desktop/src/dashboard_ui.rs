use app_core::money::format_money;
use app_core::reports;
use chrono::{Local, Timelike};
use slint::ComponentHandle;
use std::rc::Rc;

use crate::convert::{model, order_rows};
use crate::ctx::Ctx;
use crate::{Api, Bar, DashboardState, Kpi, NameTotal};

pub fn init(ctx: &Rc<Ctx>) {
    let c = ctx.clone();
    ctx.ui()
        .global::<Api>()
        .on_refresh_dashboard(move || refresh(&c));
}

fn kpi(label: &str, value: String, hint: String, tone: &str) -> Kpi {
    Kpi {
        label: label.into(),
        value: value.into(),
        hint: hint.into(),
        tone: tone.into(),
    }
}

fn plural(n: i64, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

pub fn refresh(ctx: &Rc<Ctx>) {
    let d = match ctx.with_conn(reports::dashboard) {
        Ok(d) => d,
        Err(e) => return ctx.fail(&e),
    };
    let sym = ctx.symbol();
    let money = |c: i64| format_money(c, &sym);
    let ui = ctx.ui();
    let s = ui.global::<DashboardState>();

    let now = Local::now();
    s.set_greeting(
        match now.hour() {
            5..=11 => "Good morning",
            12..=17 => "Good afternoon",
            _ => "Good evening",
        }
        .into(),
    );
    s.set_today(now.format("%A, %-d %B %Y").to_string().into());

    let trend = if d.sales_last_month_cents > 0 {
        let pct = (d.sales_this_month_cents - d.sales_last_month_cents) as f64 * 100.0
            / d.sales_last_month_cents as f64;
        format!(
            "{}{:.0}% vs last month",
            if pct >= 0.0 { "▲ " } else { "▼ " },
            pct.abs()
        )
    } else {
        format!(
            "{} this month",
            plural(d.orders_this_month, "order", "orders")
        )
    };
    let trend_tone = if d.sales_this_month_cents >= d.sales_last_month_cents {
        "good"
    } else {
        "warn"
    };

    s.set_kpis(model(vec![
        kpi(
            "Sales this month",
            money(d.sales_this_month_cents),
            trend,
            trend_tone,
        ),
        kpi(
            "Outstanding",
            money(d.outstanding_cents),
            format!("{} unpaid", plural(d.outstanding_count, "order", "orders")),
            if d.outstanding_count > 0 {
                "warn"
            } else {
                "neutral"
            },
        ),
        kpi(
            "Overdue",
            money(d.overdue_cents),
            if d.overdue_count > 0 {
                format!(
                    "{} past due date",
                    plural(d.overdue_count, "order", "orders")
                )
            } else {
                "Nothing overdue".into()
            },
            if d.overdue_count > 0 { "bad" } else { "good" },
        ),
        kpi(
            "Collected this month",
            money(d.collected_this_month_cents),
            format!(
                "{} total",
                plural(d.customer_count, "customer", "customers")
            ),
            "good",
        ),
    ]));

    let max = d
        .monthly_sales
        .iter()
        .map(|(_, v)| *v)
        .max()
        .unwrap_or(0)
        .max(1);
    s.set_monthly(model(
        d.monthly_sales
            .iter()
            .map(|(label, v)| Bar {
                label: label.as_str().into(),
                value: if *v > 0 {
                    compact_money(*v, &sym)
                } else {
                    String::new()
                }
                .into(),
                ratio: *v as f32 / max as f32,
            })
            .collect(),
    ));
    s.set_monthly_total(money(d.monthly_sales.iter().map(|(_, v)| v).sum()).into());

    let max_count = d
        .status_counts
        .iter()
        .map(|(_, n)| *n)
        .max()
        .unwrap_or(0)
        .max(1);
    s.set_statuses(model(
        d.status_counts
            .iter()
            .map(|(st, n)| NameTotal {
                name: st.label().into(),
                detail: String::new().into(),
                total: n.to_string().into(),
                ratio: *n as f32 / max_count as f32,
            })
            .collect(),
    ));

    s.set_recent(model(order_rows(&d.recent_orders, &sym)));
    let total_orders: i64 = d.status_counts.iter().map(|(_, n)| n).sum();
    s.set_empty(total_orders == 0);
}

/// Short labels for chart bars: `$1.2k`, `$3.4M`.
pub fn compact_money(cents: i64, symbol: &str) -> String {
    let v = cents as f64 / 100.0;
    let (n, suffix) = if v.abs() >= 1_000_000.0 {
        (v / 1_000_000.0, "M")
    } else if v.abs() >= 1_000.0 {
        (v / 1_000.0, "k")
    } else {
        (v, "")
    };
    let num = if suffix.is_empty() {
        format!("{n:.0}")
    } else {
        format!("{n:.1}")
    };
    let num = num.trim_end_matches(".0").to_string();
    if symbol.chars().last().is_some_and(char::is_alphabetic) {
        format!("{symbol} {num}{suffix}")
    } else {
        format!("{symbol}{num}{suffix}")
    }
}
