//! Renders an [`InvoiceSnapshot`] to an A4 PDF using the standard PDF fonts
//! (no font files to ship). Standard fonts only cover Latin-1 (WinAnsi);
//! other characters are replaced with `?`.

use printpdf::{
    BuiltinFont, Color, IndirectFontRef, Line, Mm, PdfDocument, PdfDocumentReference,
    PdfLayerReference, Point, Rect, Rgb,
};

use crate::error::{CoreError, Result};
use crate::invoices::InvoiceSnapshot;
use crate::money::{format_money, format_percent, format_quantity};
use crate::orders::OrderStatus;
use crate::templates::parse_hex_color;

const PAGE_W: f32 = 210.0;
const PAGE_H: f32 = 297.0;
const MARGIN: f32 = 18.0;
const RIGHT: f32 = PAGE_W - MARGIN;
const ROW_H: f32 = 7.5;
/// Lowest y an item row may use before a page break.
const TABLE_BOTTOM: f32 = 30.0;

const INK: (u8, u8, u8) = (15, 23, 42);
const MUTED: (u8, u8, u8) = (100, 116, 139);
const RULE: (u8, u8, u8) = (226, 232, 240);
const PAID: (u8, u8, u8) = (4, 120, 87);

struct Fonts {
    regular: IndirectFontRef,
    bold: IndirectFontRef,
}

struct Pen<'a> {
    layer: PdfLayerReference,
    fonts: &'a Fonts,
}

impl Pen<'_> {
    fn text(&self, s: &str, size: f32, x: f32, y: f32, bold: bool, color: (u8, u8, u8)) {
        self.layer.set_fill_color(rgb(color));
        let font = if bold { &self.fonts.bold } else { &self.fonts.regular };
        self.layer.use_text(sanitize(s), size, Mm(x), Mm(y), font);
    }

    fn text_right(&self, s: &str, size: f32, right: f32, y: f32, bold: bool, color: (u8, u8, u8)) {
        let w = text_width(s, size, bold);
        self.text(s, size, right - w, y, bold, color);
    }

    fn text_center(&self, s: &str, size: f32, cx: f32, y: f32, bold: bool, color: (u8, u8, u8)) {
        let w = text_width(s, size, bold);
        self.text(s, size, cx - w / 2.0, y, bold, color);
    }

    fn rect(&self, x1: f32, y1: f32, x2: f32, y2: f32, color: (u8, u8, u8)) {
        self.layer.set_fill_color(rgb(color));
        self.layer.add_rect(Rect::new(Mm(x1), Mm(y1), Mm(x2), Mm(y2)));
    }

    fn hline(&self, x1: f32, x2: f32, y: f32, color: (u8, u8, u8), thickness: f32) {
        self.layer.set_outline_color(rgb(color));
        self.layer.set_outline_thickness(thickness);
        self.layer.add_line(Line {
            points: vec![(Point::new(Mm(x1), Mm(y)), false), (Point::new(Mm(x2), Mm(y)), false)],
            is_closed: false,
        });
    }
}

