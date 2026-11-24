//! Dashboard figures and filterable sales reports.
//!
//! Definitions used throughout:
//! * **Sales** – total of orders that are confirmed (unpaid) or paid.
//! * **Outstanding** – total of confirmed orders that are not yet paid.
//! * Drafts and cancelled orders never count towards money figures.

use chrono::{Datelike, Local, NaiveDate};
use rusqlite::Connection;
use std::collections::BTreeMap;

use crate::error::Result;
use crate::orders::{self, OrderFilter, OrderStatus, OrderSummary};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Dashboard {
    pub sales_this_month_cents: i64,
    pub sales_last_month_cents: i64,
    pub collected_this_month_cents: i64,
    pub outstanding_cents: i64,
    pub outstanding_count: i64,
    pub overdue_cents: i64,
    pub overdue_count: i64,
    pub orders_this_month: i64,
    pub customer_count: i64,
    /// Oldest first; label like "Jun".
    pub monthly_sales: Vec<(String, i64)>,
    pub status_counts: Vec<(OrderStatus, i64)>,
    pub recent_orders: Vec<OrderSummary>,
}

fn month_start(d: NaiveDate) -> NaiveDate {
    d.with_day(1).expect("day 1 exists")
}

fn add_months(d: NaiveDate, months: i32) -> NaiveDate {
    let total = d.year() * 12 + d.month0() as i32 + months;
    NaiveDate::from_ymd_opt(total.div_euclid(12), total.rem_euclid(12) as u32 + 1, 1)
        .expect("valid month")
}

pub fn dashboard(conn: &Connection) -> Result<Dashboard> {
    dashboard_at(conn, Local::now().date_naive())
}

