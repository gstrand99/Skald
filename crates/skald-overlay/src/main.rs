#[cfg(target_os = "linux")]
mod linux;

#[cfg(target_os = "linux")]
fn main() -> anyhow::Result<()> {
    linux::run()
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("skald-overlay is provided by the native Skald app on this platform");
}
