#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
mod domain;
mod instance;
mod platform;
mod service;
use domain::*;
use service::{Command, Service, Update};
use slint::{ComponentHandle, ModelRc, VecModel};
use std::{path::Path, sync::mpsc::Sender};
slint::include_modules!();
fn display_time(timestamp: i64) -> String {
    let utc = match time::OffsetDateTime::from_unix_timestamp(timestamp) {
        Ok(v) => v,
        Err(_) => return String::new(),
    };
    let local =
        utc.to_offset(time::UtcOffset::local_offset_at(utc).unwrap_or(time::UtcOffset::UTC));
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}",
        local.year(),
        u8::from(local.month()),
        local.day(),
        local.hour(),
        local.minute()
    )
}
fn refresh(ui: &AppWindow, state: &State) {
    let query = ui.get_search().to_lowercase();
    let filter = ui.get_filter();
    let rows: Vec<_> = state
        .printers
        .iter()
        .filter(|p| {
            let attention =
                !p.folder.is_dir() || !p.last_error.is_empty() || (cfg!(windows) && !p.installed);
            p.name.to_lowercase().contains(&query)
                && (filter != 1 || p.enabled)
                && (filter != 2 || attention)
        })
        .map(|p| PrinterView {
            id: p.id.clone().into(),
            name: p.name.clone().into(),
            folder: p.folder.to_string_lossy().to_string().into(),
            enabled: p.enabled,
            count: p.count.min(i32::MAX as u64) as i32,
            status: if !p.folder.is_dir() {
                "Folder missing"
            } else if !p.last_error.is_empty() {
                "Needs attention"
            } else if cfg!(windows) && !p.installed {
                "Not installed"
            } else if !p.enabled {
                "Paused"
            } else if cfg!(windows) {
                "Ready"
            } else {
                "Local profile"
            }
            .into(),
        })
        .collect();
    let columns = ui.get_columns().clamp(1, 3) as usize;
    let grid: Vec<ModelRc<PrinterView>> = rows
        .chunks(columns)
        .map(|row| ModelRc::new(VecModel::from(row.to_vec())))
        .collect();
    ui.set_printer_rows(ModelRc::new(VecModel::from(grid)));
    ui.set_printers(ModelRc::new(VecModel::from(rows)));
    let query = ui.get_activity_search().to_lowercase();
    let filter = ui.get_activity_filter();
    ui.set_activity(ModelRc::new(VecModel::from(
        state
            .activity
            .iter()
            .rev()
            .filter(|a| {
                let failed = a.status != "Saved";
                (filter != 1 || !failed)
                    && (filter != 2 || failed)
                    && (a.file.to_lowercase().contains(&query)
                        || a.printer.to_lowercase().contains(&query)
                        || a.status.to_lowercase().contains(&query))
            })
            .map(|a| ActivityView {
                printer: a.printer.clone().into(),
                file: a.file.clone().into(),
                status: a.status.clone().into(),
                path: a.path.clone().into(),
                printer_id: a.printer_id.clone().into(),
                failed: a.status != "Saved",
                date: if a.created_unix == 0 {
                    "Earlier activity".into()
                } else {
                    display_time(a.created_unix).into()
                },
            })
            .collect::<Vec<_>>(),
    )));
    ui.set_printer_count(state.printers.len().min(i32::MAX as usize) as i32);
    ui.set_saved_count(
        state
            .printers
            .iter()
            .map(|p| p.count)
            .fold(0u64, u64::saturating_add)
            .min(i32::MAX as u64) as i32,
    );
    ui.set_welcome(!state.welcomed);
    ui.set_applied_keep_tray(state.keep_tray);
    ui.global::<Tokens>().set_dark(match state.theme {
        0 => false,
        1 => true,
        _ => platform::system_dark(),
    });
}
fn sync_settings(ui: &AppWindow, state: &State) {
    ui.set_default_folder(state.default_folder.clone().into());
    ui.set_theme(state.theme);
    ui.set_keep_tray(state.keep_tray);
    ui.set_notifications(state.notifications);
    ui.set_startup(state.startup);
}
fn apply(ui: &AppWindow, update: Update) {
    refresh(ui, &update.state);
    ui.set_busy(false);
    ui.set_queued_count(update.queued.min(i32::MAX as usize) as i32);
    if update.sync_settings {
        sync_settings(ui, &update.state);
    }
    if !update.message.is_empty() {
        ui.set_message(update.message.into());
    }
    if update.close_editor {
        ui.set_editor(false);
        ui.set_confirm_delete(false);
    }
    if let Some(p) = update.editor {
        ui.set_edit_id(p.id.into());
        ui.set_printer_name(p.name.into());
        ui.set_folder(p.folder.to_string_lossy().to_string().into());
        ui.set_pattern(p.pattern.into());
        ui.set_monthly(p.monthly_folders);
        ui.set_open_after(p.open_after);
        ui.set_editor(true);
        ui.set_confirm_delete(false);
    }
}
fn send(ui: &AppWindow, sender: &Sender<Command>, command: Command) {
    let busy = !matches!(command, Command::Refresh);
    if let Err(e) = sender.send(command) {
        ui.set_message(format!("The background worker stopped: {e}").into());
    } else if busy {
        ui.set_busy(true);
    }
}
fn main() -> std::process::ExitCode {
    if let Err(error) = run() {
        eprintln!("{error:#}");
        if !std::env::args().any(|arg| arg == "--process-pending") {
            rfd::MessageDialog::new()
                .set_title("Virtual Printer Studio")
                .set_description(format!("{error:#}"))
                .set_level(rfd::MessageLevel::Error)
                .show();
        }
        return std::process::ExitCode::FAILURE;
    }
    std::process::ExitCode::SUCCESS
}
fn run() -> anyhow::Result<()> {
    let dirs =
        directories::ProjectDirs::from("studio", "VirtualPrinterStudio", "VirtualPrinterStudio")
            .ok_or_else(|| anyhow::anyhow!("Cannot locate application data"))?;
    let data = dirs.data_dir().to_owned();
    let path = data.join("state.json");
    let args: Vec<_> = std::env::args().collect();
    if args.iter().any(|arg| arg == "--process-pending") {
        let selected = args
            .iter()
            .position(|arg| arg == "--state")
            .map(|i| {
                args.get(i + 1)
                    .map(std::path::PathBuf::from)
                    .ok_or_else(|| anyhow::anyhow!("--state needs a file path"))
            })
            .transpose()?
            .unwrap_or(path);
        let delivered = service::process_once(&selected, &platform::spool_root())?;
        println!("Delivered {delivered} PDF(s).");
        return Ok(());
    }
    let ui = AppWindow::new()?;
    ui.set_native_available(cfg!(windows));
    let Some(_instance) = instance::Instance::acquire(&data, &ui)? else {
        return Ok(());
    };
    let (mut initial, warning) = load_state(&path)?;
    if initial.default_folder.is_empty() {
        initial.default_folder = directories::UserDirs::new()
            .map(|d| {
                d.document_dir()
                    .unwrap_or(d.home_dir())
                    .to_string_lossy()
                    .to_string()
            })
            .unwrap_or_else(|| ".".into());
    }
    refresh(&ui, &initial);
    sync_settings(&ui, &initial);
    if let Some(message) = warning {
        ui.set_message(message.into());
    }
    let weak = ui.as_weak();
    let service = Service::start(path, initial, platform::spool_root(), move |update| {
        let _ = weak.upgrade_in_event_loop(move |ui| apply(&ui, update));
    })?;
    macro_rules! bind {($method:ident,|$u:ident,$tx:ident$(,$arg:ident)*|$body:block)=>{{let weak=ui.as_weak();let sender=service.sender.clone();ui.$method(move|$($arg),*|{if let Some($u)=weak.upgrade(){let $tx=&sender;$body}});}};}
    bind!(on_refresh, |u, tx| {
        send(&u, tx, Command::Refresh);
    });
    bind!(on_dismiss_welcome, |u, tx| {
        send(&u, tx, Command::Welcome);
    });
    bind!(on_save_printer, |u,
                            tx,
                            id,
                            name,
                            folder,
                            pattern,
                            monthly,
                            open_after| {
        send(
            &u,
            tx,
            Command::SaveProfile {
                id: id.to_string(),
                name: name.to_string(),
                folder: folder.to_string(),
                pattern: pattern.to_string(),
                monthly,
                open_after,
            },
        );
    });
    bind!(on_select_printer, |u, tx, id| {
        u.set_message("".into());
        send(&u, tx, Command::Select(id.to_string()));
    });
    bind!(on_toggle_printer, |u, tx, id, enabled| {
        send(&u, tx, Command::Toggle(id.to_string(), enabled));
    });
    bind!(on_delete_printer, |u, tx, id| {
        send(&u, tx, Command::Delete(id.to_string()));
    });
    bind!(on_repair_printer, |u, tx, id| {
        send(&u, tx, Command::Repair(id.to_string()));
    });
    bind!(on_test_printer, |u, tx, id| {
        send(&u, tx, Command::Test(id.to_string()));
    });
    bind!(on_retry, |u, tx| {
        send(&u, tx, Command::Retry);
    });
    bind!(on_clear_history, |u, tx| {
        send(&u, tx, Command::ClearHistory);
    });
    bind!(on_save_settings, |u, tx| {
        send(
            &u,
            tx,
            Command::Settings {
                dark: u.get_theme() == 1,
                theme: u.get_theme(),
                folder: u.get_default_folder().to_string(),
                tray: u.get_keep_tray(),
                notifications: u.get_notifications(),
                startup: u.get_startup(),
            },
        );
    });
    {
        let weak = ui.as_weak();
        ui.on_browse(move |settings| {
            let weak = weak.clone();
            let _ = slint::spawn_local(async move {
                if let Some(folder) = rfd::AsyncFileDialog::new().pick_folder().await
                    && let Some(ui) = weak.upgrade()
                {
                    let value = folder.path().to_string_lossy().to_string().into();
                    if settings {
                        ui.set_default_folder(value);
                    } else {
                        ui.set_folder(value);
                    }
                }
            });
        });
    }
    {
        let weak = ui.as_weak();
        let tx = service.sender.clone();
        ui.on_export_backup(move || {
            let weak = weak.clone();
            let tx = tx.clone();
            let _ = slint::spawn_local(async move {
                if let Some(file) = rfd::AsyncFileDialog::new()
                    .set_file_name("printer-profiles.json")
                    .add_filter("JSON", &["json"])
                    .save_file()
                    .await
                    && let Some(ui) = weak.upgrade()
                {
                    send(&ui, &tx, Command::Export(file.path().to_owned()));
                }
            });
        });
    }
    {
        let weak = ui.as_weak();
        let tx = service.sender.clone();
        ui.on_import_backup(move || {
            let weak = weak.clone();
            let tx = tx.clone();
            let _ = slint::spawn_local(async move {
                if let Some(file) = rfd::AsyncFileDialog::new()
                    .add_filter("JSON", &["json"])
                    .pick_file()
                    .await
                    && let Some(ui) = weak.upgrade()
                {
                    send(&ui, &tx, Command::Import(file.path().to_owned()));
                }
            });
        });
    }
    bind!(on_open_path, |u, _tx, path| {
        if let Err(e) = open::that(path.as_str()) {
            u.set_message(format!("Could not open: {e}").into());
        }
    });
    bind!(on_show_folder, |u, _tx, path| {
        if let Err(e) = platform::show_in_folder(Path::new(path.as_str())) {
            u.set_message(format!("Could not show folder: {e}").into());
        }
    });
    {
        let weak = ui.as_weak();
        let data = data.clone();
        ui.on_copy_diagnostics(move||{if let Some(ui)=weak.upgrade(){ui.set_diagnostics(format!("Virtual Printer Studio {}\nOS: {} {}\nState: {}\nCapture: {}\nPrinters: {}\nPending: {}\nMonitor: {}\nLog folder: {}",env!("CARGO_PKG_VERSION"),std::env::consts::OS,std::env::consts::ARCH,data.join("state.json").display(),platform::spool_root().display(),ui.get_printer_count(),ui.get_queued_count(),MONITOR_NAME,data.join("logs").display()).into());ui.set_message("Diagnostics copied.".into());}});
    }
    {
        let weak = ui.as_weak();
        let logs = data.join("logs");
        ui.on_open_logs(move || {
            let result = std::fs::create_dir_all(&logs)
                .map_err(anyhow::Error::from)
                .and_then(|_| Ok(open::that(&logs)?));
            if let (Err(e), Some(ui)) = (result, weak.upgrade()) {
                ui.set_message(e.to_string().into());
            }
        });
    }
    {
        let weak = ui.as_weak();
        ui.on_reopen(move || {
            if let Some(ui) = weak.upgrade() {
                let _ = ui.show();
            }
        });
    }
    {
        let weak = ui.as_weak();
        ui.on_quit(move || {
            if let Some(ui) = weak.upgrade()
                && ui.get_busy()
            {
                ui.set_message("Finish the current operation before quitting.".into());
                return;
            }
            let _ = slint::quit_event_loop();
        });
    }
    {
        let weak = ui.as_weak();
        ui.window().on_close_requested(move || {
            if let Some(ui) = weak.upgrade() {
                if ui.get_busy() {
                    ui.set_message("Finish the current operation before closing.".into());
                    return slint::CloseRequestResponse::KeepWindowShown;
                }
                if cfg!(windows) && ui.get_applied_keep_tray() {
                    let _ = ui.hide();
                    return slint::CloseRequestResponse::KeepWindowShown;
                }
            }
            let _ = slint::quit_event_loop();
            slint::CloseRequestResponse::HideWindow
        });
    }
    #[cfg(windows)]
    {
        let tx = service.sender.clone();
        platform::watch_theme(move || {
            let _ = tx.send(Command::Refresh);
        });
    }
    #[cfg(windows)]
    let _tray = {
        let tray = AppTray::new()?;
        let weak = ui.as_weak();
        tray.on_reopen(move || {
            if let Some(ui) = weak.upgrade() {
                ui.invoke_reopen();
            }
        });
        let weak = ui.as_weak();
        tray.on_retry(move || {
            if let Some(ui) = weak.upgrade() {
                ui.invoke_retry();
            }
        });
        let weak = ui.as_weak();
        tray.on_settings(move || {
            if let Some(ui) = weak.upgrade() {
                ui.set_page(3);
                ui.invoke_reopen();
            }
        });
        let weak = ui.as_weak();
        tray.on_quit(move || {
            if let Some(ui) = weak.upgrade() {
                ui.invoke_quit();
            }
        });
        tray
    };
    ui.show()?;
    if cfg!(windows) && std::env::args().any(|arg| arg == "--background") {
        ui.hide()?;
    }
    slint::run_event_loop_until_quit()?;
    drop(service);
    Ok(())
}

