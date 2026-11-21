#[cfg(windows)]
fn main() -> std::io::Result<()> {
    let icon = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/tracking.ico");
    println!("cargo:rerun-if-changed={icon}");
    winresource::WindowsResource::new()
        .set_icon(icon)
        .set("ProductName", "Tracking")
        .set("FileDescription", "Tracking")
        .set("OriginalFilename", "tracking-desktop.exe")
        .compile()
}

#[cfg(not(windows))]
fn main() {}