pub fn render_invoice(inv: &InvoiceSnapshot) -> Result<Vec<u8>> {
    let t = &inv.template;
    let accent = parse_hex_color(&t.settings.accent_color).unwrap_or((79, 70, 229));
    let tint = mix(accent, (255, 255, 255), 0.90);
    let sym = t.settings.currency_symbol.as_str();
    let money = |c: i64| format_money(c, sym);

    let title = format!("{} {}", t.settings.title, inv.invoice_no);
    let (doc, page, layer) = PdfDocument::new(&title, Mm(PAGE_W), Mm(PAGE_H), "content");
    let fonts = Fonts {
        regular: doc.add_builtin_font(BuiltinFont::Helvetica).map_err(pdf_err)?,
        bold: doc.add_builtin_font(BuiltinFont::HelveticaBold).map_err(pdf_err)?,
    };
    let mut pages = vec![doc.get_page(page).get_layer(layer)];
    let mut pen = Pen { layer: pages[0].clone(), fonts: &fonts };

    // --- Header band -------------------------------------------------------
    pen.rect(0.0, PAGE_H - 6.0, PAGE_W, PAGE_H, accent);

    // Business block (left)
    let mut y = PAGE_H - 22.0;
    for line in wrap(&t.business.name, 18.0, 105.0, true) {
        pen.text(&line, 18.0, MARGIN, y, true, INK);
        y -= 7.0;
    }
    y -= 1.0;
    let mut contact: Vec<String> = t.business.address.lines().map(str::to_string).collect();
    contact.extend(
        [&t.business.phone, &t.business.email, &t.business.website]
            .into_iter()
            .filter(|s| !s.trim().is_empty())
            .cloned(),
    );
    if !t.business.tax_id.trim().is_empty() {
        contact.push(format!("Tax ID: {}", t.business.tax_id));
    }
    for line in contact.iter().flat_map(|l| wrap(l, 9.0, 105.0, false)) {
        pen.text(&line, 9.0, MARGIN, y, false, MUTED);
        y -= 4.6;
    }
    let left_bottom = y;

    // Title + meta (right)
    let mut ry = PAGE_H - 24.0;
    pen.text_right(&t.settings.title, 24.0, RIGHT, ry, true, accent);
    ry -= 9.0;
    let mut meta = vec![
        ("Invoice no.", inv.invoice_no.clone()),
        ("Issue date", inv.issue_date.clone()),
        ("Due date", inv.due_date.clone()),
        ("Order no.", inv.order_no.clone()),
    ];
    if inv.version > 1 {
        meta.push(("Revision", inv.version.to_string()));
    }
    for (label, value) in &meta {
        pen.text_right(value, 9.5, RIGHT, ry, true, INK);
        pen.text_right(label, 9.0, RIGHT - 38.0, ry, false, MUTED);
        ry -= 5.2;
    }
    if inv.status == OrderStatus::Paid {
        ry -= 3.0;
        pen.text_right("PAID", 16.0, RIGHT, ry, true, PAID);
        ry -= 6.0;
    }

    // --- Bill to -----------------------------------------------------------
    y = left_bottom.min(ry) - 8.0;
    pen.text("BILL TO", 8.5, MARGIN, y, true, accent);
    y -= 6.0;
    pen.text(&inv.customer.name, 11.5, MARGIN, y, true, INK);
    y -= 5.2;
    let mut cust: Vec<String> = inv.customer.address.lines().map(str::to_string).collect();
    cust.extend(
        [&inv.customer.phone, &inv.customer.email]
            .into_iter()
            .filter(|s| !s.trim().is_empty())
            .cloned(),
    );
    for line in cust.iter().flat_map(|l| wrap(l, 9.0, 110.0, false)) {
        pen.text(&line, 9.0, MARGIN, y, false, MUTED);
        y -= 4.6;
    }
    y -= 6.0;

    // --- Items table -------------------------------------------------------
    const C_NUM: f32 = MARGIN + 2.0;
    const C_DESC: f32 = MARGIN + 10.0;
    const DESC_W: f32 = 88.0;
    const C_QTY: f32 = 128.0;
    const C_PRICE: f32 = 157.0;
    let header = |pen: &Pen, y: f32| {
        pen.rect(MARGIN, y - 2.6, RIGHT, y + 5.0, tint);
        pen.text("#", 8.5, C_NUM, y, true, accent);
        pen.text("DESCRIPTION", 8.5, C_DESC, y, true, accent);
        pen.text_right("QTY", 8.5, C_QTY, y, true, accent);
        pen.text_right("UNIT PRICE", 8.5, C_PRICE, y, true, accent);
        pen.text_right("AMOUNT", 8.5, RIGHT - 2.0, y, true, accent);
    };
    header(&pen, y);
    y -= ROW_H + 1.0;

    for (i, item) in inv.items.iter().enumerate() {
        let lines = wrap(&item.description, 9.5, DESC_W, false);
        let needed = ROW_H + (lines.len().saturating_sub(1)) as f32 * 4.4;
        if y - needed < TABLE_BOTTOM {
            pen = new_page(&doc, &fonts, &mut pages);
            y = PAGE_H - MARGIN - 6.0;
            header(&pen, y);
            y -= ROW_H + 1.0;
        }
        pen.text(&(i + 1).to_string(), 9.5, C_NUM, y, false, MUTED);
        for (j, line) in lines.iter().enumerate() {
            pen.text(line, 9.5, C_DESC, y - j as f32 * 4.4, false, INK);
        }
        pen.text_right(&format_quantity(item.quantity), 9.5, C_QTY, y, false, INK);
        pen.text_right(&money(item.unit_price_cents), 9.5, C_PRICE, y, false, INK);
        pen.text_right(&money(item.line_total_cents()), 9.5, RIGHT - 2.0, y, false, INK);
        y -= needed;
        pen.hline(MARGIN, RIGHT, y + 4.2, RULE, 0.5);
    }

    // --- Totals ------------------------------------------------------------
    let totals_h = 34.0;
    if y - totals_h < TABLE_BOTTOM {
        pen = new_page(&doc, &fonts, &mut pages);
        y = PAGE_H - MARGIN - 6.0;
    }
    y -= 3.0;
    let label_x = 150.0;
    let row = |pen: &Pen, label: &str, value: &str, y: &mut f32| {
        pen.text_right(label, 9.5, label_x, *y, false, MUTED);
        pen.text_right(value, 9.5, RIGHT - 2.0, *y, false, INK);
        *y -= 5.6;
    };
    row(&pen, "Subtotal", &money(inv.totals.subtotal_cents), &mut y);
    if inv.totals.discount_cents > 0 {
        row(&pen, "Discount", &format!("-{}", money(inv.totals.discount_cents)), &mut y);
    }
    if inv.tax_rate > 0.0 || inv.totals.tax_cents != 0 {
        let label = format!("{} ({}%)", t.settings.tax_label, format_percent(inv.tax_rate));
        row(&pen, &label, &money(inv.totals.tax_cents), &mut y);
    }
    y -= 1.5;
    pen.rect(110.0, y - 3.4, RIGHT, y + 6.0, accent);
    pen.text("TOTAL", 10.5, 114.0, y, true, (255, 255, 255));
    let total_label = if t.settings.currency_code.trim().is_empty() {
        money(inv.totals.total_cents)
    } else {
        format!("{} {}", money(inv.totals.total_cents), t.settings.currency_code.trim())
    };
    pen.text_right(&total_label, 11.5, RIGHT - 2.0, y, true, (255, 255, 255));
    y -= 14.0;

    // --- Notes, terms and payment instructions -----------------------------
    let blocks = [
        ("NOTES", inv.notes.as_str()),
        ("PAYMENT DETAILS", t.footer.payment_instructions.as_str()),
        ("TERMS", t.footer.terms.as_str()),
    ];
    for (heading, body) in blocks.iter().filter(|(_, b)| !b.trim().is_empty()) {
        let lines: Vec<String> =
            body.lines().flat_map(|l| wrap(l, 9.0, PAGE_W - 2.0 * MARGIN, false)).collect();
        let needed = 6.0 + lines.len() as f32 * 4.4;
        if y - needed < TABLE_BOTTOM {
            pen = new_page(&doc, &fonts, &mut pages);
            y = PAGE_H - MARGIN - 6.0;
        }
        pen.text(heading, 8.5, MARGIN, y, true, accent);
        y -= 5.0;
        for line in &lines {
            pen.text(line, 9.0, MARGIN, y, false, MUTED);
            y -= 4.4;
        }
        y -= 4.0;
    }

    // --- Footer on every page ---------------------------------------------
    let n = pages.len();
    for (i, layer) in pages.iter().enumerate() {
        let pen = Pen { layer: layer.clone(), fonts: &fonts };
        pen.hline(MARGIN, RIGHT, 18.0, RULE, 0.5);
        if !t.footer.notes.trim().is_empty() {
            pen.text_center(&t.footer.notes, 9.0, PAGE_W / 2.0, 12.5, true, accent);
        }
        pen.text(&format!("{} {}", inv.invoice_no, version_tag(inv)), 7.5, MARGIN, 7.0, false, MUTED);
        pen.text_right(&format!("Page {} of {n}", i + 1), 7.5, RIGHT, 7.0, false, MUTED);
    }

    doc.save_to_bytes().map_err(pdf_err)
}

