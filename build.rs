fn main() {
    println!("cargo:rerun-if-changed=assets/app.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        winresource::WindowsResource::new()
            .set_icon("assets/app.ico")
            .set("ProductName", "stream-dlp")
            .set("FileDescription", "stream-dlp video downloader")
            .compile()
            .expect("compile Windows app icon and version resource");
    }
}
