use chrono::NaiveDate;
use rusqlite::{Connection, OptionalExtension, params, params_from_iter, types::Value};
use serde::{Deserialize, Serialize};

use crate::db::{get_setting, now_string, set_setting};
use crate::error::{CoreError, Result, invalid};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OrderStatus {
    Draft,
    Confirmed,
    Paid,
    Cancelled,
}

impl OrderStatus {
    pub const ALL: [OrderStatus; 4] = [Self::Draft, Self::Confirmed, Self::Paid, Self::Cancelled];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Confirmed => "confirmed",
            Self::Paid => "paid",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Draft => "Draft",
            Self::Confirmed => "Unpaid",
            Self::Paid => "Paid",
            Self::Cancelled => "Cancelled",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|v| v.as_str().eq_ignore_ascii_case(s) || v.label().eq_ignore_ascii_case(s))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OrderItem {
    pub description: String,
    pub quantity: f64,
    pub unit_price_cents: i64,
}

impl OrderItem {
    pub fn line_total_cents(&self) -> i64 {
        (self.quantity * self.unit_price_cents as f64).round() as i64
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Totals {
    pub subtotal_cents: i64,
    pub discount_cents: i64,
    pub tax_cents: i64,
    pub total_cents: i64,
}

pub fn compute_totals(items: &[OrderItem], discount_cents: i64, tax_rate: f64) -> Totals {
    let subtotal: i64 = items.iter().map(OrderItem::line_total_cents).sum();
    let discount = discount_cents.clamp(0, subtotal.max(0));
    let taxable = subtotal - discount;
    let tax = (taxable as f64 * tax_rate / 100.0).round() as i64;
    Totals {
        subtotal_cents: subtotal,
        discount_cents: discount,
        tax_cents: tax,
        total_cents: taxable + tax,
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct OrderInput {
    pub customer_id: i64,
    pub template_id: Option<i64>,
    pub status: OrderStatus,
    /// `YYYY-MM-DD`
    pub order_date: String,
    /// `YYYY-MM-DD`
    pub due_date: String,
    pub notes: String,
    pub discount_cents: i64,
    pub tax_rate: f64,
    pub items: Vec<OrderItem>,
}

impl OrderInput {
    pub fn validate(&self) -> Result<()> {
        let order_date = parse_date(&self.order_date, "Order date")?;
        let due_date = parse_date(&self.due_date, "Due date")?;
        if due_date < order_date {
            return invalid("Due date can't be before the order date.");
        }
        if self.items.is_empty() {
            return invalid("Add at least one item.");
        }
        for (i, item) in self.items.iter().enumerate() {
            let n = i + 1;
            if item.description.trim().is_empty() {
                return invalid(format!("Item {n}: description is required."));
            }
            if !(item.quantity.is_finite() && item.quantity > 0.0) {
                return invalid(format!("Item {n}: quantity must be greater than zero."));
            }
            if item.unit_price_cents < 0 {
                return invalid(format!("Item {n}: price can't be negative."));
            }
        }
        if !(0.0..=100.0).contains(&self.tax_rate) {
            return invalid("Tax rate must be between 0 and 100.");
        }
        let subtotal: i64 = self.items.iter().map(OrderItem::line_total_cents).sum();
        if self.discount_cents < 0 || self.discount_cents > subtotal {
            return invalid("Discount must be between zero and the subtotal.");
        }
        Ok(())
    }

    pub fn totals(&self) -> Totals {
        compute_totals(&self.items, self.discount_cents, self.tax_rate)
    }
}

pub fn parse_date(s: &str, field: &str) -> Result<NaiveDate> {
    NaiveDate::parse_from_str(s.trim(), "%Y-%m-%d")
        .or_else(|_| invalid(format!("{field} must be a date like 2026-01-31.")))
}

#[derive(Debug, Clone, PartialEq)]
pub struct Order {
    pub id: i64,
    pub order_no: String,
    pub customer_id: i64,
    pub customer_name: String,
    pub template_id: Option<i64>,
    pub status: OrderStatus,
    pub order_date: String,
    pub due_date: String,
    pub notes: String,
    pub tax_rate: f64,
    pub items: Vec<OrderItem>,
    pub totals: Totals,
    pub created_at: String,
    pub updated_at: String,
    pub invoice_id: Option<i64>,
    pub invoice_no: Option<String>,
}

impl Order {
    pub fn to_input(&self) -> OrderInput {
        OrderInput {
            customer_id: self.customer_id,
            template_id: self.template_id,
            status: self.status,
            order_date: self.order_date.clone(),
            due_date: self.due_date.clone(),
            notes: self.notes.clone(),
            discount_cents: self.totals.discount_cents,
            tax_rate: self.tax_rate,
            items: self.items.clone(),
        }
    }
}

/// One row in an order listing.
#[derive(Debug, Clone, PartialEq)]
pub struct OrderSummary {
    pub id: i64,
    pub order_no: String,
    pub customer_id: i64,
    pub customer_name: String,
    pub status: OrderStatus,
    pub order_date: String,
    pub due_date: String,
    pub subtotal_cents: i64,
    pub tax_cents: i64,
    pub total_cents: i64,
    pub invoice_no: Option<String>,
    pub invoice_versions: i64,
}

impl OrderSummary {
    /// Unpaid and past its due date.
    pub fn is_overdue(&self, today: &str) -> bool {
        self.status == OrderStatus::Confirmed && self.due_date.as_str() < today
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct OrderFilter {
    /// Matches order number, invoice number or customer name.
    pub search: String,
    pub status: Option<OrderStatus>,
    pub customer_id: Option<i64>,
    /// Inclusive, `YYYY-MM-DD`.
    pub from: Option<String>,
    /// Inclusive, `YYYY-MM-DD`.
    pub to: Option<String>,
    pub limit: Option<u32>,
}

pub fn list(conn: &Connection, filter: &OrderFilter) -> Result<Vec<OrderSummary>> {
    let mut sql = String::from(
        "SELECT o.id, o.order_no, o.customer_id, c.name, o.status, o.order_date, o.due_date,
                o.subtotal_cents, o.tax_cents, o.total_cents, i.invoice_no,
                (SELECT COUNT(*) FROM invoice_versions v WHERE v.invoice_id = i.id)
         FROM orders o
         JOIN customers c ON c.id = o.customer_id
         LEFT JOIN invoices i ON i.order_id = o.id
         WHERE 1 = 1",
    );
    let mut args: Vec<Value> = Vec::new();
    let search = filter.search.trim();
    if !search.is_empty() {
        args.push(Value::Text(format!("%{search}%")));
        let n = args.len();
        sql.push_str(&format!(
            " AND (o.order_no LIKE ?{n} OR c.name LIKE ?{n} OR i.invoice_no LIKE ?{n})"
        ));
    }
    if let Some(status) = filter.status {
        args.push(Value::Text(status.as_str().into()));
        sql.push_str(&format!(" AND o.status = ?{}", args.len()));
    }
    if let Some(cid) = filter.customer_id {
        args.push(Value::Integer(cid));
        sql.push_str(&format!(" AND o.customer_id = ?{}", args.len()));
    }
    if let Some(from) = filter.from.as_deref().filter(|s| !s.trim().is_empty()) {
        parse_date(from, "From date")?;
        args.push(Value::Text(from.trim().into()));
        sql.push_str(&format!(" AND o.order_date >= ?{}", args.len()));
    }
    if let Some(to) = filter.to.as_deref().filter(|s| !s.trim().is_empty()) {
        parse_date(to, "To date")?;
        args.push(Value::Text(to.trim().into()));
        sql.push_str(&format!(" AND o.order_date <= ?{}", args.len()));
    }
    sql.push_str(" ORDER BY o.order_date DESC, o.id DESC");
    if let Some(limit) = filter.limit {
        sql.push_str(&format!(" LIMIT {limit}"));
    }

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params_from_iter(args), |r| {
        let status: String = r.get(4)?;
        Ok(OrderSummary {
            id: r.get(0)?,
            order_no: r.get(1)?,
            customer_id: r.get(2)?,
            customer_name: r.get(3)?,
            status: OrderStatus::parse(&status).unwrap_or(OrderStatus::Draft),
            order_date: r.get(5)?,
            due_date: r.get(6)?,
            subtotal_cents: r.get(7)?,
            tax_cents: r.get(8)?,
            total_cents: r.get(9)?,
            invoice_no: r.get(10)?,
            invoice_versions: r.get(11)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

pub fn get(conn: &Connection, id: i64) -> Result<Order> {
    let mut order = conn
        .query_row(
            "SELECT o.order_no, o.customer_id, c.name, o.template_id, o.status, o.order_date,
                    o.due_date, o.notes, o.discount_cents, o.tax_rate, o.created_at, o.updated_at,
                    i.id, i.invoice_no
             FROM orders o
             JOIN customers c ON c.id = o.customer_id
             LEFT JOIN invoices i ON i.order_id = o.id
             WHERE o.id = ?1",
            [id],
            |r| {
                let status: String = r.get(4)?;
                let discount: i64 = r.get(8)?;
                Ok(Order {
                    id,
                    order_no: r.get(0)?,
                    customer_id: r.get(1)?,
                    customer_name: r.get(2)?,
                    template_id: r.get(3)?,
                    status: OrderStatus::parse(&status).unwrap_or(OrderStatus::Draft),
                    order_date: r.get(5)?,
                    due_date: r.get(6)?,
                    notes: r.get(7)?,
                    tax_rate: r.get(9)?,
                    items: Vec::new(),
                    totals: Totals {
                        discount_cents: discount,
                        ..Default::default()
                    },
                    created_at: r.get(10)?,
                    updated_at: r.get(11)?,
                    invoice_id: r.get(12)?,
                    invoice_no: r.get(13)?,
                })
            },
        )
        .optional()?
        .ok_or(CoreError::NotFound("Order"))?;

    let mut stmt = conn.prepare(
        "SELECT description, quantity, unit_price_cents FROM order_items WHERE order_id = ?1 ORDER BY position",
    )?;
    order.items = stmt
        .query_map([id], |r| {
            Ok(OrderItem {
                description: r.get(0)?,
                quantity: r.get(1)?,
                unit_price_cents: r.get(2)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    order.totals = compute_totals(&order.items, order.totals.discount_cents, order.tax_rate);
    Ok(order)
}

pub fn create(conn: &Connection, input: &OrderInput) -> Result<i64> {
    input.validate()?;
    ensure_customer(conn, input.customer_id)?;
    let tx = conn.unchecked_transaction()?;
    let order_no = next_number(&tx, "seq.order", "ORD-")?;
    let totals = input.totals();
    let now = now_string();
    tx.execute(
        "INSERT INTO orders (order_no, customer_id, template_id, status, order_date, due_date, notes,
                             discount_cents, tax_rate, subtotal_cents, tax_cents, total_cents,
                             created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?13)",
        params![
            order_no,
            input.customer_id,
            input.template_id,
            input.status.as_str(),
            input.order_date.trim(),
            input.due_date.trim(),
            input.notes.trim(),
            totals.discount_cents,
            input.tax_rate,
            totals.subtotal_cents,
            totals.tax_cents,
            totals.total_cents,
            now
        ],
    )?;
    let id = tx.last_insert_rowid();
    write_items(&tx, id, &input.items)?;
    tx.commit()?;
    Ok(id)
}

pub fn update(conn: &Connection, id: i64, input: &OrderInput) -> Result<()> {
    input.validate()?;
    ensure_customer(conn, input.customer_id)?;
    let totals = input.totals();
    let tx = conn.unchecked_transaction()?;
    let n = tx.execute(
        "UPDATE orders SET customer_id = ?1, template_id = ?2, status = ?3, order_date = ?4,
                due_date = ?5, notes = ?6, discount_cents = ?7, tax_rate = ?8,
                subtotal_cents = ?9, tax_cents = ?10, total_cents = ?11, updated_at = ?12
         WHERE id = ?13",
        params![
            input.customer_id,
            input.template_id,
            input.status.as_str(),
            input.order_date.trim(),
            input.due_date.trim(),
            input.notes.trim(),
            totals.discount_cents,
            input.tax_rate,
            totals.subtotal_cents,
            totals.tax_cents,
            totals.total_cents,
            now_string(),
            id
        ],
    )?;
    if n == 0 {
        return Err(CoreError::NotFound("Order"));
    }
    tx.execute("DELETE FROM order_items WHERE order_id = ?1", [id])?;
    write_items(&tx, id, &input.items)?;
    tx.commit()?;
    Ok(())
}

pub fn set_status(conn: &Connection, id: i64, status: OrderStatus) -> Result<()> {
    let n = conn.execute(
        "UPDATE orders SET status = ?1, updated_at = ?2 WHERE id = ?3",
        params![status.as_str(), now_string(), id],
    )?;
    if n == 0 {
        return Err(CoreError::NotFound("Order"));
    }
    Ok(())
}

/// Orders that already have an invoice are part of the paper trail and can
/// only be cancelled, not deleted.
pub fn delete(conn: &Connection, id: i64) -> Result<()> {
    let invoiced: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM invoices WHERE order_id = ?1)",
        [id],
        |r| r.get(0),
    )?;
    if invoiced {
        return invalid("This order has an invoice. Mark it as cancelled instead of deleting it.");
    }
    conn.execute("DELETE FROM orders WHERE id = ?1", [id])?;
    Ok(())
}

fn write_items(conn: &Connection, order_id: i64, items: &[OrderItem]) -> Result<()> {
    let mut stmt = conn.prepare(
        "INSERT INTO order_items (order_id, position, description, quantity, unit_price_cents)
         VALUES (?1, ?2, ?3, ?4, ?5)",
    )?;
    for (pos, item) in items.iter().enumerate() {
        stmt.execute(params![
            order_id,
            pos as i64,
            item.description.trim(),
            item.quantity,
            item.unit_price_cents
        ])?;
    }
    Ok(())
}

fn ensure_customer(conn: &Connection, id: i64) -> Result<()> {
    let exists: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM customers WHERE id = ?1)",
        [id],
        |r| r.get(0),
    )?;
    if exists {
        Ok(())
    } else {
        invalid("Choose a customer for this order.")
    }
}

/// Allocates the next number of a sequence, e.g. `ORD-0007`.
pub(crate) fn next_number(conn: &Connection, key: &str, prefix: &str) -> Result<String> {
    let next = get_setting(conn, key)?
        .and_then(|v| v.parse::<i64>().ok())
        .unwrap_or(0)
        + 1;
    set_setting(conn, key, &next.to_string())?;
    Ok(format!("{prefix}{next:04}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn totals_apply_discount_before_tax() {
        let items = vec![
            OrderItem {
                description: "A".into(),
                quantity: 2.0,
                unit_price_cents: 1000,
            },
            OrderItem {
                description: "B".into(),
                quantity: 0.5,
                unit_price_cents: 333,
            },
        ];
        let t = compute_totals(&items, 500, 10.0);
        assert_eq!(t.subtotal_cents, 2167);
        assert_eq!(t.discount_cents, 500);
        assert_eq!(t.tax_cents, 167);
        assert_eq!(t.total_cents, 1834);
    }
}
