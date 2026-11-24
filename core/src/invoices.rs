//! Invoices and their versions.
//!
//! An order has at most one invoice number. Every time the user generates the
//! invoice, an immutable *version* is stored containing a full snapshot of the
//! order, customer and template at that moment. Any version can be re-rendered
//! to an identical PDF later, even after the order or template has changed.

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::customers;
use crate::db::{now_string, today_string};
use crate::error::{CoreError, Result, invalid};
use crate::orders::{self, OrderItem, OrderStatus, Totals, next_number};
use crate::templates::{self, InvoiceTemplate};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CustomerSnapshot {
    pub name: String,
    pub email: String,
    pub phone: String,
    pub address: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InvoiceSnapshot {
    pub invoice_no: String,
    pub version: i64,
    /// `YYYY-MM-DD`
    pub issue_date: String,
    pub due_date: String,
    pub order_no: String,
    pub order_date: String,
    pub status: OrderStatus,
    pub customer: CustomerSnapshot,
    pub items: Vec<OrderItem>,
    pub tax_rate: f64,
    pub totals: Totals,
    pub notes: String,
    pub template: InvoiceTemplate,
}

impl InvoiceSnapshot {
    /// True when two snapshots would print the same content (ignores the
    /// version number and issue date).
    fn same_content(&self, other: &Self) -> bool {
        let strip = |s: &Self| Self { version: 0, issue_date: String::new(), ..s.clone() };
        strip(self) == strip(other)
    }

    pub fn file_name(&self) -> String {
        let safe: String = self
            .invoice_no
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '_' })
            .collect();
        format!("{safe}-v{}.pdf", self.version)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct InvoiceSummary {
    pub id: i64,
    pub invoice_no: String,
    pub order_id: i64,
    pub order_no: String,
    pub customer_name: String,
    pub status: OrderStatus,
    pub latest_version: i64,
    pub latest_total_cents: i64,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct InvoiceVersion {
    pub id: i64,
    pub version: i64,
    pub total_cents: i64,
    pub note: String,
    pub created_at: String,
}

pub struct Generated {
    pub invoice_id: i64,
    pub version_id: i64,
    pub snapshot: InvoiceSnapshot,
}

/// Creates the invoice for an order (first call) or a new version of it.
///
/// A draft order is confirmed in the process. Fails if nothing changed since
/// the latest version, so the history only contains meaningful revisions.
pub fn generate(conn: &Connection, order_id: i64, note: &str) -> Result<Generated> {
    let order = orders::get(conn, order_id)?;
    if order.status == OrderStatus::Cancelled {
        return invalid("Cancelled orders can't be invoiced.");
    }
    let customer = customers::get(conn, order.customer_id)?;
    let template = match order.template_id {
        Some(id) => templates::get(conn, id)?,
        None => templates::get_default(conn)?,
    }
    .template;

    let tx = conn.unchecked_transaction()?;
    let status = if order.status == OrderStatus::Draft {
        orders::set_status(&tx, order_id, OrderStatus::Confirmed)?;
        OrderStatus::Confirmed
    } else {
        order.status
    };

    let existing: Option<(i64, String)> = tx
        .query_row("SELECT id, invoice_no FROM invoices WHERE order_id = ?1", [order_id], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .optional()?;
    let (invoice_id, invoice_no) = match existing {
        Some(found) => found,
        None => {
            let no = next_number(&tx, "seq.invoice", &template.settings.invoice_prefix)?;
            tx.execute(
                "INSERT INTO invoices (order_id, invoice_no, created_at) VALUES (?1, ?2, ?3)",
                params![order_id, no, now_string()],
            )?;
            (tx.last_insert_rowid(), no)
        }
    };

    let latest: Option<(i64, String)> = tx
        .query_row(
            "SELECT version, snapshot_json FROM invoice_versions WHERE invoice_id = ?1
             ORDER BY version DESC LIMIT 1",
            [invoice_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let version = latest.as_ref().map_or(1, |(v, _)| v + 1);

    let snapshot = InvoiceSnapshot {
        invoice_no,
        version,
        issue_date: today_string(),
        due_date: order.due_date.clone(),
        order_no: order.order_no.clone(),
        order_date: order.order_date.clone(),
        status,
        customer: CustomerSnapshot {
            name: customer.name,
            email: customer.email,
            phone: customer.phone,
            address: customer.address,
        },
        items: order.items.clone(),
        tax_rate: order.tax_rate,
        totals: order.totals,
        notes: order.notes.clone(),
        template,
    };

    if let Some((v, json)) = &latest
        && let Ok(prev) = serde_json::from_str::<InvoiceSnapshot>(json)
            && prev.same_content(&snapshot) {
                return invalid(format!(
                    "Nothing has changed since version {v}. Download that version instead."
                ));
            }

    tx.execute(
        "INSERT INTO invoice_versions (invoice_id, version, snapshot_json, total_cents, note, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            invoice_id,
            version,
            serde_json::to_string(&snapshot)?,
            snapshot.totals.total_cents,
            note.trim(),
            now_string()
        ],
    )?;
    let version_id = tx.last_insert_rowid();
    tx.commit()?;
    Ok(Generated { invoice_id, version_id, snapshot })
}

pub fn list(conn: &Connection, search: &str) -> Result<Vec<InvoiceSummary>> {
    let pattern = format!("%{}%", search.trim());
    let mut stmt = conn.prepare(
        "SELECT i.id, i.invoice_no, o.id, o.order_no, c.name, o.status,
                v.version, v.total_cents, v.created_at
         FROM invoices i
         JOIN orders o ON o.id = i.order_id
         JOIN customers c ON c.id = o.customer_id
         JOIN invoice_versions v ON v.invoice_id = i.id
              AND v.version = (SELECT MAX(version) FROM invoice_versions WHERE invoice_id = i.id)
         WHERE i.invoice_no LIKE ?1 OR o.order_no LIKE ?1 OR c.name LIKE ?1
         ORDER BY v.created_at DESC, i.id DESC",
    )?;
    let rows = stmt.query_map([pattern], |r| {
        let status: String = r.get(5)?;
        Ok(InvoiceSummary {
            id: r.get(0)?,
            invoice_no: r.get(1)?,
            order_id: r.get(2)?,
            order_no: r.get(3)?,
            customer_name: r.get(4)?,
            status: OrderStatus::parse(&status).unwrap_or(OrderStatus::Confirmed),
            latest_version: r.get(6)?,
            latest_total_cents: r.get(7)?,
            updated_at: r.get(8)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

pub fn versions(conn: &Connection, invoice_id: i64) -> Result<Vec<InvoiceVersion>> {
    let mut stmt = conn.prepare(
        "SELECT id, version, total_cents, note, created_at FROM invoice_versions
         WHERE invoice_id = ?1 ORDER BY version DESC",
    )?;
    let rows = stmt.query_map([invoice_id], |r| {
        Ok(InvoiceVersion {
            id: r.get(0)?,
            version: r.get(1)?,
            total_cents: r.get(2)?,
            note: r.get(3)?,
            created_at: r.get(4)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

pub fn invoice_for_order(conn: &Connection, order_id: i64) -> Result<Option<i64>> {
    Ok(conn
        .query_row("SELECT id FROM invoices WHERE order_id = ?1", [order_id], |r| r.get(0))
        .optional()?)
}

pub fn load_version(conn: &Connection, version_id: i64) -> Result<InvoiceSnapshot> {
    let json: String = conn
        .query_row("SELECT snapshot_json FROM invoice_versions WHERE id = ?1", [version_id], |r| {
            r.get(0)
        })
        .optional()?
        .ok_or(CoreError::NotFound("Invoice version"))?;
    Ok(serde_json::from_str(&json)?)
}

pub fn latest_version_id(conn: &Connection, invoice_id: i64) -> Result<i64> {
    conn.query_row(
        "SELECT id FROM invoice_versions WHERE invoice_id = ?1 ORDER BY version DESC LIMIT 1",
        [invoice_id],
        |r| r.get(0),
    )
    .optional()?
    .ok_or(CoreError::NotFound("Invoice version"))
}
