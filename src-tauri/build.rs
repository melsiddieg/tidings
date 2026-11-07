fn main() {
    // `cfg(webkitgtk)`: platforms where Tauri renders with WebKitGTK and plays media
    // through GStreamer (Linux and the BSDs). Code that works around WebKitGTK uses
    // this instead of listing operating systems in several places.
    println!("cargo::rustc-check-cfg=cfg(webkitgtk)");
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if matches!(os.as_str(), "linux" | "freebsd" | "openbsd" | "netbsd" | "dragonfly") {
        println!("cargo::rustc-cfg=webkitgtk");
    }
    tauri_build::build()
}
