use super::*;
use anyhow::ensure;
use base64::{Engine, engine::general_purpose::STANDARD};
use std::{
    fs::OpenOptions,
    io::Write,
    os::windows::{fs::OpenOptionsExt, process::CommandExt},
    process::Command,
};
use winreg::{RegKey, enums::*};
fn powershell(script: &str) -> Result<std::process::Output> {
    let utf16: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
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
pub fn native_action(action: &str, printer: &Printer) -> Result<()> {
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
    let exe = std::env::current_exe()?;
    let dll_path = exe.with_file_name(vps_core::MONITOR_DLL);
    let _locked_dll = if action == "ensure" && dll_path.exists() {
        Some(
            OpenOptions::new()
                .read(true)
                .share_mode(1)
                .open(&dll_path)?,
        )
    } else {
        None
    };
    let request = serde_json::json!({"action":action,"id":printer.id,"name":printer.name,"enabled":printer.enabled,"sid":sid,"dll":exe.with_file_name(vps_core::MONITOR_DLL),"result":result_path});
    let script = include_str!("native.ps1").replace(
        "__REQUEST__",
        &STANDARD.encode(serde_json::to_vec(&request)?),
    );
    // Keep this file open without write/delete sharing through the UAC operation;
    // another process cannot replace the script after the user approves elevation.
    let mut locked = OpenOptions::new()
        .create_new(true)
        .read(true)
        .write(true)
        .share_mode(1)
        .open(&script_path)?;
    locked.write_all(script.as_bytes())?;
    locked.sync_all()?;
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
        ensure!(
            output.status.success(),
            "Windows printer setup was cancelled or failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
        anyhow::bail!(
            "Windows setup returned no confirmation. Use Install or repair to check the printer."
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
pub fn spool_root() -> PathBuf {
    PathBuf::from(std::env::var_os("ProgramData").unwrap_or_else(|| "C:\\ProgramData".into()))
        .join("VirtualPrinterStudio")
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
