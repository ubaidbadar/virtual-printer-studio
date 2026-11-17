fn main() {
    #[cfg(windows)]
    {
        winresource::WindowsResource::new()
            .set_icon("assets/logo.ico")
            .set("ProductName", "Virtual Printer Studio")
            .set("FileDescription", "Virtual Printer Studio")
            .compile()
            .expect("embed Windows application icon");
    }
    slint_build::compile_with_config(
        "ui/app.slint",
        slint_build::CompilerConfiguration::new()
            .with_style("fluent".into())
            .with_debug_info(std::env::var("PROFILE").is_ok_and(|profile| profile == "debug")),
    )
    .expect("compile desktop UI");
}
