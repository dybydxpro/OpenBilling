//! Invoice templates: list, form/XML editor, import and export.

use app_core::money::{format_percent, parse_percent};
use app_core::templates::{self, Business, Footer, InvoiceTemplate, TemplateSettings};
use app_core::{CoreError, Result};
use slint::{ComponentHandle, SharedString};
use std::rc::Rc;

use crate::convert::{hex_color, model, template_row};
use crate::ctx::Ctx;
use crate::{Api, AppState, TemplateForm, TemplatesState, files, nav};

const XML_FILTER: (&str, &[&str]) = ("Invoice template (XML)", &["xml"]);

pub fn init(ctx: &Rc<Ctx>) {
    let ui = ctx.ui();
    let api = ui.global::<Api>();

    let c = ctx.clone();
    api.on_new_template(move || {
        let t = InvoiceTemplate { name: "New template".into(), ..Default::default() };
        open_editor(&c, 0, &t);
    });
    let c = ctx.clone();
    api.on_edit_template(move |id| match c.with_conn(|conn| templates::get(conn, id as i64)) {
        Ok(rec) => open_editor(&c, rec.id, &rec.template),
        Err(e) => c.fail(&e),
    });
    let c = ctx.clone();
    api.on_duplicate_template(move |id| match c.with_conn(|conn| templates::duplicate(conn, id as i64)) {
        Ok(_) => {
            c.success("Template duplicated");
            refresh(&c);
        }
        Err(e) => c.fail(&e),
    });
    let c = ctx.clone();
    api.on_default_template(move |id| match c.with_conn(|conn| templates::set_default(conn, id as i64)) {
        Ok(()) => {
            c.success("Default template updated");
            nav::refresh_business_name(&c);
            refresh(&c);
        }
        Err(e) => c.fail(&e),
    });
    let c = ctx.clone();
    api.on_delete_template(move |id| {
        c.confirm(
            "Delete this template?",
            "Orders that use it will switch to the default template. Invoices already generated keep their original look.",
            "Delete",
            true,
            move |c| match c.with_conn(|conn| templates::delete(conn, id as i64)) {
                Ok(()) => {
                    c.success("Template deleted");
                    refresh(c);
                }
                Err(e) => c.fail(&e),
            },
        );
    });
    let c = ctx.clone();
    api.on_import_template(move || import(&c));
    let c = ctx.clone();
    api.on_export_template(move |id| match c.with_conn(|conn| templates::get(conn, id as i64)) {
        Ok(rec) => export(&c, &rec.template.name, &rec.xml),
        Err(e) => c.fail(&e),
    });

    let c = ctx.clone();
    api.on_template_tab(move |tab| switch_tab(&c, tab.as_str()));
    let c = ctx.clone();
    api.on_template_accent_edited(move || {
        let ui_ = c.ui();
        let f = ui_.global::<TemplateForm>();
        if let Some(color) = hex_color(&f.get_accent()) {
            f.set_accent_preview(color);
        }
    });
    let c = ctx.clone();
    api.on_template_save(move || save(&c));
    let c = ctx.clone();
    api.on_template_cancel(move || nav::navigate(&c, "templates"));
    let c = ctx.clone();
    api.on_template_export_current(move || match current_xml(&c) {
        Ok((t, xml)) => export(&c, &t.name, &xml),
        Err(e) => c.ui().global::<TemplateForm>().set_error(e.to_string().into()),
    });
}

pub fn refresh(ctx: &Rc<Ctx>) {
    match ctx.with_conn(templates::list) {
        Ok(list) => ctx
            .ui()
            .global::<TemplatesState>()
            .set_rows(model(list.iter().map(template_row).collect())),
        Err(e) => ctx.fail(&e),
    }
}

fn fill_form(f: &TemplateForm, t: &InvoiceTemplate) {
    let s = &t.settings;
    f.set_name(t.name.as_str().into());
    f.set_business_name(t.business.name.as_str().into());
    f.set_address(t.business.address.as_str().into());
    f.set_phone(t.business.phone.as_str().into());
    f.set_email(t.business.email.as_str().into());
    f.set_website(t.business.website.as_str().into());
    f.set_tax_id(t.business.tax_id.as_str().into());
    f.set_title(s.title.as_str().into());
    f.set_currency_code(s.currency_code.as_str().into());
    f.set_currency_symbol(s.currency_symbol.as_str().into());
    f.set_prefix(s.invoice_prefix.as_str().into());
    f.set_tax_label(s.tax_label.as_str().into());
    f.set_tax_rate(format_percent(s.default_tax_rate).into());
    f.set_terms_days(s.payment_terms_days.to_string().into());
    f.set_accent(s.accent_color.as_str().into());
    if let Some(c) = hex_color(&s.accent_color) {
        f.set_accent_preview(c);
    }
    f.set_notes(t.footer.notes.as_str().into());
    f.set_terms(t.footer.terms.as_str().into());
    f.set_payment(t.footer.payment_instructions.as_str().into());
}

