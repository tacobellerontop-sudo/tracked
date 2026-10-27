//! Embed the app icon in tracked.exe on Windows (taskbar, Explorer, shortcuts).

fn main() {
    println!("cargo:rerun-if-changed=assets/icon/tracked.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icon/tracked.ico");
        if let Err(e) = res.compile() {
            // A missing resource compiler shouldn't break the build; the exe
            // just keeps the default icon.
            println!("cargo:warning=couldn't embed the app icon: {e}");
        }
    }
}
