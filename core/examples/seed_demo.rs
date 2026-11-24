//! Fills a data folder with demo data for trying the app out.
//!
//! ```text
//! cargo run -p app-core --example seed_demo -- <data-dir>
//! OPENBILLING_DATA_DIR=<data-dir> cargo run -p open-billing
//! ```
//! The demo PIN is 205813.

use app_core::customers::{self, CustomerInput};
use app_core::orders::{self, OrderInput, OrderItem, OrderStatus};
use app_core::{App, auth, invoices, templates};
use chrono::{Duration, Local};

fn main() -> app_core::Result<()> {
    let dir = std::env::args().nth(1).expect("usage: seed_demo <data-dir>");
    let app = App::open(std::path::Path::new(&dir))?;
    let c = app.conn();
    if !auth::is_initialized(c)? {
        let key = auth::setup(c, "205813", "205813")?;
        println!("PIN 205813, recovery key {key}");
    }

    let mut t = templates::get_default(c)?.template;
    t.business.name = "Sunrise Bakery & Cafe".into();
    t.business.address = "42 Galle Road\nColombo 03".into();
    t.business.phone = "+94 11 234 5678".into();
    t.business.email = "hello@sunrise.example".into();
    t.settings.currency_code = "LKR".into();
    t.settings.currency_symbol = "Rs".into();
    t.settings.tax_label = "VAT".into();
    t.settings.default_tax_rate = 18.0;
    templates::update(c, templates::default_id(c)?, &t.to_xml()?)?;

    let names = [
        ("Ocean View Hotel", "accounts@oceanview.example"),
        ("Green Leaf Cafe", "orders@greenleaf.example"),
        ("Kandy Events Co.", "hello@kandyevents.example"),
        ("Lakeside School", "office@lakeside.example"),
        ("Nimal Perera", ""),
    ];
    let ids: Vec<i64> = names
        .iter()
        .map(|(n, e)| {
            customers::create(c, &CustomerInput { name: (*n).into(), email: (*e).into(), phone: "+94 77 000 0000".into(), address: "Colombo".into() })
        })
        .collect::<Result<_, _>>()?;

    let products = [
        ("Sourdough loaf", 65000),
        ("Butter croissants (12)", 180000),
        ("Celebration cake", 1250000),
        ("Coffee beans 1kg", 420000),
        ("Catering platter", 850000),
    ];
    let today = Local::now().date_naive();
    for n in 0..36i64 {
        let date = today - Duration::days(n * 5 + (n % 3));
        let items: Vec<OrderItem> = (0..(1 + n % 3))
            .map(|k| {
                let (d, p) = products[((n + k) % 5) as usize];
                OrderItem { description: d.into(), quantity: (1 + (n + k) % 4) as f64, unit_price_cents: p }
            })
            .collect();
        let status = match n % 7 {
            0 => OrderStatus::Draft,
            1 | 4 => OrderStatus::Confirmed,
            6 => OrderStatus::Cancelled,
            _ => OrderStatus::Paid,
        };
        let id = orders::create(c, &OrderInput {
            customer_id: ids[(n % 5) as usize],
            template_id: None,
            status: OrderStatus::Draft,
            order_date: date.format("%Y-%m-%d").to_string(),
            due_date: (date + Duration::days(14)).format("%Y-%m-%d").to_string(),
            notes: String::new(),
            discount_cents: if n % 4 == 0 { 50000 } else { 0 },
            tax_rate: 18.0,
            items,
        })?;
        if status != OrderStatus::Draft && status != OrderStatus::Cancelled {
            invoices::generate(c, id, "")?;
        }
        orders::set_status(c, id, status)?;
    }
    println!("Seeded {dir}");
    Ok(())
}
