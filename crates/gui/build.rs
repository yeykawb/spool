//! Embeds the icon in spool.exe so Explorer and the taskbar show it.

fn main() {
    println!("cargo:rerun-if-changed=assets/spool.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/spool.ico");
        res.compile().expect("failed to embed the app icon");
    }
}
