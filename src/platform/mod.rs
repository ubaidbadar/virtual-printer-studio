use anyhow::Result;
use std::path::{Path, PathBuf};
use vps_core::Printer;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::*;
#[cfg(not(windows))]
pub fn native_action(_action: &str, _printer: &Printer, _port: u16) -> Result<()> {
    anyhow::bail!(
        "Windows printer installation is available on Windows 10/11. Profiles and PDF delivery can be used here."
    )
}
#[cfg(not(windows))]
pub fn print_test(_printer: &Printer) -> Result<()> {
    anyhow::bail!("Windows is required for a system print test.")
}
#[cfg(not(windows))]
pub fn startup(_enabled: bool) -> Result<()> {
    anyhow::bail!("Start with Windows is available on Windows.")
}
#[cfg(not(windows))]
pub fn system_dark() -> bool {
    false
}
#[cfg(not(windows))]
pub fn notify_saved(_path: &Path) -> Result<()> {
    Ok(())
}
pub fn spool_root(data: &Path) -> PathBuf {
    data.join("capture")
}
#[cfg(not(windows))]
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
    #[cfg(not(windows))]
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

#[cfg(not(windows))]
pub fn capture_title(_id: &str) -> Result<Option<String>> {
    Ok(None)
}
