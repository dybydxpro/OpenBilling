fn main() {
    println!("Starting {}", app_core::app_name());

    let user = app_core::services::create_user(1, "Desktop User".to_string());

    println!("User: {:?}", user);
}
