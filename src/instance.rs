#[cfg(windows)]
use anyhow::Context;
use anyhow::Result;
#[cfg(windows)]
use slint::ComponentHandle;
use std::{
    fs::{File, OpenOptions},
    path::Path,
};
pub struct Instance {
    _lock: File,
    #[cfg(windows)]
    event: std::sync::Arc<Event>,
    #[cfg(windows)]
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    #[cfg(windows)]
    join: Option<std::thread::JoinHandle<()>>,
}
#[cfg(windows)]
struct Event(usize);
#[cfg(windows)]
impl Drop for Event {
    fn drop(&mut self) {
        unsafe { windows_sys::Win32::Foundation::CloseHandle(self.0 as _) };
    }
}
impl Instance {
    pub fn acquire(data: &Path, ui: &crate::AppWindow) -> Result<Option<Self>> {
        std::fs::create_dir_all(data)?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(data.join("app.lock"))?;
        #[cfg(windows)]
        let event = {
            use sha2::{Digest, Sha256};
            use windows_sys::Win32::System::Threading::CreateEventW;
            let hash = Sha256::digest(data.to_string_lossy().as_bytes());
            let key: String = hash.iter().take(16).map(|b| format!("{b:02x}")).collect();
            let name: Vec<u16> = format!("Local\\VirtualPrinterStudio-{key}")
                .encode_utf16()
                .chain(Some(0))
                .collect();
            // Safety: the null-terminated event name lives through CreateEventW.
            let raw = unsafe { CreateEventW(std::ptr::null(), 0, 0, name.as_ptr()) };
            anyhow::ensure!(
                !raw.is_null(),
                "Could not create the application activation event"
            );
            std::sync::Arc::new(Event(raw as usize))
        };
        if file.try_lock().is_err() {
            #[cfg(windows)]
            unsafe {
                windows_sys::Win32::System::Threading::SetEvent(event.0 as _)
            };
            #[cfg(not(windows))]
            rfd::MessageDialog::new()
                .set_title("Virtual Printer Studio")
                .set_description("Virtual Printer Studio is already running.")
                .show();
            return Ok(None);
        }
        #[cfg(windows)]
        {
            use std::sync::{
                Arc,
                atomic::{AtomicBool, Ordering},
            };
            use windows_sys::Win32::System::Threading::{INFINITE, WaitForSingleObject};
            let stop = Arc::new(AtomicBool::new(false));
            let child_stop = stop.clone();
            let child_event = event.clone();
            let weak = ui.as_weak();
            let join = std::thread::Builder::new()
                .name("app-activation".into())
                .spawn(move || {
                    loop {
                        // Safety: Arc keeps the event alive until this wait completes.
                        if unsafe { WaitForSingleObject(child_event.0 as _, INFINITE) } != 0
                            || child_stop.load(Ordering::Acquire)
                        {
                            break;
                        }
                        let _ = weak.upgrade_in_event_loop(|ui| {
                            let _ = ui.show();
                        });
                    }
                })
                .context("Could not start activation listener")?;
            Ok(Some(Self {
                _lock: file,
                event,
                stop,
                join: Some(join),
            }))
        }
        #[cfg(not(windows))]
        {
            let _ = ui;
            Ok(Some(Self { _lock: file }))
        }
    }
}
#[cfg(windows)]
impl Drop for Instance {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Release);
        unsafe { windows_sys::Win32::System::Threading::SetEvent(self.event.0 as _) };
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}
