fn main() {
    // `cfg(webkitgtk)`: platforms where Tauri renders with WebKitGTK and plays media
    // through GStreamer (Linux and the BSDs). Code that works around WebKitGTK uses
    // this instead of listing operating systems in several places.
    println!("cargo::rustc-check-cfg=cfg(webkitgtk)");
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if matches!(os.as_str(), "linux" | "freebsd" | "openbsd" | "netbsd" | "dragonfly") {
        println!("cargo::rustc-cfg=webkitgtk");
    }
    // The day of the build, for the security check in Settings (updates.rs).
    // SOURCE_DATE_EPOCH, when set, keeps builds reproducible.
    println!("cargo::rerun-if-env-changed=SOURCE_DATE_EPOCH");
    println!("cargo::rustc-env=TIDINGS_BUILD_DATE={}", build_date());
    tauri_build::build()
}

/// Today (or SOURCE_DATE_EPOCH) as YYYY-MM-DD, in UTC.
fn build_date() -> String {
    let secs = std::env::var("SOURCE_DATE_EPOCH")
        .ok()
        .and_then(|s| s.trim().parse::<i64>().ok())
        .unwrap_or_else(|| {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0)
        });
    // Days since 1970-01-01 to a calendar date (Howard Hinnant's civil_from_days).
    let z = secs.div_euclid(86_400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}
