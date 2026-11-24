fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut resource = winres::WindowsResource::new();

        resource
            .set_icon("assets/app.ico")
            .set("ProductName", "Open Billing")
            .set("FileDescription", "Open Billing")
            .set("InternalName", "OpenBilling")
            .set("OriginalFilename", "OpenBilling.exe");

        resource
            .compile()
            .expect("Failed to compile Windows resources");
    }

    let config = slint_build::CompilerConfiguration::new().with_style("fluent".into());

    slint_build::compile_with_config("ui/windows/MainWindow.slint", config)
        .expect("Slint build failed");
}
