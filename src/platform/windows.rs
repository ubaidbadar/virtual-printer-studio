use super::*;
use anyhow::ensure;
use base64::{Engine, engine::general_purpose::STANDARD};
use std::{
    fs::OpenOptions,
    io::{Read, Write},
    os::windows::{fs::OpenOptionsExt, process::CommandExt},
    process::Command,
};
use winreg::{RegKey, enums::*};
fn powershell(script: &str) -> Result<std::process::Output> {
    let utf16: Vec<u8> = format!("$ProgressPreference='SilentlyContinue';{script}")
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    Ok(Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-EncodedCommand",
            &STANDARD.encode(utf16),
        ])
        .creation_flags(0x08000000)
        .output()?)
}
fn literal(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}
fn prepare_installer(path: &Path, script: &str) -> Result<std::fs::File> {
    let mut writer = OpenOptions::new()
        .create_new(true)
        .read(true)
        .write(true)
        .share_mode(1)
        .open(path)?;
    writer.write_all(script.as_bytes())?;
    writer.sync_all()?;
    drop(writer);
    let mut locked = OpenOptions::new().read(true).share_mode(1).open(path)?;
    let mut actual = Vec::new();
    locked.read_to_end(&mut actual)?;
    ensure!(
        actual == script.as_bytes(),
        "The installer script changed before administrator approval"
    );
    Ok(locked)
}
pub fn native_action(action: &str, printer: &Printer, port: u16) -> Result<()> {
    ensure!(
        ["ensure", "pause", "remove"].contains(&action),
        "Unknown Windows printer action"
    );
    ensure!(vps_core::valid_id(&printer.id), "Invalid printer identity");
    let identity = powershell("[Security.Principal.WindowsIdentity]::GetCurrent().User.Value")?;
    ensure!(
        identity.status.success(),
        "Could not determine your Windows account"
    );
    let sid = String::from_utf8_lossy(&identity.stdout).trim().to_owned();
    let temp = tempfile::tempdir()?;
    let script_path = temp.path().join("configure-printer.ps1");
    let result_path = temp.path().join("result.json");
    let request = serde_json::json!({"action":action,"id":printer.id,"name":printer.name,"enabled":printer.enabled,"sid":sid,"port":port,"queue":vps_core::lpr_queue(&printer.id)?,"result":result_path});
    let script = include_str!("native.ps1").replace(
        "__REQUEST__",
        &STANDARD.encode(serde_json::to_vec(&request)?),
    );
    // A write handle prevents Windows PowerShell's source reader from opening
    // the script. Reopen read-only, verify under the lock, and retain that handle
    // until elevation finishes so the checked script cannot be changed.
    let _locked = prepare_installer(&script_path, &script)?;
    let arguments = format!(
        "-NoProfile -NonInteractive -ExecutionPolicy Bypass -File \"{}\"",
        script_path.display()
    );
    let wrapper = format!(
        "$ErrorActionPreference='Stop'; try {{$p=Start-Process -FilePath powershell.exe -Verb RunAs -ArgumentList {} -Wait -PassThru; exit $p.ExitCode}} catch {{Write-Error $_; exit 1}}",
        literal(&arguments)
    );
    let output = powershell(&wrapper)?;
    if result_path.exists() {
        let bytes = std::fs::read(&result_path)?;
        // Windows PowerShell emits a UTF-8 BOM with Set-Content -Encoding UTF8.
        let value: serde_json::Value =
            serde_json::from_slice(bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&bytes))?;
        ensure!(
            value["ok"] == true,
            "{}",
            value["error"]
                .as_str()
                .unwrap_or("Windows could not configure the printer")
        );
    } else {
        anyhow::bail!(
            "Windows printer setup returned no confirmation. It may have been cancelled: {} {}",
            String::from_utf8_lossy(&output.stdout).trim(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}
pub fn print_test(printer: &Printer) -> Result<()> {
    let encoded = STANDARD.encode(printer.name.as_bytes());
    let script = format!(
        r#"$ErrorActionPreference='Stop'; Add-Type -AssemblyName System.Drawing; $name=[Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('{encoded}')); $doc=New-Object Drawing.Printing.PrintDocument; $doc.PrinterSettings.PrinterName=$name; if (!$doc.PrinterSettings.IsValid){{throw 'The Windows printer is unavailable. Install or repair it first.'}}; $doc.PrintController=New-Object Drawing.Printing.StandardPrintController; $doc.DocumentName='Virtual Printer Studio test page'; $doc.add_PrintPage({{param($sender,$e) $font=New-Object Drawing.Font('Segoe UI',20); try{{$e.Graphics.DrawString('Virtual Printer Studio',$font,[Drawing.Brushes]::Black,72,72);$e.Graphics.DrawString('Your virtual printer is working.',$font,[Drawing.Brushes]::Black,72,130)}}finally{{$font.Dispose()}}; $e.HasMorePages=$false}}); try{{$doc.Print()}}finally{{$doc.Dispose()}}"#
    );
    let output = powershell(&script)?;
    ensure!(
        output.status.success(),
        "Print test failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}
pub fn startup(enabled: bool) -> Result<()> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (key, _) = hkcu.create_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Run")?;
    if enabled {
        key.set_value(
            "VirtualPrinterStudio",
            &format!("\"{}\" --background", std::env::current_exe()?.display()),
        )?;
    } else {
        match key.delete_value("VirtualPrinterStudio") {
            Ok(()) => (),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}
pub fn system_dark() -> bool {
    RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize")
        .and_then(|k| k.get_value::<u32, _>("AppsUseLightTheme"))
        .map(|v| v == 0)
        .unwrap_or(false)
}
fn xml(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
pub fn notify_saved(path: &Path) -> Result<()> {
    use ::windows::{
        Data::Xml::Dom::XmlDocument,
        Foundation::TypedEventHandler,
        UI::Notifications::{ToastActivatedEventArgs, ToastNotification, ToastNotificationManager},
        Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx},
        core::{HSTRING, Interface},
    };
    // Safety: the delivery worker is a dedicated thread; COM initialization is
    // idempotent for its lifetime. No UI object crosses into this worker.
    thread_local! { static COM: () = { let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }; }; }
    COM.with(|_| {});
    register_notification_shortcut()?;
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (key, _) = hkcu.create_subkey("Software\\Classes\\AppUserModelId\\VirtualPrinterStudio")?;
    key.set_value("DisplayName", &"Virtual Printer Studio")?;
    let doc = XmlDocument::new()?;
    doc.LoadXml(&HSTRING::from(format!("<toast><visual><binding template=\"ToastGeneric\"><text>PDF saved</text><text>{}</text></binding></visual><actions><action content=\"Open\" arguments=\"open\" activationType=\"foreground\"/><action content=\"Show folder\" arguments=\"folder\" activationType=\"foreground\"/></actions></toast>",xml(&path.file_name().unwrap_or_default().to_string_lossy()))))?;
    let toast = ToastNotification::CreateToastNotification(&doc)?;
    let destination = path.to_owned();
    toast.Activated(&TypedEventHandler::new(
        move |_sender, args: ::windows::core::Ref<'_, ::windows::core::IInspectable>| {
            let action = args
                .as_ref()
                .and_then(|args| args.cast::<ToastActivatedEventArgs>().ok())
                .and_then(|args| args.Arguments().ok())
                .map(|s| s.to_string())
                .unwrap_or_default();
            if action == "folder" {
                let _ = super::show_in_folder(&destination);
            } else {
                let _ = open::that(&destination);
            }
            Ok(())
        },
    ))?;
    ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from("VirtualPrinterStudio"))?
        .Show(&toast)?;
    // Keep event delegates alive for the most recent notifications.
    static TOASTS: std::sync::Mutex<Vec<ToastNotification>> = std::sync::Mutex::new(Vec::new());
    if let Ok(mut items) = TOASTS.lock() {
        items.push(toast);
        if items.len() > 32 {
            items.remove(0);
        }
    }
    Ok(())
}

pub fn watch_theme(on_change: impl Fn() + Send + 'static) {
    std::thread::spawn(move || {
        use windows_sys::Win32::System::Registry::{
            REG_NOTIFY_CHANGE_LAST_SET, RegNotifyChangeKeyValue,
        };
        if let Ok(key) = RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags(
            "Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize",
            KEY_NOTIFY,
        ) {
            loop {
                // Safety: the registry key stays open on this thread for the entire wait.
                if unsafe {
                    RegNotifyChangeKeyValue(
                        key.raw_handle(),
                        0,
                        REG_NOTIFY_CHANGE_LAST_SET,
                        std::ptr::null_mut(),
                        0,
                    )
                } != 0
                {
                    break;
                }
                on_change();
            }
        }
    });
}

pub fn health(printer: &Printer) -> Result<String> {
    let port = literal(&format!("VPS_{}", printer.id));
    let script = format!(
        "$ErrorActionPreference='Stop'; if ((Get-Service Spooler).Status -ne 'Running'){{throw 'The Windows Print Spooler is stopped. Start it from Services, then retry.'}}; $p=Get-Printer | Where-Object PortName -eq {port} | Select-Object -First 1; if (!$p){{throw 'The Windows printer is missing. Select Install or repair.'}}; $n=@(Get-PrintJob -PrinterName $p.Name).Count; Write-Output \"Folder writable. Windows Print Spooler running. $n job(s) in the Windows queue.\""
    );
    let output = powershell(&script)?;
    ensure!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr).trim()
    );
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn register_notification_shortcut() -> Result<()> {
    use ::windows::{
        Win32::{
            Storage::EnhancedStorage::PKEY_AppUserModel_ID,
            System::{
                Com::{
                    CLSCTX_INPROC_SERVER, CoCreateInstance, IPersistFile,
                    StructuredStorage::{PROPVARIANT, PVCHF_DEFAULT, PropVariantChangeType},
                },
                Variant::VT_LPWSTR,
            },
            UI::Shell::{
                IShellLinkW, PropertiesSystem::IPropertyStore,
                SetCurrentProcessExplicitAppUserModelID, ShellLink,
            },
        },
        core::{HSTRING, Interface, w},
    };
    let appdata = std::env::var_os("APPDATA")
        .ok_or_else(|| anyhow::anyhow!("Cannot locate the Start menu"))?;
    let shortcut = PathBuf::from(appdata)
        .join("Microsoft/Windows/Start Menu/Programs/Virtual Printer Studio.lnk");
    std::fs::create_dir_all(
        shortcut
            .parent()
            .ok_or_else(|| anyhow::anyhow!("Invalid shortcut folder"))?,
    )?;
    let executable = std::env::current_exe()?;
    // Safety: COM was initialized on this delivery thread before this call. The
    // shell-link interfaces and UTF-16 strings remain alive throughout each call;
    // PROPVARIANT owns and frees its contents through windows-rs Drop.
    unsafe {
        SetCurrentProcessExplicitAppUserModelID(w!("VirtualPrinterStudio"))?;
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
        link.SetPath(&HSTRING::from(executable.to_string_lossy().as_ref()))?;
        link.SetDescription(w!("Virtual Printer Studio"))?;
        let properties: IPropertyStore = link.cast()?;
        let source = PROPVARIANT::from("VirtualPrinterStudio");
        let mut value = PROPVARIANT::default();
        PropVariantChangeType(&mut value, &source, PVCHF_DEFAULT, VT_LPWSTR)?;
        properties.SetValue(&PKEY_AppUserModel_ID, &value)?;
        properties.Commit()?;
        let persist: IPersistFile = link.cast()?;
        persist.Save(&HSTRING::from(shortcut.to_string_lossy().as_ref()), true)?;
    }
    Ok(())
}

/// Authorize the loopback peer by comparing its owning PID with the SCM's
/// actual Print Spooler PID. A different local account cannot inject PDFs just
/// by connecting to a loopback socket or naming its process spoolsv.exe.
pub fn authorize_capture(stream: &std::net::TcpStream) -> Result<()> {
    use windows_sys::Win32::{
        Foundation::ERROR_INSUFFICIENT_BUFFER,
        NetworkManagement::IpHelper::{
            GetExtendedTcpTable, MIB_TCPROW_OWNER_PID, TCP_TABLE_OWNER_PID_ALL,
        },
        System::Services::{
            CloseServiceHandle, OpenSCManagerW, OpenServiceW, QueryServiceStatusEx,
            SC_MANAGER_CONNECT, SC_STATUS_PROCESS_INFO, SERVICE_QUERY_STATUS,
            SERVICE_STATUS_PROCESS,
        },
    };
    let peer = stream.peer_addr()?;
    let local = stream.local_addr()?;
    ensure!(
        peer.ip().is_loopback(),
        "Capture accepts only local Windows spooler connections"
    );
    // Safety: all handles/buffers are locally owned and valid through each API;
    // SCM grants unprivileged users service-query access, not service control.
    let spooler_pid = unsafe {
        let manager = OpenSCManagerW(std::ptr::null(), std::ptr::null(), SC_MANAGER_CONNECT);
        ensure!(
            !manager.is_null(),
            "Cannot query the Windows service manager"
        );
        let name: Vec<u16> = "Spooler".encode_utf16().chain(Some(0)).collect();
        let service = OpenServiceW(manager, name.as_ptr(), SERVICE_QUERY_STATUS);
        if service.is_null() {
            CloseServiceHandle(manager);
            anyhow::bail!("Cannot query the Windows Print Spooler");
        }
        let mut status: SERVICE_STATUS_PROCESS = std::mem::zeroed();
        let mut needed = 0;
        let ok = QueryServiceStatusEx(
            service,
            SC_STATUS_PROCESS_INFO,
            (&mut status as *mut SERVICE_STATUS_PROCESS).cast(),
            std::mem::size_of::<SERVICE_STATUS_PROCESS>() as u32,
            &mut needed,
        );
        CloseServiceHandle(service);
        CloseServiceHandle(manager);
        ensure!(
            ok != 0 && status.dwProcessId != 0,
            "The Windows Print Spooler is not running"
        );
        status.dwProcessId
    };
    let mut needed = 0u32;
    unsafe {
        GetExtendedTcpTable(
            std::ptr::null_mut(),
            &mut needed,
            0,
            2,
            TCP_TABLE_OWNER_PID_ALL,
            0,
        )
    };
    for _ in 0..4 {
        let mut buffer = vec![0u32; (needed as usize).div_ceil(4).max(1)];
        let mut bytes = (buffer.len() * 4) as u32;
        let result = unsafe {
            GetExtendedTcpTable(
                buffer.as_mut_ptr().cast(),
                &mut bytes,
                0,
                2,
                TCP_TABLE_OWNER_PID_ALL,
                0,
            )
        };
        if result == ERROR_INSUFFICIENT_BUFFER {
            needed = bytes;
            continue;
        }
        ensure!(
            result == 0,
            "Cannot authenticate the local print connection (Windows error {result})"
        );
        let count = buffer[0] as usize;
        let row_size = std::mem::size_of::<MIB_TCPROW_OWNER_PID>();
        ensure!(
            4 + count * row_size <= buffer.len() * 4,
            "Windows returned an incomplete TCP owner table"
        );
        for i in 0..count {
            let row = unsafe {
                std::ptr::read_unaligned(
                    buffer
                        .as_ptr()
                        .cast::<u8>()
                        .add(4 + i * row_size)
                        .cast::<MIB_TCPROW_OWNER_PID>(),
                )
            };
            if u16::from_be(row.dwLocalPort as u16) == peer.port()
                && u16::from_be(row.dwRemotePort as u16) == local.port()
                && row.dwLocalAddr.to_ne_bytes() == [127, 0, 0, 1]
                && row.dwRemoteAddr.to_ne_bytes() == [127, 0, 0, 1]
            {
                ensure!(
                    row.dwOwningPid == spooler_pid,
                    "Only the Windows Print Spooler may submit captured PDFs"
                );
                return Ok(());
            }
        }
        anyhow::bail!("The print connection could not be matched to the Windows Print Spooler");
    }
    anyhow::bail!(
        "The Windows connection table changed while authenticating the printer; retry the print job"
    )
}

/// Preserve the live Unicode document title before acknowledging the ANSI LPR
/// transfer. Each application printer owns one Standard TCP/IP port, which
/// serializes its transmitted jobs. Never guess if multiple jobs are printing.
pub fn capture_title(id: &str) -> Result<Option<String>> {
    use windows_sys::Win32::Graphics::Printing::*;
    fn enumerate(
        mut call: impl FnMut(*mut u8, u32, &mut u32, &mut u32) -> i32,
    ) -> Result<(Vec<usize>, u32)> {
        let (mut needed, mut count) = (0, 0);
        if call(std::ptr::null_mut(), 0, &mut needed, &mut count) == 0 {
            let error = std::io::Error::last_os_error();
            if error.raw_os_error() != Some(122) {
                return Err(error.into());
            }
        }
        for _ in 0..3 {
            if needed == 0 {
                return Ok((Vec::new(), 0));
            }
            ensure!(needed <= 16 * 1024 * 1024, "Spooler metadata is too large");
            let capacity = needed;
            let mut buffer =
                vec![0usize; (capacity as usize).div_ceil(std::mem::size_of::<usize>())];
            if call(
                buffer.as_mut_ptr().cast(),
                capacity,
                &mut needed,
                &mut count,
            ) != 0
            {
                return Ok((buffer, count));
            }
            let error = std::io::Error::last_os_error();
            if error.raw_os_error() != Some(122) {
                return Err(error.into());
            }
        }
        anyhow::bail!("Spooler metadata changed repeatedly; Windows will retry")
    }
    fn text(pointer: *const u16, buffer: &[usize]) -> Result<String> {
        if pointer.is_null() {
            return Ok(String::new());
        }
        let start = buffer.as_ptr() as usize;
        let end = start + std::mem::size_of_val(buffer);
        let position = pointer as usize;
        ensure!(
            position >= start && position < end && position.is_multiple_of(2),
            "Invalid spooler string"
        );
        // Windows returns all strings inside this caller-owned, aligned buffer.
        let words = unsafe { std::slice::from_raw_parts(pointer, (end - position) / 2) };
        let length = words
            .iter()
            .position(|&c| c == 0)
            .ok_or_else(|| anyhow::anyhow!("Unterminated spooler string"))?;
        Ok(String::from_utf16_lossy(&words[..length]))
    }
    let (printers, count) = enumerate(|p, size, needed, count| unsafe {
        EnumPrintersW(
            PRINTER_ENUM_LOCAL,
            std::ptr::null(),
            5,
            p,
            size,
            needed,
            count,
        )
    })?;
    ensure!(
        count as usize
            <= std::mem::size_of_val(printers.as_slice()) / std::mem::size_of::<PRINTER_INFO_5W>(),
        "Invalid printer metadata count"
    );
    let port = format!("VPS_{id}");
    for index in 0..count as usize {
        // Aligned storage and returned count were checked above.
        let printer = unsafe { &*printers.as_ptr().cast::<PRINTER_INFO_5W>().add(index) };
        if text(printer.pPortName, &printers)? != port {
            continue;
        }
        let mut handle = PRINTER_HANDLE::default();
        if unsafe { OpenPrinterW(printer.pPrinterName, &mut handle, std::ptr::null()) } == 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        struct Guard(PRINTER_HANDLE);
        impl Drop for Guard {
            fn drop(&mut self) {
                unsafe {
                    ClosePrinter(self.0);
                }
            }
        }
        let _guard = Guard(handle);
        let (jobs, count) = enumerate(|p, size, needed, count| unsafe {
            EnumJobsW(handle, 0, u32::MAX, 1, p, size, needed, count)
        })?;
        ensure!(
            count as usize
                <= std::mem::size_of_val(jobs.as_slice()) / std::mem::size_of::<JOB_INFO_1W>(),
            "Invalid job metadata count"
        );
        let mut title = None;
        for index in 0..count as usize {
            let job = unsafe { &*jobs.as_ptr().cast::<JOB_INFO_1W>().add(index) };
            if job.Status & JOB_STATUS_PRINTING == 0 {
                continue;
            }
            ensure!(
                title.is_none(),
                "Multiple jobs are printing on one capture port; Windows will retry"
            );
            let value = text(job.pDocument, &jobs)?;
            if !value.is_empty() {
                title = Some(value.chars().take(512).collect());
            }
        }
        return Ok(title);
    }
    Ok(None)
}

#[cfg(test)]
mod installer_tests {
    use super::*;
    #[test]
    fn locked_installer_is_readable_by_powershell_and_cannot_be_modified() {
        let dir = tempfile::tempdir().expect("folder");
        let path = dir.path().join("installer.ps1");
        let locked = prepare_installer(&path, "Write-Output 'VPS installer readable'; exit 0")
            .expect("prepare installer");
        assert!(
            OpenOptions::new().write(true).open(&path).is_err(),
            "Another writer must be refused while administrator approval is pending"
        );
        let output = powershell(&format!(
            "Set-ExecutionPolicy -Scope Process Bypass -Force; & {}",
            literal(&path.to_string_lossy())
        ))
        .expect("PowerShell");
        assert!(
            output.status.success(),
            "{} {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("VPS installer readable"));
        drop(locked);
        assert!(OpenOptions::new().write(true).open(&path).is_ok());
    }
}
