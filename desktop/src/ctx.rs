//! Shared state for all UI controllers.

use app_core::orders::OrderStatus;
use app_core::{App, CoreError, templates};
use slint::{ComponentHandle, Timer, TimerMode, VecModel, Weak};
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use crate::{AppState, ItemRow, MainWindow};

#[derive(Default)]
pub struct ItemDraft {
    pub description: String,
    pub quantity: String,
    pub price: String,
}

#[derive(Default)]
pub struct OrderDraft {
    pub id: Option<i64>,
    pub items: Vec<ItemDraft>,
    pub symbol: String,
}

type ConfirmAction = Box<dyn FnOnce(&Rc<Ctx>)>;

pub struct Ctx {
    pub app: RefCell<App>,
    pub ui: Weak<MainWindow>,
    pub data_dir: PathBuf,

    // Auth flow (kept only in memory, cleared on every transition)
    pub pin: RefCell<String>,
    pub first_pin: RefCell<String>,
    pub reset_key: RefCell<String>,

    // Order editor
    pub draft: RefCell<OrderDraft>,
    pub items_model: Rc<VecModel<ItemRow>>,
    pub customer_ids: RefCell<Vec<i64>>,
    pub template_ids: RefCell<Vec<i64>>,

    pub report_customer_ids: RefCell<Vec<i64>>,
    pub pending_confirm: RefCell<Option<ConfirmAction>>,
    pub toast_timer: Timer,
    pub clipboard: RefCell<Option<arboard::Clipboard>>,
}

impl Ctx {
    pub fn new(app: App, ui: &MainWindow, data_dir: PathBuf) -> Rc<Self> {
        Rc::new(Self {
            app: RefCell::new(app),
            ui: ui.as_weak(),
            data_dir,
            pin: RefCell::default(),
            first_pin: RefCell::default(),
            reset_key: RefCell::default(),
            draft: RefCell::default(),
            items_model: Rc::new(VecModel::default()),
            customer_ids: RefCell::default(),
            template_ids: RefCell::default(),
            report_customer_ids: RefCell::default(),
            pending_confirm: RefCell::default(),
            toast_timer: Timer::default(),
            clipboard: RefCell::default(),
        })
    }

    pub fn ui(&self) -> MainWindow {
        self.ui
            .upgrade()
            .expect("main window is alive while callbacks run")
    }

    /// Runs `f` with the database connection.
    pub fn with_conn<T>(&self, f: impl FnOnce(&app_core::rusqlite::Connection) -> T) -> T {
        f(self.app.borrow().conn())
    }

    /// Currency symbol of the default template, used for listings.
    pub fn symbol(&self) -> String {
        self.with_conn(|c| templates::get_default(c).map(|t| t.template.settings.currency_symbol))
            .unwrap_or_else(|_| "$".into())
    }

    pub fn toast(&self, kind: &str, text: impl Into<String>) {
        let ui = self.ui();
        let state = ui.global::<AppState>();
        state.set_toast_text(text.into().into());
        state.set_toast_kind(kind.into());
        state.set_toast_visible(true);
        let weak = self.ui.clone();
        let secs = if kind == "error" { 7 } else { 4 };
        self.toast_timer.start(
            TimerMode::SingleShot,
            Duration::from_secs(secs),
            move || {
                if let Some(ui) = weak.upgrade() {
                    ui.global::<AppState>().set_toast_visible(false);
                }
            },
        );
    }

    pub fn success(&self, text: impl Into<String>) {
        self.toast("success", text);
    }

    pub fn fail(&self, err: &CoreError) {
        self.toast("error", err.to_string());
    }

    pub fn confirm(
        &self,
        title: &str,
        message: &str,
        action_label: &str,
        danger: bool,
        action: impl FnOnce(&Rc<Ctx>) + 'static,
    ) {
        let ui = self.ui();
        let s = ui.global::<AppState>();
        s.set_confirm_title(title.into());
        s.set_confirm_message(message.into());
        s.set_confirm_action_label(action_label.into());
        s.set_confirm_danger(danger);
        s.set_confirm_visible(true);
        *self.pending_confirm.borrow_mut() = Some(Box::new(action));
    }

    pub fn copy_text(&self, text: &str) {
        let mut slot = self.clipboard.borrow_mut();
        if slot.is_none() {
            *slot = arboard::Clipboard::new().ok();
        }
        match slot.as_mut().map(|c| c.set_text(text.to_string())) {
            Some(Ok(())) => self.success("Copied to clipboard"),
            _ => self.toast("error", "Couldn't access the clipboard."),
        }
    }
}

pub fn status_index(s: OrderStatus) -> i32 {
    OrderStatus::ALL.iter().position(|x| *x == s).unwrap_or(0) as i32
}

pub fn status_from_index(i: i32) -> OrderStatus {
    OrderStatus::ALL
        .get(i.max(0) as usize)
        .copied()
        .unwrap_or(OrderStatus::Draft)
}