fn template_from_form(f: &TemplateForm) -> Result<InvoiceTemplate> {
    let days_text = f.get_terms_days();
    let days = if days_text.trim().is_empty() {
        0
    } else {
        days_text.trim().parse::<u32>().map_err(|_| {
            CoreError::Validation("Payment terms must be a whole number of days.".into())
        })?
    };
    let t = InvoiceTemplate {
        version: templates::FORMAT_VERSION,
        name: f.get_name().trim().to_string(),
        business: Business {
            name: f.get_business_name().trim().to_string(),
            address: f.get_address().trim().to_string(),
            phone: f.get_phone().trim().to_string(),
            email: f.get_email().trim().to_string(),
            website: f.get_website().trim().to_string(),
            tax_id: f.get_tax_id().trim().to_string(),
        },
        settings: TemplateSettings {
            title: f.get_title().trim().to_string(),
            currency_code: f.get_currency_code().trim().to_string(),
            currency_symbol: f.get_currency_symbol().trim().to_string(),
            invoice_prefix: f.get_prefix().trim().to_string(),
            tax_label: f.get_tax_label().trim().to_string(),
            default_tax_rate: parse_percent(&f.get_tax_rate())?,
            payment_terms_days: days,
            accent_color: f.get_accent().trim().to_uppercase(),
        },
        footer: Footer {
            notes: f.get_notes().trim().to_string(),
            terms: f.get_terms().trim().to_string(),
            payment_instructions: f.get_payment().trim().to_string(),
        },
    };
    t.validate()?;
    Ok(t)
}

/// The template being edited, validated, plus its XML (from whichever tab is active).
fn current_xml(ctx: &Ctx) -> Result<(InvoiceTemplate, String)> {
    let ui_ = ctx.ui();
    let f = ui_.global::<TemplateForm>();
    if f.get_tab() == "xml" {
        let xml = f.get_xml().to_string();
        let t = InvoiceTemplate::from_xml(&xml)?;
        Ok((t, xml))
    } else {
        let t = template_from_form(&f)?;
        let xml = t.to_xml()?;
        Ok((t, xml))
    }
}

fn open_editor(ctx: &Rc<Ctx>, id: i64, t: &InvoiceTemplate) {
    let ui = ctx.ui();
    let f = ui.global::<TemplateForm>();
    f.set_id(id as i32);
    f.set_tab("form".into());
    f.set_error(SharedString::new());
    f.set_xml(t.to_xml().unwrap_or_default().into());
    fill_form(&f, t);
    f.set_generation(f.get_generation() + 1);
    ui.global::<AppState>().set_page("template-edit".into());
}

fn switch_tab(ctx: &Rc<Ctx>, tab: &str) {
    let ui_ = ctx.ui();
    let f = ui_.global::<TemplateForm>();
    if f.get_tab() == tab {
        return;
    }
    // Carry edits across tabs; refuse to switch while the source is invalid
    // so nothing the user typed is silently lost.
    match current_xml(ctx) {
        Ok((t, xml)) => {
            if tab == "xml" {
                f.set_xml(xml.into());
            } else {
                fill_form(&f, &t);
            }
            f.set_error(SharedString::new());
            f.set_tab(tab.into());
        }
        Err(e) => f.set_error(format!("Fix this before switching: {e}").into()),
    }
}

fn save(ctx: &Rc<Ctx>) {
    let ui_ = ctx.ui();
    let f = ui_.global::<TemplateForm>();
    let id = f.get_id() as i64;
    let result = current_xml(ctx).and_then(|(_, xml)| {
        ctx.with_conn(|c| if id == 0 { templates::create(c, &xml).map(|_| ()) } else { templates::update(c, id, &xml) })
    });
    match result {
        Ok(()) => {
            ctx.success("Template saved");
            nav::navigate(ctx, "templates");
        }
        Err(e) => f.set_error(e.to_string().into()),
    }
}

fn safe_file_stem(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '-' })
        .collect();
    let s = s.trim_matches('-').to_string();
    if s.is_empty() { "template".into() } else { s }
}

fn export(ctx: &Ctx, name: &str, xml: &str) {
    let file = format!("{}.xml", safe_file_stem(name));
    let Some(path) = files::save_dialog("Export template", &file, XML_FILTER) else { return };
    match std::fs::write(&path, xml) {
        Ok(()) => ctx.success(format!("Exported {}", files::display_name(&path))),
        Err(e) => ctx.toast("error", format!("Couldn't save the file: {e}")),
    }
}

fn import(ctx: &Rc<Ctx>) {
    let Some(path) = files::open_dialog("Import template", XML_FILTER) else { return };
    let xml = match std::fs::read_to_string(&path) {
        Ok(x) => x,
        Err(e) => return ctx.toast("error", format!("Couldn't read the file: {e}")),
    };
    match ctx.with_conn(|c| templates::create(c, &xml)) {
        Ok(_) => {
            ctx.success(format!("Imported {}", files::display_name(&path)));
            refresh(ctx);
        }
        Err(e) => ctx.fail(&e),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn file_stems() {
        assert_eq!(super::safe_file_stem("Retail / Wholesale"), "Retail---Wholesale");
        assert_eq!(super::safe_file_stem("///"), "template");
    }
}
