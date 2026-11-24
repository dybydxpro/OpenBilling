//! PIN setup, sign-in and recovery-key reset.

use app_core::auth::{self, PIN_LENGTH};
use slint::{ComponentHandle, SharedString, Timer};
use std::rc::Rc;
use std::time::Duration;

use crate::convert::model;
use crate::ctx::Ctx;
use crate::{Api, AppState, AuthState, nav};

pub fn init(ctx: &Rc<Ctx>) {
    let ui = ctx.ui();
    let api = ui.global::<Api>();

    let c = ctx.clone();
    api.on_pin_key(move |key| pin_key(&c, key.as_str()));
    let c = ctx.clone();
    api.on_auth_continue(move || auth_continue(&c));
    let c = ctx.clone();
    api.on_auth_forgot(move || set_mode(&c, "reset-key"));
    let c = ctx.clone();
    api.on_auth_back(move || {
        let mode = c.ui().global::<AuthState>().get_mode();
        let back = match mode.as_str() {
            "setup-confirm" => "setup-pin",
            "reset-confirm" => "reset-pin",
            _ => "login",
        };
        set_mode(&c, back);
    });
    let c = ctx.clone();
    api.on_lock(move || lock(&c));

    show_auth(ctx);
}

/// Shows the sign-in screen (or first-run setup when no PIN exists).
pub fn show_auth(ctx: &Rc<Ctx>) {
    let initialized = ctx.with_conn(auth::is_initialized).unwrap_or(false);
    ctx.ui().global::<AppState>().set_screen("auth".into());
    set_mode(ctx, if initialized { "login" } else { "setup-pin" });
}

pub fn lock(ctx: &Rc<Ctx>) {
    // Drop anything sensitive that may still be on screen.
    let ui = ctx.ui();
    ui.global::<AppState>().set_confirm_visible(false);
    ui.global::<crate::CustomersState>()
        .set_dialog_visible(false);
    ui.global::<crate::OrderView>().set_generate_visible(false);
    crate::settings_ui::clear_sensitive(ctx);
    show_auth(ctx);
}

fn set_mode(ctx: &Rc<Ctx>, mode: &str) {
    let ui_ = ctx.ui();
    let s = ui_.global::<AuthState>();
    s.set_mode(mode.into());
    s.set_error(SharedString::new());
    s.set_busy(false);
    if mode != "reset-key" {
        s.set_key_input(SharedString::new());
    }
    if !matches!(mode, "setup-key" | "reset-done") {
        s.set_recovery_key(SharedString::new());
        s.set_key_saved(false);
    }
    if matches!(mode, "login" | "setup-pin" | "reset-key") {
        ctx.first_pin.borrow_mut().clear();
    }
    if matches!(mode, "login" | "setup-pin" | "setup-confirm") {
        ctx.reset_key.borrow_mut().clear();
    }
    clear_pin(ctx);
}

fn clear_pin(ctx: &Ctx) {
    ctx.pin.borrow_mut().clear();
    sync_pin(ctx);
}

fn sync_pin(ctx: &Ctx) {
    let pin = ctx.pin.borrow();
    let digits: Vec<SharedString> = (0..PIN_LENGTH)
        .map(|i| pin.get(i..i + 1).unwrap_or("").into())
        .collect();
    let ui_ = ctx.ui();
    let s = ui_.global::<AuthState>();
    s.set_pin_len(pin.len() as i32);
    s.set_pin_digits(model(digits));
}

fn show_error(ctx: &Ctx, msg: impl Into<SharedString>) {
    let ui_ = ctx.ui();
    let s = ui_.global::<AuthState>();
    s.set_error(msg.into());
    s.set_busy(false);
    clear_pin(ctx);
}