fn version_tag(inv: &InvoiceSnapshot) -> String {
    format!("v{}", inv.version)
}

fn new_page<'a>(
    doc: &PdfDocumentReference,
    fonts: &'a Fonts,
    pages: &mut Vec<PdfLayerReference>,
) -> Pen<'a> {
    let (p, l) = doc.add_page(Mm(PAGE_W), Mm(PAGE_H), "content");
    let layer = doc.get_page(p).get_layer(l);
    pages.push(layer.clone());
    Pen { layer, fonts }
}

fn pdf_err(e: impl std::fmt::Display) -> CoreError {
    CoreError::Pdf(e.to_string())
}

fn rgb((r, g, b): (u8, u8, u8)) -> Color {
    Color::Rgb(Rgb::new(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, None))
}

fn mix(a: (u8, u8, u8), b: (u8, u8, u8), t: f32) -> (u8, u8, u8) {
    let m = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    (m(a.0, b.0), m(a.1, b.1), m(a.2, b.2))
}

/// Keeps characters the WinAnsi-encoded standard fonts can show.
fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '\t' => ' ',
            c if (c as u32) < 0x20 => ' ',
            c if (c as u32) <= 0xFF => c,
            '€' | '–' | '—' | '‘' | '’' | '“' | '”' | '•' | '…' | '™' | 'Œ' | 'œ' | 'Š' | 'š'
            | 'Ž' | 'ž' | 'Ÿ' | 'ƒ' | '†' | '‡' | '‰' => c,
            _ => '?',
        })
        .collect()
}

