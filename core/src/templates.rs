//! Invoice templates.
//!
//! A template is an XML document (root `<invoice-template>`) that holds the
//! business details, invoice settings and footer text. The XML is the source of
//! truth: it is what users download, edit and upload, and it is stored verbatim
//! (after validation) in the database.

use quick_xml::events::Event;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::db::now_string;
use crate::error::{CoreError, Result, invalid};

pub const ROOT_ELEMENT: &str = "invoice-template";
pub const FORMAT_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename = "invoice-template", rename_all = "kebab-case")]
pub struct InvoiceTemplate {
    #[serde(rename = "@version", default = "format_version")]
    pub version: u32,
    pub name: String,
    pub business: Business,
    #[serde(default)]
    pub settings: TemplateSettings,
    #[serde(default)]
    pub footer: Footer,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Business {
    pub name: String,
    #[serde(default)]
    pub address: String,
    #[serde(default)]
    pub phone: String,
    #[serde(default)]
    pub email: String,
    #[serde(default)]
    pub website: String,
    #[serde(default)]
    pub tax_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", default)]
pub struct TemplateSettings {
    /// Title printed at the top of the document, e.g. "INVOICE" or "TAX INVOICE".
    pub title: String,
    pub currency_code: String,
    pub currency_symbol: String,
    pub invoice_prefix: String,
    pub tax_label: String,
    pub default_tax_rate: f64,
    pub payment_terms_days: u32,
    /// `#RRGGBB`
    pub accent_color: String,
}

impl Default for TemplateSettings {
    fn default() -> Self {
        Self {
            title: "INVOICE".into(),
            currency_code: "USD".into(),
            currency_symbol: "$".into(),
            invoice_prefix: "INV-".into(),
            tax_label: "Tax".into(),
            default_tax_rate: 0.0,
            payment_terms_days: 14,
            accent_color: "#4F46E5".into(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", default)]
pub struct Footer {
    pub notes: String,
    pub terms: String,
    pub payment_instructions: String,
}

fn format_version() -> u32 {
    FORMAT_VERSION
}

impl Default for InvoiceTemplate {
    fn default() -> Self {
        Self {
            version: FORMAT_VERSION,
            name: "Standard".into(),
            business: Business {
                name: "My Business".into(),
                address: "123 Main Street\nYour City".into(),
                phone: String::new(),
                email: String::new(),
                website: String::new(),
                tax_id: String::new(),
            },
            settings: TemplateSettings::default(),
            footer: Footer {
                notes: "Thank you for your business!".into(),
                terms: "Payment is due within 14 days of the invoice date.".into(),
                payment_instructions: String::new(),
            },
        }
    }
}

impl InvoiceTemplate {
    pub fn validate(&self) -> Result<()> {
        if self.name.trim().is_empty() {
            return invalid("Template name is required.");
        }
        if self.business.name.trim().is_empty() {
            return invalid("Business name is required.");
        }
        let s = &self.settings;
        if s.title.trim().is_empty() {
            return invalid("Document title is required (e.g. INVOICE).");
        }
        if s.currency_symbol.trim().is_empty() || s.currency_symbol.chars().count() > 6 {
            return invalid("Currency symbol must be 1–6 characters.");
        }
        let prefix_ok = !s.invoice_prefix.is_empty()
            && s.invoice_prefix.len() <= 12
            && s.invoice_prefix
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '/'));
        if !prefix_ok {
            return invalid("Invoice prefix must be 1–12 letters, digits, '-', '_' or '/'.");
        }
        if !(0.0..=100.0).contains(&s.default_tax_rate) {
            return invalid("Default tax rate must be between 0 and 100.");
        }
        if s.payment_terms_days > 365 {
            return invalid("Payment terms must be 365 days or fewer.");
        }
        if parse_hex_color(&s.accent_color).is_none() {
            return invalid("Accent color must look like #4F46E5.");
        }
        Ok(())
    }

    pub fn to_xml(&self) -> Result<String> {
        let mut body = String::new();
        let mut ser = quick_xml::se::Serializer::new(&mut body);
        ser.indent(' ', 2);
        self.serialize(ser)
            .map_err(|e| CoreError::Xml(e.to_string()))?;
        Ok(format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
             <!-- OpenBilling invoice template. Edit the values, keep the element names. -->\n\
             {body}\n"
        ))
    }

    /// Parses and validates a template document.
    pub fn from_xml(xml: &str) -> Result<Self> {
        check_root(xml)?;
        let t: InvoiceTemplate =
            quick_xml::de::from_str(xml).map_err(|e| CoreError::Xml(e.to_string()))?;
        if t.version > FORMAT_VERSION {
            return Err(CoreError::Xml(format!(
                "template format version {} is newer than this app supports ({FORMAT_VERSION})",
                t.version
            )));
        }
        t.validate()?;
        Ok(t)
    }
}

fn check_root(xml: &str) -> Result<()> {
    let mut reader = quick_xml::Reader::from_str(xml);
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                let name = e.name().0.to_string();
                return if name == ROOT_ELEMENT {
                    Ok(())
                } else {
                    Err(CoreError::Xml(format!(
                        "root element must be <{ROOT_ELEMENT}>, found <{name}>"
                    )))
                };
            }
            Ok(Event::Eof) => return Err(CoreError::Xml("the document is empty".into())),
            Err(e) => return Err(CoreError::Xml(e.to_string())),
            Ok(_) => {}
        }
    }
}

pub fn parse_hex_color(s: &str) -> Option<(u8, u8, u8)> {
    let h = s.trim().strip_prefix('#')?;
    if h.len() != 6 {
        return None;
    }
    let v = u32::from_str_radix(h, 16).ok()?;
    Some(((v >> 16) as u8, (v >> 8) as u8, v as u8))
}

