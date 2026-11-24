use rusqlite::{Connection, OptionalExtension, params};
use std::path::Path;

use crate::error::Result;

/// Schema version written to `PRAGMA user_version`. Bump it and append a step
/// to `MIGRATIONS` whenever the schema changes; never edit an old step.
pub const SCHEMA_VERSION: i64 = 1;

const MIGRATIONS: &[&str] = &[
    // v1: initial schema
    r#"
    CREATE TABLE settings (
        key   TEXT PRIMARY KEY,
        value TEXT NOT NULL
    );

    CREATE TABLE customers (
        id         INTEGER PRIMARY KEY,
        name       TEXT NOT NULL,
        email      TEXT NOT NULL DEFAULT '',
        phone      TEXT NOT NULL DEFAULT '',
        address    TEXT NOT NULL DEFAULT '',
        created_at TEXT NOT NULL
    );
    CREATE INDEX idx_customers_name ON customers(name COLLATE NOCASE);

    CREATE TABLE templates (
        id         INTEGER PRIMARY KEY,
        name       TEXT NOT NULL,
        xml        TEXT NOT NULL,
        is_default INTEGER NOT NULL DEFAULT 0,
        created_at TEXT NOT NULL,
        updated_at TEXT NOT NULL
    );

    CREATE TABLE orders (
        id             INTEGER PRIMARY KEY,
        order_no       TEXT NOT NULL UNIQUE,
        customer_id    INTEGER NOT NULL REFERENCES customers(id),
        template_id    INTEGER REFERENCES templates(id) ON DELETE SET NULL,
        status         TEXT NOT NULL,
        order_date     TEXT NOT NULL,
        due_date       TEXT NOT NULL,
        notes          TEXT NOT NULL DEFAULT '',
        discount_cents INTEGER NOT NULL DEFAULT 0,
        tax_rate       REAL NOT NULL DEFAULT 0,
        subtotal_cents INTEGER NOT NULL DEFAULT 0,
        tax_cents      INTEGER NOT NULL DEFAULT 0,
        total_cents    INTEGER NOT NULL DEFAULT 0,
        created_at     TEXT NOT NULL,
        updated_at     TEXT NOT NULL
    );
    CREATE INDEX idx_orders_date ON orders(order_date);
    CREATE INDEX idx_orders_customer ON orders(customer_id);

    CREATE TABLE order_items (
        id               INTEGER PRIMARY KEY,
        order_id         INTEGER NOT NULL REFERENCES orders(id) ON DELETE CASCADE,
        position         INTEGER NOT NULL,
        description      TEXT NOT NULL,
        quantity         REAL NOT NULL,
        unit_price_cents INTEGER NOT NULL
    );
    CREATE INDEX idx_items_order ON order_items(order_id);

    CREATE TABLE invoices (
        id         INTEGER PRIMARY KEY,
        order_id   INTEGER NOT NULL UNIQUE REFERENCES orders(id) ON DELETE CASCADE,
        invoice_no TEXT NOT NULL UNIQUE,
        created_at TEXT NOT NULL
    );

    CREATE TABLE invoice_versions (
        id            INTEGER PRIMARY KEY,
        invoice_id    INTEGER NOT NULL REFERENCES invoices(id) ON DELETE CASCADE,
        version       INTEGER NOT NULL,
        snapshot_json TEXT NOT NULL,
        total_cents   INTEGER NOT NULL,
        note          TEXT NOT NULL DEFAULT '',
        created_at    TEXT NOT NULL,
        UNIQUE (invoice_id, version)
    );
    "#,
];

pub fn open(path: &Path) -> Result<Connection> {
    let conn = Connection::open(path)?;
    configure(&conn)?;
    migrate(&conn)?;
    Ok(conn)
}

pub fn open_in_memory() -> Result<Connection> {
    let conn = Connection::open_in_memory()?;
    configure(&conn)?;
    migrate(&conn)?;
    Ok(conn)
}

fn configure(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "PRAGMA foreign_keys = ON;
         PRAGMA journal_mode = WAL;
         PRAGMA synchronous = NORMAL;",
    )?;
    Ok(())
}

pub fn schema_version(conn: &Connection) -> Result<i64> {
    Ok(conn.query_row("PRAGMA user_version", [], |r| r.get(0))?)
}

fn migrate(conn: &Connection) -> Result<()> {
    let current = schema_version(conn)?;
    for (i, sql) in MIGRATIONS.iter().enumerate() {
        let version = i as i64 + 1;
        if version > current {
            let tx = conn.unchecked_transaction()?;
            tx.execute_batch(sql)?;
            tx.execute_batch(&format!("PRAGMA user_version = {version}"))?;
            tx.commit()?;
        }
    }
    Ok(())
}

pub fn get_setting(conn: &Connection, key: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| {
            r.get(0)
        })
        .optional()?)
}

pub fn set_setting(conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

pub fn now_string() -> String {
    chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

pub fn today_string() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}