// Helvetica / Helvetica-Bold advance widths (1/1000 em) for ASCII 32..=126,
// from the Adobe Core 14 AFM files.
const HELV: [u16; 95] = [
    278, 278, 355, 556, 556, 889, 667, 191, 333, 333, 389, 584, 278, 333, 278, 278, 556, 556, 556,
    556, 556, 556, 556, 556, 556, 556, 278, 278, 584, 584, 584, 556, 1015, 667, 667, 722, 722, 667,
    611, 778, 722, 278, 500, 667, 556, 833, 722, 778, 667, 778, 722, 667, 611, 722, 667, 944, 667,
    667, 611, 278, 278, 278, 469, 556, 333, 556, 556, 500, 556, 556, 278, 556, 556, 222, 222, 500,
    222, 833, 556, 556, 556, 556, 333, 500, 278, 556, 500, 722, 500, 500, 500, 334, 260, 334, 584,
];
const HELV_BOLD: [u16; 95] = [
    278, 333, 474, 556, 556, 889, 722, 238, 333, 333, 389, 584, 278, 333, 278, 278, 556, 556, 556,
    556, 556, 556, 556, 556, 556, 556, 333, 333, 584, 584, 584, 611, 975, 722, 722, 722, 722, 667,
    611, 778, 722, 278, 556, 722, 611, 833, 722, 778, 667, 778, 722, 667, 611, 722, 667, 944, 667,
    667, 611, 333, 278, 333, 584, 556, 333, 556, 611, 556, 611, 556, 333, 611, 611, 278, 278, 556,
    278, 889, 611, 611, 611, 611, 389, 556, 333, 611, 556, 778, 556, 556, 500, 389, 280, 389, 584,
];

/// Width of `s` in millimetres.
fn text_width(s: &str, size_pt: f32, bold: bool) -> f32 {
    let table = if bold { &HELV_BOLD } else { &HELV };
    let units: u32 = sanitize(s)
        .chars()
        .map(|c| match c as u32 {
            n @ 32..=126 => table[(n - 32) as usize] as u32,
            _ => 556,
        })
        .sum();
    units as f32 / 1000.0 * size_pt * 0.352_778
}

/// Greedy word wrap; very long words are hard-broken.
fn wrap(text: &str, size: f32, max_w: f32, bold: bool) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        let candidate = if current.is_empty() { word.to_string() } else { format!("{current} {word}") };
        if text_width(&candidate, size, bold) <= max_w {
            current = candidate;
            continue;
        }
        if !current.is_empty() {
            lines.push(std::mem::take(&mut current));
        }
        // Hard-break a word that alone is too wide.
        let mut piece = String::new();
        for ch in word.chars() {
            piece.push(ch);
            if text_width(&piece, size, bold) > max_w {
                piece.pop();
                lines.push(std::mem::take(&mut piece));
                piece.push(ch);
            }
        }
        current = piece;
    }
    if !current.is_empty() || lines.is_empty() {
        lines.push(current);
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_long_text() {
        let lines = wrap("the quick brown fox jumps over the lazy dog ".repeat(5).as_str(), 10.0, 50.0, false);
        assert!(lines.len() > 3);
        assert!(lines.iter().all(|l| text_width(l, 10.0, false) <= 50.0));
    }

    #[test]
    fn sanitizes_unsupported_chars() {
        assert_eq!(sanitize("Café €5 ₹"), "Café €5 ?");
    }
}
