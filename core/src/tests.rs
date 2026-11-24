//! End-to-end tests of the core workflows.

use crate::customers::{self, CustomerInput};
use crate::orders::{self, OrderFilter, OrderInput, OrderItem, OrderStatus};
use crate::templates::{self, InvoiceTemplate};
use crate::*;

fn customer(app: &App, name: &str) -> i64 {
    customers::create(
        app.conn(),
        &CustomerInput {
            name: name.into(),
            email: format!("{}@example.com", name.to_lowercase()),
            ..Default::default()
        },
    )
    .unwrap()
}

fn order_input(customer_id: i64, date: &str) -> OrderInput {
    OrderInput {
        customer_id,
        template_id: None,
        status: OrderStatus::Draft,
        order_date: date.into(),
        due_date: date.into(),
        notes: String::new(),
        discount_cents: 0,
        tax_rate: 10.0,
        items: vec![
            OrderItem {
                description: "Widget".into(),
                quantity: 2.0,
                unit_price_cents: 1500,
            },
            OrderItem {
                description: "Setup".into(),
                quantity: 1.0,
                unit_price_cents: 2000,
            },
        ],
    }
}

#[test]
fn pin_setup_login_and_recovery() {
    let app = App::open_in_memory().unwrap();
    let c = app.conn();
    assert!(!auth::is_initialized(c).unwrap());
    assert!(
        auth::setup(c, "111111", "111111").is_err(),
        "repeated digits rejected"
    );
    assert!(
        auth::setup(c, "123456", "123456").is_err(),
        "sequence rejected"
    );
    assert!(
        auth::setup(c, "205813", "205814").is_err(),
        "mismatch rejected"
    );

    let key = auth::setup(c, "205813", "205813").unwrap();
    assert_eq!(key.len(), 29);
    assert!(auth::is_initialized(c).unwrap());
    auth::verify_pin(c, "205813").unwrap();
    assert!(matches!(
        auth::verify_pin(c, "000001"),
        Err(CoreError::Auth(_))
    ));

    // Lowercase, missing dashes and look-alike letters are all accepted.
    let typed = key.to_lowercase().replace('-', " ");
    let new_key = auth::reset_with_recovery_key(c, &typed, "730194", "730194").unwrap();
    assert_ne!(new_key, key);
    auth::verify_pin(c, "730194").unwrap();
    // The used key is now dead.
    assert!(auth::reset_with_recovery_key(c, &key, "482017", "482017").is_err());
}

#[test]
fn lockout_after_repeated_failures() {
    let app = App::open_in_memory().unwrap();
    let c = app.conn();
    auth::setup(c, "205813", "205813").unwrap();
    for _ in 0..4 {
        assert!(matches!(
            auth::verify_pin(c, "999990"),
            Err(CoreError::Auth(_))
        ));
    }
    assert!(matches!(
        auth::verify_pin(c, "999990"),
        Err(CoreError::LockedOut(_))
    ));
    // Even the right PIN is refused while locked.
    assert!(matches!(
        auth::verify_pin(c, "205813"),
        Err(CoreError::LockedOut(_))
    ));
    assert!(auth::lockout_remaining(c).unwrap() > 0);
}

#[test]
fn template_crud_and_default() {
    let app = App::open_in_memory().unwrap();
    let c = app.conn();
    let list = templates::list(c).unwrap();
    assert_eq!(list.len(), 1, "starter template is created");
    let first = list[0].id;

    let mut t = InvoiceTemplate {
        name: "Retail".into(),
        ..Default::default()
    };
    t.settings.invoice_prefix = "RT-".into();
    let id = templates::create(c, &t.to_xml().unwrap()).unwrap();
    templates::set_default(c, id).unwrap();
    assert_eq!(templates::default_id(c).unwrap(), id);
    assert!(templates::update(c, id, "<invoice-template><name/></invoice-template>").is_err());

    templates::delete(c, id).unwrap();
    assert_eq!(
        templates::default_id(c).unwrap(),
        first,
        "default falls back"
    );
    assert!(
        templates::delete(c, first).is_err(),
        "last template is kept"
    );
}

