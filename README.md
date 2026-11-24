# OpenBilling

Order management and invoicing for small businesses that don't have an ERP.
Built with Rust, [Slint](https://slint.dev) and SQLite. All data stays on the
device.

## Features

- **PIN sign-in with a recovery key.** On first launch you create a 6-digit
  PIN and get a recovery key (`XXXXX-XXXXX-XXXXX-XXXXX-XXXXX`) to keep
  somewhere safe. The key resets a forgotten PIN and is replaced with a new
  one each time it is used. The PIN and key are stored only as Argon2id
  hashes. Repeated wrong attempts trigger an escalating lockout.
- **XML invoice templates.** Business details, currency, tax, invoice prefix,
  accent colour and footer text. You can edit a template in a form or as raw
  XML, export it to a `.xml` file, and import it back.
- **Orders → PDF invoices with versions.** Generating an invoice stores an
  immutable snapshot of the order, customer and template. Editing the order
  and generating again creates v2, v3 and so on. Every version can be
  re-downloaded and produces an identical PDF.
- **Dashboard, listings and reports.** Sales, outstanding and overdue totals,
  a 6-month sales chart, and searchable order, invoice and customer lists.
  Reports can be filtered by date range, status and customer, and exported
  to CSV.
- **Backup and restore.** One `.obbackup` file (a zip with a consistent
  SQLite snapshot and a manifest). Restoring replaces everything, keeps the
  previous database as `openbilling.db.before-restore`, and locks the app.
- **Responsive UI.** A sidebar layout on wide windows and a bottom-navigation
  layout below 880px wide.

## Project layout

| Crate      | Purpose |
|------------|---------|
| `core/`    | All business logic, with no UI code: `auth`, `templates`, `customers`, `orders`, `invoices`, `pdf`, `reports`, `backup`. Front-ends hold an `app_core::App` and call these modules. |
| `desktop/` | Slint UI (`ui/`) and the Rust controllers that connect it to `core` (`src/*_ui.rs`). |
| `mobile/`  | Placeholder for the Android/iOS entry point (see *Platforms*). |

Money is stored as integer cents. Order totals are computed as follows:
subtotal − discount, then tax on the remainder.

## Running

```sh
cargo run -p open-billing            # desktop app
cargo test --workspace               # core + desktop tests
```

To try the app with sample data in a separate folder (the demo PIN is `205813`):

```sh
cargo run -p app-core --example seed_demo -- ./demo-data
OPENBILLING_DATA_DIR=./demo-data cargo run -p open-billing
```

`cargo run -p app-core --example sample_invoice -- out.pdf` renders a sample
invoice PDF.

Data location (override with `OPENBILLING_DATA_DIR`):

- Windows: `%APPDATA%\OpenBilling\OpenBilling\data`
- macOS: `~/Library/Application Support/com.OpenBilling.OpenBilling`
- Linux: `~/.local/share/openbilling`

## Template XML

```xml
<invoice-template version="1">
  <name>Standard</name>
  <business>
    <name>My Business</name>
    <address>123 Main Street
Your City</address>
    <phone/><email/><website/><tax-id/>
  </business>
  <settings>
    <title>INVOICE</title>
    <currency-code>USD</currency-code>
    <currency-symbol>$</currency-symbol>
    <invoice-prefix>INV-</invoice-prefix>
    <tax-label>Tax</tax-label>
    <default-tax-rate>0</default-tax-rate>
    <payment-terms-days>14</payment-terms-days>
    <accent-color>#4F46E5</accent-color>
  </settings>
  <footer>
    <notes>Thank you for your business!</notes>
    <terms>Payment is due within 14 days.</terms>
    <payment-instructions/>
  </footer>
</invoice-template>
```

Only `<name>` and `<business><name>` are required. Missing elements use the
defaults shown above, and unknown elements are ignored.

## Platforms and known limitations

- **Windows, macOS, Linux:** supported by the desktop crate (Slint winit
  backend, native file dialogs through `rfd`). Only Windows has been run and
  checked so far.
- **Android and iOS:** `core` is platform-independent, but the mobile app is
  not wired up yet. The next step is to move the Slint UI and controllers into
  a shared crate, start it from `mobile/` through Slint's Android and iOS
  backends, and replace `rfd` dialogs with platform share/pick sheets. The UI
  already has a phone-width layout.
- PDFs use the built-in PDF fonts, which cover Latin-1 only. Other scripts
  (for example Sinhala, Tamil or the ₹ sign) print as `?` until a TTF font is
  embedded.
- Backup files are not encrypted.
