fn main() {
    let config = slint_build::CompilerConfiguration::new().with_style("fluent".into());
    slint_build::compile_with_config("ui/app.slint", config).expect("Slint UI failed to compile");

    // Give the Windows .exe its icon and name (shown in Explorer and the taskbar).
    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("../../packaging/windows/app.ico");
        res.set("ProductName", "EV Showroom POS");
        res.set("FileDescription", "EV Showroom POS");
        res.set("CompanyName", "EV Showroom POS");
        res.compile().expect("could not add the Windows icon");
    }
}
