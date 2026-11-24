use rusqlite::{Connection, OptionalExtension, params};

use crate::db::now_string;
use crate::error::{CoreError, Result, invalid};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct CustomerInput {
    pub name: String,
    pub email: String,
    pub phone: String,
    pub address: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Customer {
    pub id: i64,
    pub name: String,
    pub email: String,
    pub phone: String,
    pub address: String,
    pub created_at: String,
    /// Orders that are not cancelled.
    pub order_count: i64,
    /// Total of non-draft, non-cancelled orders.
    pub billed_cents: i64,
}

impl CustomerInput {
    fn validate(&self) -> Result<()> {
        if self.name.trim().is_empty() {
            return invalid("Customer name is required.");
        }
        let email = self.email.trim();
        if !email.is_empty() && (!email.contains('@') || email.contains(' ')) {
            return invalid("That email address doesn't look right.");
        }
        Ok(())
    }
}

const SELECT: &str = "
    SELECT c.id, c.name, c.email, c.phone, c.address, c.created_at,
           COUNT(o.id) FILTER (WHERE o.status <> 'cancelled'),
           COALESCE(SUM(o.total_cents) FILTER (WHERE o.status IN ('confirmed','paid')), 0)
    FROM customers c
    LEFT JOIN orders o ON o.customer_id = c.id";

fn map_row(r: &rusqlite::Row) -> rusqlite::Result<Customer> {
    Ok(Customer {
        id: r.get(0)?,
        name: r.get(1)?,
        email: r.get(2)?,
        phone: r.get(3)?,
        address: r.get(4)?,
        created_at: r.get(5)?,
        order_count: r.get(6)?,
        billed_cents: r.get(7)?,
    })
}

pub fn list(conn: &Connection, search: &str) -> Result<Vec<Customer>> {
    let pattern = format!("%{}%", search.trim());
    let mut stmt = conn.prepare(&format!(
        "{SELECT}
         WHERE c.name LIKE ?1 OR c.email LIKE ?1 OR c.phone LIKE ?1
         GROUP BY c.id
         ORDER BY c.name COLLATE NOCASE"
    ))?;
    let rows = stmt.query_map([pattern], map_row)?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

pub fn get(conn: &Connection, id: i64) -> Result<Customer> {
    conn.query_row(
        &format!("{SELECT} WHERE c.id = ?1 GROUP BY c.id"),
        [id],
        map_row,
    )
    .optional()?
    .ok_or(CoreError::NotFound("Customer"))
}

pub fn create(conn: &Connection, input: &CustomerInput) -> Result<i64> {
    input.validate()?;
    conn.execute(
        "INSERT INTO customers (name, email, phone, address, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            input.name.trim(),
            input.email.trim(),
            input.phone.trim(),
            input.address.trim(),
            now_string()
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn update(conn: &Connection, id: i64, input: &CustomerInput) -> Result<()> {
    input.validate()?;
    let n = conn.execute(
        "UPDATE customers SET name = ?1, email = ?2, phone = ?3, address = ?4 WHERE id = ?5",
        params![
            input.name.trim(),
            input.email.trim(),
            input.phone.trim(),
            input.address.trim(),
            id
        ],
    )?;
    if n == 0 {
        return Err(CoreError::NotFound("Customer"));
    }
    Ok(())
}

pub fn delete(conn: &Connection, id: i64) -> Result<()> {
    let orders: i64 = conn.query_row(
        "SELECT COUNT(*) FROM orders WHERE customer_id = ?1",
        [id],
        |r| r.get(0),
    )?;
    if orders > 0 {
        return invalid(format!(
            "This customer has {orders} order{} and can't be deleted.",
            if orders == 1 { "" } else { "s" }
        ));
    }
    conn.execute("DELETE FROM customers WHERE id = ?1", [id])?;
    Ok(())
}