#[test]
fn order_invoice_versions_and_pdf() {
    let app = App::open_in_memory().unwrap();
    let c = app.conn();
    let cust = customer(&app, "Acme");
    let id = orders::create(c, &order_input(cust, "2026-03-10")).unwrap();
    let order = orders::get(c, id).unwrap();
    assert_eq!(order.order_no, "ORD-0001");
    assert_eq!(order.totals.total_cents, 5500);

    let g1 = invoices::generate(c, id, "").unwrap();
    assert_eq!(g1.snapshot.invoice_no, "INV-0001");
    assert_eq!(g1.snapshot.version, 1);
    assert_eq!(
        orders::get(c, id).unwrap().status,
        OrderStatus::Confirmed,
        "draft gets confirmed"
    );

    assert!(
        invoices::generate(c, id, "").is_err(),
        "no-op regeneration refused"
    );

    let mut input = orders::get(c, id).unwrap().to_input();
    input.items[0].quantity = 3.0;
    orders::update(c, id, &input).unwrap();
    let g2 = invoices::generate(c, id, "Added a widget").unwrap();
    assert_eq!(g2.snapshot.version, 2);
    assert_eq!(
        g2.snapshot.invoice_no, "INV-0001",
        "number is stable across versions"
    );

    let versions = invoices::versions(c, g1.invoice_id).unwrap();
    assert_eq!(versions.len(), 2);
    let v1 = invoices::load_version(c, versions[1].id).unwrap();
    assert_eq!(v1.totals.total_cents, 5500, "old version is frozen");

    let pdf = pdf::render_invoice(&g2.snapshot).unwrap();
    assert!(pdf.starts_with(b"%PDF"));

    assert!(
        orders::delete(c, id).is_err(),
        "invoiced orders can't be deleted"
    );
    assert!(
        customers::delete(c, cust).is_err(),
        "customers with orders can't be deleted"
    );
}

#[test]
fn many_items_paginate() {
    let app = App::open_in_memory().unwrap();
    let c = app.conn();
    let cust = customer(&app, "Bulk");
    let mut input = order_input(cust, "2026-03-10");
    input.items = (0..80)
        .map(|i| OrderItem {
            description: format!("Item number {i} with a fairly long description that needs wrapping onto a second line"),
            quantity: 1.0,
            unit_price_cents: 100,
        })
        .collect();
    let id = orders::create(c, &input).unwrap();
    let g = invoices::generate(c, id, "").unwrap();
    let pdf = pdf::render_invoice(&g.snapshot).unwrap();
    assert!(pdf.len() > 4000);
}

#[test]
fn reports_and_dashboard() {
    let app = App::open_in_memory().unwrap();
    let c = app.conn();
    let a = customer(&app, "Alpha");
    let b = customer(&app, "Beta");
    let o1 = orders::create(c, &order_input(a, "2026-02-01")).unwrap();
    let o2 = orders::create(c, &order_input(b, "2026-03-05")).unwrap();
    let _draft = orders::create(c, &order_input(b, "2026-03-06")).unwrap();
    orders::set_status(c, o1, OrderStatus::Paid).unwrap();
    orders::set_status(c, o2, OrderStatus::Confirmed).unwrap();

    let all = reports::run(c, &OrderFilter::default()).unwrap();
    assert_eq!(all.summary.order_count, 3);
    assert_eq!(all.summary.sales_cents, 11000);
    assert_eq!(all.summary.paid_cents, 5500);
    assert_eq!(all.summary.outstanding_cents, 5500);
    assert_eq!(all.summary.draft_count, 1);
    assert_eq!(all.by_month.len(), 2);

    let march = reports::run(
        c,
        &OrderFilter {
            from: Some("2026-03-01".into()),
            to: Some("2026-03-31".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(march.summary.order_count, 2);
    let csv = reports::to_csv(&march);
    assert_eq!(csv.lines().count(), 3);

    let today = chrono::NaiveDate::from_ymd_opt(2026, 3, 20).unwrap();
    let d = reports::dashboard_at(c, today).unwrap();
    assert_eq!(d.sales_this_month_cents, 5500);
    assert_eq!(d.sales_last_month_cents, 5500);
    assert_eq!(d.overdue_count, 1);
    assert_eq!(d.monthly_sales.len(), 6);
}

#[test]
fn backup_and_restore_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = App::open(&dir.path().join("data")).unwrap();
    auth::setup(app.conn(), "205813", "205813").unwrap();
    let cust = customer(&app, "Keep");
    orders::create(app.conn(), &order_input(cust, "2026-03-10")).unwrap();

    let file = dir.path().join("b.obbackup");
    let manifest = backup::create_backup(app.conn(), &file).unwrap();
    assert_eq!(manifest.orders, 1);
    assert_eq!(backup::inspect_backup(&file).unwrap().customers, 1);

    // Diverge after the backup, then restore.
    customer(&app, "Later");
    auth::change_pin(app.conn(), "205813", "730194", "730194").unwrap();
    app.restore_backup(&file).unwrap();

    assert_eq!(customers::list(app.conn(), "").unwrap().len(), 1);
    auth::verify_pin(app.conn(), "205813").unwrap();

    // Garbage files are rejected without touching data.
    let junk = dir.path().join("junk.obbackup");
    std::fs::write(&junk, b"nope").unwrap();
    assert!(app.restore_backup(&junk).is_err());
    assert_eq!(customers::list(app.conn(), "").unwrap().len(), 1);
}
