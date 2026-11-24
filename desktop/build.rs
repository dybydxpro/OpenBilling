fn main() {
    // Embed the app icon and version info in the Windows executable.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut resource = winres::WindowsResource::new();
        resource
            .set_icon("assets/app.ico")
            .set("ProductName", "OpenBilling")
            .set("FileDescription", "OpenBilling")
            .set("InternalName", "OpenBilling")
            .set("OriginalFilename", "OpenBilling.exe");
        resource
            .compile()
            .expect("Failed to compile Windows resources");
    }
}
