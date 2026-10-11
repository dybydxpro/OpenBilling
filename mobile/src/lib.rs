use app_core::models::User;

pub fn mobile_app_name() -> &'static str {
    app_core::app_name()
}

pub fn create_mobile_user(id: u64, name: String) -> User {
    app_core::services::create_user(id, name)
}
