//! Embeds the app icon into aitelier-editor.exe on Windows (shown in Explorer, taskbar and shortcuts).

fn main() {
    println!("cargo:rerun-if-changed=assets/aitelier.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/aitelier.ico");
        res.set("ProductName", "AItelier");
        res.set("FileDescription", "AItelier editor");
        if let Err(e) = res.compile() {
            // Not fatal: the program works without an embedded icon.
            println!("cargo:warning=could not embed the icon: {e}");
        }
    }
}