// ---------------------------------------------------------------------------
// Persistence
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct TemplateRecord {
    pub id: i64,
    pub is_default: bool,
    pub updated_at: String,
    pub xml: String,
    pub template: InvoiceTemplate,
}

pub fn list(conn: &Connection) -> Result<Vec<TemplateRecord>> {
    let mut stmt = conn.prepare(
        "SELECT id, is_default, updated_at, xml FROM templates ORDER BY is_default DESC, name COLLATE NOCASE",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, bool>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (id, is_default, updated_at, xml) = row?;
        // Stored XML was validated on save; fall back defensively anyway.
        let template = InvoiceTemplate::from_xml(&xml).unwrap_or_default();
        out.push(TemplateRecord {
            id,
            is_default,
            updated_at,
            xml,
            template,
        });
    }
    Ok(out)
}

pub fn get(conn: &Connection, id: i64) -> Result<TemplateRecord> {
    let (is_default, updated_at, xml): (bool, String, String) = conn
        .query_row(
            "SELECT is_default, updated_at, xml FROM templates WHERE id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?
        .ok_or(CoreError::NotFound("Template"))?;
    let template = InvoiceTemplate::from_xml(&xml)?;
    Ok(TemplateRecord {
        id,
        is_default,
        updated_at,
        xml,
        template,
    })
}

/// Id of the default template, creating a starter template if none exist.
pub fn default_id(conn: &Connection) -> Result<i64> {
    if let Some(id) = conn
        .query_row(
            "SELECT id FROM templates ORDER BY is_default DESC, id LIMIT 1",
            [],
            |r| r.get(0),
        )
        .optional()?
    {
        return Ok(id);
    }
    let id = create(conn, &InvoiceTemplate::default().to_xml()?)?;
    set_default(conn, id)?;
    Ok(id)
}

pub fn get_default(conn: &Connection) -> Result<TemplateRecord> {
    get(conn, default_id(conn)?)
}

pub fn create(conn: &Connection, xml: &str) -> Result<i64> {
    let t = InvoiceTemplate::from_xml(xml)?;
    let now = now_string();
    let has_any: bool =
        conn.query_row("SELECT EXISTS(SELECT 1 FROM templates)", [], |r| r.get(0))?;
    conn.execute(
        "INSERT INTO templates (name, xml, is_default, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?4)",
        params![t.name.trim(), xml, !has_any, now],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn update(conn: &Connection, id: i64, xml: &str) -> Result<()> {
    let t = InvoiceTemplate::from_xml(xml)?;
    let n = conn.execute(
        "UPDATE templates SET name = ?1, xml = ?2, updated_at = ?3 WHERE id = ?4",
        params![t.name.trim(), xml, now_string(), id],
    )?;
    if n == 0 {
        return Err(CoreError::NotFound("Template"));
    }
    Ok(())
}

pub fn set_default(conn: &Connection, id: i64) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    tx.execute("UPDATE templates SET is_default = 0", [])?;
    let n = tx.execute("UPDATE templates SET is_default = 1 WHERE id = ?1", [id])?;
    if n == 0 {
        return Err(CoreError::NotFound("Template"));
    }
    tx.commit()?;
    Ok(())
}

pub fn delete(conn: &Connection, id: i64) -> Result<()> {
    let count: i64 = conn.query_row("SELECT COUNT(*) FROM templates", [], |r| r.get(0))?;
    if count <= 1 {
        return invalid("You need at least one template.");
    }
    let was_default: bool = conn
        .query_row(
            "SELECT is_default FROM templates WHERE id = ?1",
            [id],
            |r| r.get(0),
        )
        .optional()?
        .ok_or(CoreError::NotFound("Template"))?;
    let tx = conn.unchecked_transaction()?;
    // Orders that used this template fall back to the default (ON DELETE SET NULL).
    tx.execute("DELETE FROM templates WHERE id = ?1", [id])?;
    if was_default {
        tx.execute(
            "UPDATE templates SET is_default = 1 WHERE id = (SELECT MIN(id) FROM templates)",
            [],
        )?;
    }
    tx.commit()?;
    Ok(())
}

pub fn duplicate(conn: &Connection, id: i64) -> Result<i64> {
    let mut t = get(conn, id)?.template;
    t.name = format!("{} (copy)", t.name);
    create(conn, &t.to_xml()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xml_round_trip() {
        let mut t = InvoiceTemplate::default();
        t.business.address = "Line 1\nLine 2 & <co>".into();
        t.settings.default_tax_rate = 8.5;
        let xml = t.to_xml().unwrap();
        assert!(xml.contains("<invoice-template version=\"1\">"), "{xml}");
        let back = InvoiceTemplate::from_xml(&xml).unwrap();
        assert_eq!(back, t);
    }

    #[test]
    fn minimal_xml_uses_defaults() {
        let xml = r#"<invoice-template><name>Mini</name><business><name>Shop</name></business></invoice-template>"#;
        let t = InvoiceTemplate::from_xml(xml).unwrap();
        assert_eq!(t.settings.invoice_prefix, "INV-");
        assert_eq!(t.business.phone, "");
    }

    #[test]
    fn rejects_bad_documents() {
        assert!(InvoiceTemplate::from_xml("<foo/>").is_err());
        assert!(InvoiceTemplate::from_xml("not xml at all").is_err());
        let bad_color = InvoiceTemplate {
            settings: TemplateSettings {
                accent_color: "blue".into(),
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(InvoiceTemplate::from_xml(&bad_color.to_xml().unwrap()).is_err());
    }
}
