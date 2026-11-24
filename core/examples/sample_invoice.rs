//! Renders a sample invoice: `cargo run -p app-core --example sample_invoice -- out.pdf`
use app_core::customers::{self, CustomerInput};
use app_core::orders::{self, OrderInput, OrderItem, OrderStatus};
use app_core::{App, invoices, pdf, templates};

fn main() -> app_core::Result<()> {
    let out = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "sample-invoice.pdf".into());
    let app = App::open_in_memory()?;
    let c = app.conn();
    let mut t = templates::get_default(c)?.template;
    t.business.name = "Sunrise Bakery & Cafe".into();
    t.business.address = "42 Galle Road\nColombo 03, Sri Lanka".into();
    t.business.phone = "+94 11 234 5678".into();
    t.business.email = "hello@sunrise.example".into();
    t.business.tax_id = "VAT-114455".into();
    t.settings.tax_label = "VAT".into();
    t.footer.payment_instructions = "Bank: Example Bank, Acc 0012 3456 7890".into();
    templates::update(c, templates::default_id(c)?, &t.to_xml()?)?;
    let cust = customers::create(
        c,
        &CustomerInput {
            name: "Ocean View Hotel".into(),
            email: "accounts@oceanview.example".into(),
            phone: "+94 77 123 4567".into(),
            address: "18 Beach Road\nNegombo".into(),
        },
    )?;
    let id = orders::create(
        c,
        &OrderInput {
            customer_id: cust,
            template_id: None,
            status: OrderStatus::Draft,
            order_date: "2026-11-20".into(),
            due_date: "2026-12-04".into(),
            notes: "Delivered to the main kitchen.".into(),
            discount_cents: 2500,
            tax_rate: 18.0,
            items: vec![
                OrderItem {
                    description: "Sourdough loaves".into(),
                    quantity: 24.0,
                    unit_price_cents: 650,
                },
                OrderItem {
                    description: "Butter croissants (box of 12)".into(),
                    quantity: 5.0,
                    unit_price_cents: 1800,
                },
                OrderItem {
                    description:
                        "Custom celebration cake, three tiers with fondant decoration and delivery"
                            .into(),
                    quantity: 1.0,
                    unit_price_cents: 12500,
                },
            ],
        },
    )?;
    let g = invoices::generate(c, id, "")?;
    std::fs::write(&out, pdf::render_invoice(&g.snapshot)?)?;
    println!("wrote {out}");
    Ok(())
}