#[cfg(test)]
mod ui_tests {
    use super::*;
    #[test]
    fn pages_search_filters_and_actions_work() {
        i_slint_backend_testing::init_no_event_loop();
        let ui = AppWindow::new().expect("UI");
        ui.set_welcome(false);
        let state = State {
            welcomed: true,
            printers: vec![Printer {
                id: "one".into(),
                name: "Invoices".into(),
                folder: std::env::temp_dir(),
                pattern: "{title}".into(),
                enabled: true,
                ..Printer::default()
            }],
            activity: vec![Activity {
                printer: "Invoices".into(),
                file: "Invoice.pdf".into(),
                status: "Saved".into(),
                path: "Invoice.pdf".into(),
                printer_id: "one".into(),
                job_id: "job1".into(),
                created_unix: 1700000000,
            }],
            ..State::default()
        };
        use slint::Model;
        refresh(&ui, &state);
        assert_eq!(ui.get_printers().row_count(), 1);
        assert_eq!(ui.get_activity().row_count(), 1);
        ui.set_search("missing".into());
        refresh(&ui, &state);
        assert_eq!(ui.get_printers().row_count(), 0);
        ui.set_activity_filter(2);
        refresh(&ui, &state);
        assert_eq!(ui.get_activity().row_count(), 0);
        for page in 0..4 {
            ui.set_page(page);
            ui.show().expect("show");
            ui.hide().expect("hide");
        }
        let called = std::rc::Rc::new(std::cell::RefCell::new(String::new()));
        let result = called.clone();
        ui.on_select_printer(move |id| *result.borrow_mut() = id.to_string());
        ui.invoke_select_printer("one".into());
        assert_eq!(&*called.borrow(), "one");
    }
}