fn pin_key(ctx: &Rc<Ctx>, key: &str) {
    let ui_ = ctx.ui();
    let s = ui_.global::<AuthState>();
    if s.get_busy() {
        return;
    }
    {
        let mut pin = ctx.pin.borrow_mut();
        match key {
            "back" => {
                pin.pop();
            }
            "clear" => pin.clear(),
            k if k.len() == 1 && k.as_bytes()[0].is_ascii_digit() => {
                if pin.len() < PIN_LENGTH {
                    pin.push_str(k);
                }
            }
            _ => return,
        }
    }
    if !ctx.pin.borrow().is_empty() {
        s.set_error(SharedString::new());
    }
    sync_pin(ctx);

    if ctx.pin.borrow().len() == PIN_LENGTH {
        s.set_busy(true);
        // Let the last dot paint before the (deliberately slow) hash check.
        let c = ctx.clone();
        Timer::single_shot(Duration::from_millis(120), move || submit_pin(&c));
    }
}

fn submit_pin(ctx: &Rc<Ctx>) {
    let ui = ctx.ui();
    let s = ui.global::<AuthState>();
    let mode = s.get_mode();
    let pin = ctx.pin.borrow().clone();

    match mode.as_str() {
        "setup-pin" | "reset-pin" => {
            if let Err(e) = auth::validate_new_pin(&pin, &pin) {
                return show_error(ctx, e.to_string());
            }
            *ctx.first_pin.borrow_mut() = pin;
            let next = if mode == "setup-pin" {
                "setup-confirm"
            } else {
                "reset-confirm"
            };
            set_mode(ctx, next);
        }
        "setup-confirm" | "reset-confirm" => {
            let first = ctx.first_pin.borrow().clone();
            if first != pin {
                set_mode(
                    ctx,
                    if mode == "setup-confirm" {
                        "setup-pin"
                    } else {
                        "reset-pin"
                    },
                );
                return show_error(ctx, "The PINs didn't match. Please choose your PIN again.");
            }
            let result = if mode == "setup-confirm" {
                ctx.with_conn(|c| auth::setup(c, &first, &pin))
            } else {
                let key = ctx.reset_key.borrow().clone();
                ctx.with_conn(|c| auth::reset_with_recovery_key(c, &key, &first, &pin))
            };
            match result {
                Ok(recovery_key) => {
                    let done = if mode == "setup-confirm" {
                        "setup-key"
                    } else {
                        "reset-done"
                    };
                    set_mode(ctx, done);
                    s.set_recovery_key(recovery_key.into());
                    ctx.first_pin.borrow_mut().clear();
                    ctx.reset_key.borrow_mut().clear();
                }
                Err(e) => {
                    set_mode(
                        ctx,
                        if mode == "setup-confirm" {
                            "setup-pin"
                        } else {
                            "reset-key"
                        },
                    );
                    show_error(ctx, e.to_string());
                }
            }
        }
        "login" => match ctx.with_conn(|c| auth::verify_pin(c, &pin)) {
            Ok(()) => enter_app(ctx),
            Err(e) => show_error(ctx, e.to_string()),
        },
        _ => clear_pin(ctx),
    }
}

fn auth_continue(ctx: &Rc<Ctx>) {
    let ui_ = ctx.ui();
    let s = ui_.global::<AuthState>();
    match s.get_mode().as_str() {
        "setup-key" | "reset-done" if s.get_key_saved() => enter_app(ctx),
        "reset-key" => {
            let key = s.get_key_input().to_string();
            s.set_busy(true);
            let c = ctx.clone();
            Timer::single_shot(Duration::from_millis(60), move || {
                match c.with_conn(|conn| auth::verify_recovery_key(conn, &key)) {
                    Ok(()) => {
                        *c.reset_key.borrow_mut() = key;
                        set_mode(&c, "reset-pin");
                    }
                    Err(e) => {
                        let ui_ = c.ui();
                        let s = ui_.global::<AuthState>();
                        s.set_busy(false);
                        s.set_error(e.to_string().into());
                    }
                }
            });
        }
        _ => {}
    }
}

fn enter_app(ctx: &Rc<Ctx>) {
    set_mode(ctx, "login");
    let ui = ctx.ui();
    ui.global::<AppState>().set_screen("app".into());
    nav::navigate(ctx, "dashboard");
}