pub fn dashboard_at(conn: &Connection, today: NaiveDate) -> Result<Dashboard> {
    let this_month = month_start(today);
    let next_month = add_months(this_month, 1);
    let last_month = add_months(this_month, -1);
    let fmt = |d: NaiveDate| d.format("%Y-%m-%d").to_string();
    let today_s = fmt(today);

    let sales_between = |from: NaiveDate, to: NaiveDate, statuses: &str| -> Result<(i64, i64)> {
        Ok(conn.query_row(
            &format!(
                "SELECT COALESCE(SUM(total_cents),0), COUNT(*) FROM orders
                 WHERE order_date >= ?1 AND order_date < ?2 AND status IN ({statuses})"
            ),
            [fmt(from), fmt(to)],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?)
    };

    let mut d = Dashboard {
        sales_this_month_cents: sales_between(this_month, next_month, "'confirmed','paid'")?.0,
        sales_last_month_cents: sales_between(last_month, this_month, "'confirmed','paid'")?.0,
        collected_this_month_cents: sales_between(this_month, next_month, "'paid'")?.0,
        orders_this_month: sales_between(this_month, next_month, "'draft','confirmed','paid'")?.1,
        ..Default::default()
    };

    (d.outstanding_cents, d.outstanding_count) = conn.query_row(
        "SELECT COALESCE(SUM(total_cents),0), COUNT(*) FROM orders WHERE status = 'confirmed'",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    (d.overdue_cents, d.overdue_count) = conn.query_row(
        "SELECT COALESCE(SUM(total_cents),0), COUNT(*) FROM orders
         WHERE status = 'confirmed' AND due_date < ?1",
        [&today_s],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    d.customer_count = conn.query_row("SELECT COUNT(*) FROM customers", [], |r| r.get(0))?;

    for i in (0..6).rev() {
        let start = add_months(this_month, -i);
        let (sum, _) = sales_between(start, add_months(start, 1), "'confirmed','paid'")?;
        d.monthly_sales.push((start.format("%b").to_string(), sum));
    }

    for status in OrderStatus::ALL {
        let n: i64 = conn.query_row(
            "SELECT COUNT(*) FROM orders WHERE status = ?1",
            [status.as_str()],
            |r| r.get(0),
        )?;
        d.status_counts.push((status, n));
    }

    d.recent_orders = orders::list(
        conn,
        &OrderFilter {
            limit: Some(6),
            ..Default::default()
        },
    )?;
    Ok(d)
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ReportSummary {
    pub order_count: i64,
    pub sales_cents: i64,
    pub paid_cents: i64,
    pub outstanding_cents: i64,
    pub tax_cents: i64,
    pub average_order_cents: i64,
    pub draft_count: i64,
    pub cancelled_count: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CustomerTotal {
    pub customer_name: String,
    pub order_count: i64,
    pub sales_cents: i64,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Report {
    pub summary: ReportSummary,
    pub by_customer: Vec<CustomerTotal>,
    /// `("2026-03", sales)`, oldest first.
    pub by_month: Vec<(String, i64)>,
    pub orders: Vec<OrderSummary>,
}

fn counts_as_sale(s: OrderStatus) -> bool {
    matches!(s, OrderStatus::Confirmed | OrderStatus::Paid)
}

/// Runs a report. The filter is the same one used by the order listing, so a
/// report always matches what the user sees in the list.
pub fn run(conn: &Connection, filter: &OrderFilter) -> Result<Report> {
    let orders = orders::list(
        conn,
        &OrderFilter {
            limit: None,
            ..filter.clone()
        },
    )?;
    let mut s = ReportSummary {
        order_count: orders.len() as i64,
        ..Default::default()
    };
    let mut customers: BTreeMap<String, (i64, i64)> = BTreeMap::new();
    let mut months: BTreeMap<String, i64> = BTreeMap::new();

    for o in &orders {
        match o.status {
            OrderStatus::Draft => s.draft_count += 1,
            OrderStatus::Cancelled => s.cancelled_count += 1,
            OrderStatus::Paid => s.paid_cents += o.total_cents,
            OrderStatus::Confirmed => s.outstanding_cents += o.total_cents,
        }
        if counts_as_sale(o.status) {
            s.sales_cents += o.total_cents;
            s.tax_cents += o.tax_cents;
            let c = customers.entry(o.customer_name.clone()).or_default();
            c.0 += 1;
            c.1 += o.total_cents;
            *months
                .entry(o.order_date.get(..7).unwrap_or("").to_string())
                .or_default() += o.total_cents;
        }
    }
    let sale_count = orders.iter().filter(|o| counts_as_sale(o.status)).count() as i64;
    if sale_count > 0 {
        s.average_order_cents = s.sales_cents / sale_count;
    }

    let mut by_customer: Vec<CustomerTotal> = customers
        .into_iter()
        .map(
            |(customer_name, (order_count, sales_cents))| CustomerTotal {
                customer_name,
                order_count,
                sales_cents,
            },
        )
        .collect();
    by_customer.sort_by_key(|c| std::cmp::Reverse(c.sales_cents));

    Ok(Report {
        summary: s,
        by_customer,
        by_month: months.into_iter().collect(),
        orders,
    })
}

fn csv_field(s: &str) -> String {
    if s.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

/// Order lines as CSV (amounts as plain decimals for spreadsheets).
pub fn to_csv(report: &Report) -> String {
    let amount = |c: i64| format!("{}.{:02}", c / 100, (c % 100).abs());
    let mut out = String::from(
        "Order No,Invoice No,Order Date,Due Date,Customer,Status,Subtotal,Tax,Total\n",
    );
    for o in &report.orders {
        let row = [
            csv_field(&o.order_no),
            csv_field(o.invoice_no.as_deref().unwrap_or("")),
            o.order_date.clone(),
            o.due_date.clone(),
            csv_field(&o.customer_name),
            o.status.label().to_string(),
            amount(o.subtotal_cents),
            amount(o.tax_cents),
            amount(o.total_cents),
        ];
        out.push_str(&row.join(","));
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn month_math() {
        let d = NaiveDate::from_ymd_opt(2026, 1, 15).unwrap();
        assert_eq!(
            add_months(d, -1),
            NaiveDate::from_ymd_opt(2025, 12, 1).unwrap()
        );
        assert_eq!(
            add_months(d, 12),
            NaiveDate::from_ymd_opt(2027, 1, 1).unwrap()
        );
    }

    #[test]
    fn csv_escapes() {
        assert_eq!(csv_field("A, \"B\""), "\"A, \"\"B\"\"\"");
    }
}
