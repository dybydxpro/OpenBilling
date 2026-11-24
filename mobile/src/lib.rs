//! Mobile (Android / iOS) entry point. Business logic lives in `app-core`;
//! the Slint UI wiring is still to be shared with the desktop crate.

pub fn mobile_app_name() -> &'static str {
    app_core::app_name()
}
