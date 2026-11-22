use anyhow::Result;
use std::path::{Path, PathBuf};
use vps_core::Printer;
#[derive(Clone, Default, serde::Deserialize)]
pub struct PrinterHealth {
    pub write: bool,
    pub service: bool,
    pub free_bytes: Option<u64>,
}
#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::*;
#[cfg(not(any(windows, target_os = "macos")))]
pub fn native_action(_action: &str, _printer: &Printer, _port: u16, _data: &Path) -> Result<()> {
    anyhow::bail!(
        "Windows printer installation is available on Windows 10/11. Profiles and PDF delivery can be used here."
    )
}
#[cfg(not(any(windows, target_os = "macos")))]
pub fn print_test(_printer: &Printer) -> Result<()> {
    anyhow::bail!("Windows is required for a system print test.")
}
#[cfg(not(any(windows, target_os = "macos")))]
pub fn startup(_enabled: bool) -> Result<()> {
    anyhow::bail!("Start with Windows is available on Windows.")
}
#[cfg(not(any(windows, target_os = "macos")))]
pub fn system_dark() -> bool {
    false
}
#[cfg(not(any(windows, target_os = "macos")))]
pub fn notify_saved(_path: &Path, _printer: &str) -> Result<()> {
    Ok(())
}
pub fn spool_root(data: &Path) -> PathBuf {
    data.join("capture")
}
#[cfg(not(any(windows, target_os = "macos")))]
pub fn authorize_capture(_stream: &std::net::TcpStream) -> Result<()> {
    anyhow::bail!("Windows spooler authentication is unavailable on this platform")
}
pub fn show_in_folder(path: &Path) -> Result<()> {
    #[cfg(windows)]
    {
        std::process::Command::new("explorer.exe")
            .arg(format!("/select,{}", path.display()))
            .spawn()?;
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("/usr/bin/open")
            .arg("-R")
            .arg(path)
            .spawn()?;
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        open::that(path.parent().unwrap_or(path))?;
    }
    Ok(())
}
pub fn log(path: &Path, message: &str) {
    use std::io::Write;
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if std::fs::metadata(path).is_ok_and(|meta| meta.len() > 2_000_000) {
        let rotated = path.with_extension("log.1");
        let _ = std::fs::remove_file(&rotated);
        let _ = std::fs::rename(path, rotated);
    }
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(path)
    {
        let _ = writeln!(file, "{} {message}", time::OffsetDateTime::now_utc());
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
pub fn capture_title(_id: &str) -> Result<Option<String>> {
    Ok(None)
}

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::*;
pub const NATIVE_AVAILABLE: bool = cfg!(any(windows, target_os = "macos"));

#[cfg(target_os = "macos")]
pub const CAPTURE_BACKEND: &str = "macOS CUPS PDF (authenticated Unix socket)";
#[cfg(not(target_os = "macos"))]
pub const CAPTURE_BACKEND: &str = vps_core::CAPTURE_BACKEND;
