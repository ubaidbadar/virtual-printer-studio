#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
mod domain;
mod instance;
mod platform;
mod qa;
mod service;
mod view_model;
use domain::*;
use service::{Command, Service, Update};
use slint::{ComponentHandle, Model, ModelRc};
use std::{path::Path, sync::mpsc::Sender};
slint::include_modules!();
fn refresh(ui: &AppWindow, state: &State) {
    let utc = time::OffsetDateTime::now_utc();
    let now = utc.to_offset(time::UtcOffset::local_offset_at(utc).unwrap_or(time::UtcOffset::UTC));
    let is_today = |entry: &&Activity| {
        time::OffsetDateTime::from_unix_timestamp(entry.created_unix)
            .is_ok_and(|date| date.to_offset(now.offset()).date() == now.date())
    };
    let saved_today: Vec<_> = state
        .activity
        .iter()
        .filter(|a| a.status == "Saved")
        .filter(is_today)
        .collect();
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
        .enumerate()
        .map(|(index, p)| PrinterView {
            id: p.id.clone().into(),
            name: p.name.clone().into(),
            folder: p.folder.to_string_lossy().to_string().into(),
            enabled: p.enabled,
            count: saved_today
                .iter()
                .filter(|a| a.printer_id == p.id)
                .count()
                .min(i32::MAX as usize) as i32,
            last_saved: if !p.folder.is_dir() {
                "Can't save until the folder is available".into()
            } else {
                state
                    .activity
                    .iter()
                    .rev()
                    .find(|a| a.printer_id == p.id && a.status == "Saved")
                    .map(|a| {
                        format!(
                            "{} · {}",
                            a.file,
                            view_model::relative_time(a.created_unix, now)
                        )
                    })
                    .unwrap_or_else(|| "Nothing saved today".into())
                    .into()
            },
            tone: if !p.folder.is_dir() || !p.last_error.is_empty() {
                4
            } else {
                (index % 4) as i32
            },
            is_new: false,
            status: if !p.folder.is_dir() {
                "Folder missing"
            } else if !p.last_error.is_empty() {
                "Needs attention"
            } else if cfg!(windows) && !p.installed {
                "Not installed"
            } else if !p.enabled {
                "Paused"
            } else if cfg!(windows) && !ui.get_capture_running() {
                "Receiver offline"
            } else if cfg!(windows) {
                "Ready"
            } else {
                "Local profile"
            }
            .into(),
        })
        .collect();
    let columns = ui.get_columns().clamp(1, 3) as usize;
    let mut grid_rows = rows.clone();
    grid_rows.push(PrinterView {
        is_new: true,
        ..PrinterView::default()
    });
    let previous = ui.get_printer_rows();
    let grid: Vec<ModelRc<PrinterView>> = grid_rows
        .chunks(columns)
        .enumerate()
        .map(|(index, row)| {
            view_model::reconcile(previous.row_data(index).unwrap_or_default(), row.to_vec())
        })
        .collect();
    ui.set_printer_rows(view_model::reconcile(previous, grid));
    ui.set_printers(view_model::reconcile(ui.get_printers(), rows));
    let mut names = vec![slint::SharedString::from("All printers")];
    names.extend(
        state
            .printers
            .iter()
            .map(|p| slint::SharedString::from(p.name.as_str())),
    );
    ui.set_activity_printers(view_model::reconcile(ui.get_activity_printers(), names));
    let selected = if ui.get_activity_printer() > 0 {
        state
            .printers
            .get((ui.get_activity_printer() - 1) as usize)
            .map(|p| p.id.as_str())
    } else {
        None
    };
    let date_filter = ui.get_activity_date();
    let query = ui.get_activity_search().to_lowercase();
    let filter = ui.get_activity_filter();
    let activity_rows: Vec<_> = state
        .activity
        .iter()
        .rev()
        .filter(|a| {
            let failed = a.status != "Saved";
            let in_date = date_filter == 1
                || time::OffsetDateTime::from_unix_timestamp(a.created_unix).is_ok_and(|date| {
                    if date_filter == 0 {
                        date.to_offset(now.offset()).date() == now.date()
                    } else {
                        a.created_unix >= now.unix_timestamp() - 7 * 86_400
                    }
                });
            in_date
                && selected.is_none_or(|id| a.printer_id == id)
                && (filter != 1 || !failed)
                && (filter != 2 || failed)
                && (a.file.to_lowercase().contains(&query)
                    || a.printer.to_lowercase().contains(&query)
                    || a.status.to_lowercase().contains(&query))
        })
        .map(|a| view_model::activity_row(a, now))
        .collect();
    ui.set_activity(view_model::reconcile(ui.get_activity(), activity_rows));
    ui.set_printer_count(state.printers.len().min(i32::MAX as usize) as i32);
    ui.set_saved_count(saved_today.len().min(i32::MAX as usize) as i32);
    ui.set_saved_space(
        view_model::bytes_label(saved_today.iter().map(|a| a.size_bytes).sum()).into(),
    );
    ui.set_active_count(
        state
            .printers
            .iter()
            .filter(|p| {
                p.enabled
                    && p.folder.is_dir()
                    && p.last_error.is_empty()
                    && (!cfg!(windows) || p.installed)
            })
            .count()
            .min(i32::MAX as usize) as i32,
    );
    ui.set_attention_count(
        state
            .printers
            .iter()
            .filter(|p| {
                !p.folder.is_dir() || !p.last_error.is_empty() || (cfg!(windows) && !p.installed)
            })
            .count()
            .min(i32::MAX as usize) as i32,
    );
    ui.set_failed_count(
        state
            .activity
            .iter()
            .filter(|a| a.status != "Saved")
            .filter(is_today)
            .count()
            .min(i32::MAX as usize) as i32,
    );
    ui.set_any_enabled(state.printers.iter().any(|p| p.enabled));
    ui.set_tray_files(view_model::reconcile(
        ui.get_tray_files(),
        state
            .activity
            .iter()
            .rev()
            .filter(|a| a.status == "Saved")
            .take(3)
            .map(|a| view_model::activity_row(a, now))
            .collect(),
    ));
    if let Some(p) = state.printers.iter().find(|p| !p.folder.is_dir()) {
        ui.set_attention_id(p.id.clone().into());
        ui.set_attention_name(p.name.clone().into());
        ui.set_attention_folder(p.folder.to_string_lossy().to_string().into());
    } else {
        ui.set_attention_id("".into());
    }
    if let Some(p) = state
        .printers
        .iter()
        .find(|p| p.id == ui.get_edit_id().as_str())
    {
        ui.set_detail_enabled(p.enabled);
        ui.set_detail_folder_ok(p.folder.is_dir());
        ui.set_detail_write_ok(p.folder.is_dir() && p.last_error.is_empty());
        ui.set_detail_status(
            if !p.folder.is_dir() {
                "Folder missing"
            } else if !p.last_error.is_empty() {
                "Needs attention"
            } else if !p.enabled {
                "Paused"
            } else if cfg!(windows) && !p.installed {
                "Not installed"
            } else {
                "Ready"
            }
            .into(),
        );
        let recent = state
            .activity
            .iter()
            .rev()
            .filter(|a| a.printer_id == p.id && a.status == "Saved")
            .take(3)
            .map(|a| view_model::activity_row(a, now))
            .collect();
        ui.set_recent_files(view_model::reconcile(ui.get_recent_files(), recent));
    }
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
    ui.set_history_index(match state.history_days {
        30 => 0,
        365 => 2,
        0 => 3,
        _ => 1,
    });
}
fn apply(ui: &AppWindow, update: Update) {
    ui.set_capture_running(update.capture_running);
    ui.set_capture_port(update.capture_port as i32);
    ui.set_busy(false);
    ui.set_queued_count(update.queued.min(i32::MAX as usize) as i32);
    if update.sync_settings {
        sync_settings(ui, &update.state);
    }
    if !update.message.is_empty() {
        ui.set_message(update.message.into());
    }
    if update.close_editor {
        ui.set_editor(
            !ui.get_edit_id().is_empty()
                && update
                    .state
                    .printers
                    .iter()
                    .any(|p| p.id == ui.get_edit_id().as_str()),
        );
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
    refresh(ui, &update.state);
    if let Some(health) = update.editor_health {
        ui.set_detail_write_ok(health.write);
        ui.set_detail_service_ok(health.service);
        ui.set_detail_space_ok(health.free_bytes.is_some_and(|bytes| bytes > 0));
        ui.set_detail_free_space(
            health
                .free_bytes
                .map(|bytes| format!("{} GB free", bytes / 1_073_741_824))
                .unwrap_or_else(|| "Free space unavailable".into())
                .into(),
        );
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
        if !std::env::args()
            .any(|arg| arg == "--process-pending" || arg == "--serve" || arg == "--ui-snapshot")
        {
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
    let args: Vec<_> = std::env::args().collect();
    if let Some(index) = args.iter().position(|arg| arg == "--ui-snapshot") {
        let output = args
            .get(index + 1)
            .ok_or_else(|| anyhow::anyhow!("--ui-snapshot needs an output directory"))?;
        return qa::snapshots(std::path::Path::new(output));
    }
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
        let delivered = service::process_once(
            &selected,
            &platform::spool_root(selected.parent().unwrap_or(Path::new("."))),
        )?;
        println!("Delivered {delivered} PDF(s).");
        return Ok(());
    }
    if args.iter().any(|arg| arg == "--serve") {
        anyhow::ensure!(
            cfg!(windows),
            "The Windows capture service requires Windows"
        );
        let selected = args
            .iter()
            .position(|arg| arg == "--state")
            .and_then(|i| args.get(i + 1))
            .map(std::path::PathBuf::from)
            .unwrap_or(path);
        let data = selected
            .parent()
            .ok_or_else(|| anyhow::anyhow!("State path needs a parent folder"))?;
        std::fs::create_dir_all(data)?;
        let lock = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(data.join("app.lock"))?;
        lock.try_lock()
            .map_err(|e| anyhow::anyhow!("The app is already running: {e}"))?;
        let (state, _) = load_state(&selected)?;
        let (tx, rx) = std::sync::mpsc::channel();
        let _service = Service::start(
            selected.clone(),
            state,
            platform::spool_root(data),
            move |update| {
                let _ = tx.send(update);
            },
        )?;
        loop {
            let update = rx.recv()?;
            if !update.capture_running {
                anyhow::bail!("Capture receiver failed: {}", update.message);
            }
            if !update.message.is_empty() {
                println!("{}", update.message);
            }
        }
    }
    let ui = AppWindow::new()?;
    if !cfg!(windows) {
        ui.global::<Tokens>()
            .set_font_family("Liberation Sans".into());
    }
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
    let service = Service::start(path, initial, platform::spool_root(&data), move |update| {
        let _ = weak.upgrade_in_event_loop(move |ui| apply(&ui, update));
    })?;
    macro_rules! bind {($method:ident,|$u:ident,$tx:ident$(,$arg:ident)*|$body:block)=>{{let weak=ui.as_weak();let sender=service.sender.clone();ui.$method(move|$($arg),*|{if let Some($u)=weak.upgrade(){let $tx=&sender;$body}});}};}
    bind!(on_refresh, |u, tx| {
        send(&u, tx, Command::Refresh);
    });
    ui.on_preview_pattern({
        let weak = ui.as_weak();
        move || {
            if let Some(ui) = weak.upgrade() {
                let utc = time::OffsetDateTime::now_utc();
                let now = utc.to_offset(
                    time::UtcOffset::local_offset_at(utc).unwrap_or(time::UtcOffset::UTC),
                );
                ui.set_pattern_preview(
                    view_model::filename_preview(ui.get_pattern().as_str(), now).into(),
                );
            }
        }
    });
    ui.invoke_preview_pattern();
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
                            open_after,
                            start_enabled| {
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
                start_enabled,
            },
        );
    });
    {
        let weak = ui.as_weak();
        let sender = service.sender.clone();
        ui.on_choose_folder(move |id| {
            let weak = weak.clone();
            let sender = sender.clone();
            let _ = slint::spawn_local(async move {
                if let Some(folder) = rfd::AsyncFileDialog::new().pick_folder().await
                    && let Some(ui) = weak.upgrade()
                {
                    send(
                        &ui,
                        &sender,
                        Command::Relocate {
                            id: id.to_string(),
                            folder: folder.path().to_owned(),
                        },
                    );
                }
            });
        });
    }
    bind!(on_select_printer, |u, tx, id| {
        u.set_message("".into());
        send(&u, tx, Command::Select(id.to_string()));
    });
    bind!(on_toggle_printer, |u, tx, id, enabled| {
        send(&u, tx, Command::Toggle(id.to_string(), enabled));
    });
    bind!(on_toggle_all_printers, |u, tx, enabled| {
        send(&u, tx, Command::SetAll(enabled));
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
                history_days: match u.get_history_index() {
                    0 => 30,
                    2 => 365,
                    3 => 0,
                    _ => 90,
                },
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
                        if !ui.get_welcome() {
                            ui.invoke_save_settings();
                        }
                    } else {
                        ui.set_folder(value);
                        if !ui.get_edit_id().is_empty() && ui.get_editor() {
                            ui.invoke_save_printer(
                                ui.get_edit_id(),
                                ui.get_printer_name(),
                                ui.get_folder(),
                                ui.get_pattern(),
                                ui.get_monthly(),
                                ui.get_open_after(),
                                true,
                            );
                        }
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
        ui.on_copy_diagnostics(move||{if let Some(ui)=weak.upgrade(){ui.set_diagnostics(format!("Virtual Printer Studio {}\nOS: {} {}\nState: {}\nCapture: {}\nPrinters: {}\nPending: {}\nMonitor: {}\nLog folder: {}",env!("CARGO_PKG_VERSION"),std::env::consts::OS,std::env::consts::ARCH,data.join("state.json").display(),platform::spool_root(&data).display(),ui.get_printer_count(),ui.get_queued_count(),CAPTURE_BACKEND,data.join("logs").display()).into());ui.set_message("Diagnostics copied.".into());}});
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
        let flyout = TrayWindow::new()?;
        let tray_weak = tray.as_weak();
        ui.on_show_tray(move || {
            if let Some(tray) = tray_weak.upgrade() {
                tray.invoke_popup();
            }
        });
        let weak = ui.as_weak();
        let menu = flyout.as_weak();
        tray.on_popup(move || {
            use slint::winit_030::{EventResult, WinitWindowAccessor, winit::event::WindowEvent};
            if let (Some(ui), Some(menu)) = (weak.upgrade(), menu.upgrade()) {
                menu.global::<Tokens>()
                    .set_dark(ui.global::<Tokens>().get_dark());
                menu.set_ready(ui.get_active_count());
                menu.set_any_enabled(ui.get_any_enabled());
                menu.set_busy(ui.get_busy());
                menu.set_recent(ui.get_tray_files());
                let _ = menu.show();
                let size = menu.window().size();
                if let Some((x, y)) = platform::tray_position(size.width as i32, size.height as i32)
                {
                    menu.window()
                        .set_position(slint::PhysicalPosition::new(x, y));
                }
                let focused = std::cell::Cell::new(false);
                menu.window().on_winit_window_event(move |window, event| {
                    if let WindowEvent::Focused(active) = event {
                        if *active {
                            focused.set(true);
                        } else if focused.replace(false) {
                            let _ = window.hide();
                        }
                    }
                    EventResult::Propagate
                });
            }
        });
        let weak = ui.as_weak();
        let menu = flyout.as_weak();
        flyout.on_open(move || {
            if let Some(menu) = menu.upgrade() {
                let _ = menu.hide();
            }
            if let Some(ui) = weak.upgrade() {
                ui.invoke_reopen();
            }
        });
        let weak = ui.as_weak();
        let menu = flyout.as_weak();
        flyout.on_toggle_all(move |enabled| {
            if let Some(menu) = menu.upgrade() {
                let _ = menu.hide();
            }
            if let Some(ui) = weak.upgrade() {
                ui.invoke_toggle_all_printers(enabled);
            }
        });
        let weak = ui.as_weak();
        let menu = flyout.as_weak();
        flyout.on_file(move |path| {
            if let Some(menu) = menu.upgrade() {
                let _ = menu.hide();
            }
            if let Some(ui) = weak.upgrade() {
                ui.invoke_open_path(path);
            }
        });
        let weak = ui.as_weak();
        let menu = flyout.as_weak();
        flyout.on_settings(move || {
            if let Some(menu) = menu.upgrade() {
                let _ = menu.hide();
            }
            if let Some(ui) = weak.upgrade() {
                ui.set_editor(false);
                ui.set_page(3);
                ui.invoke_reopen();
            }
        });
        let weak = ui.as_weak();
        let menu = flyout.as_weak();
        flyout.on_quit(move || {
            if let Some(menu) = menu.upgrade() {
                let _ = menu.hide();
            }
            if let Some(ui) = weak.upgrade() {
                ui.invoke_quit();
            }
        });
        let menu = flyout.as_weak();
        flyout.on_dismiss(move || {
            if let Some(menu) = menu.upgrade() {
                let _ = menu.hide();
            }
        });
        let menu = flyout.as_weak();
        flyout.window().on_close_requested(move || {
            if let Some(menu) = menu.upgrade() {
                let _ = menu.hide();
            }
            slint::CloseRequestResponse::KeepWindowShown
        });
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
                ui.set_editor(false);
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
        (tray, flyout)
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
        ui.set_activity_date(1);
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
                size_bytes: 148 * 1024,
            }],
            ..State::default()
        };
        use slint::Model;
        refresh(&ui, &state);
        assert_eq!(ui.get_printers().row_count(), 1);
        assert_eq!(ui.get_activity().row_count(), 1);
        ui.set_activity_date(0);
        refresh(&ui, &state);
        assert_eq!(
            ui.get_activity().row_count(),
            0,
            "Today excludes older entries"
        );
        ui.set_activity_date(1);
        ui.set_activity_printer(1);
        refresh(&ui, &state);
        assert_eq!(
            ui.get_activity().row_count(),
            1,
            "Printer selection retains its entries"
        );
        ui.set_activity_printer(0);
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
        // Exercise the actual widgets' accessibility actions, rather than
        // invoking application callbacks directly.
        use i_slint_backend_testing::{AccessibleRole, ElementHandle};
        let activate = |label: &str| {
            ElementHandle::find_by_accessible_label(&ui, label)
                .find(|element| element.accessible_role() == Some(AccessibleRole::Button))
                .unwrap_or_else(|| panic!("Missing button: {label}"))
                .invoke_accessible_default_action();
        };
        ui.show().expect("show");
        for (label, page) in [
            ("Activity", 1),
            ("Presets", 2),
            ("Settings", 3),
            ("Printers", 0),
        ] {
            activate(label);
            assert_eq!(ui.get_page(), page, "{label} navigation");
        }
        ui.set_default_folder("C:\\PDFs".into());
        activate("+ New printer");
        assert!(ui.get_editor());
        assert_eq!(
            ElementHandle::find_by_accessible_label(&ui, "Printers")
                .find(|element| element.accessible_role() == Some(AccessibleRole::Button))
                .expect("navigation")
                .accessible_enabled(),
            Some(false)
        );
        assert!(ui.get_edit_id().is_empty());
        assert_eq!(ui.get_folder(), "C:\\PDFs");
        ui.set_printer_name("Invoices".into());
        let saved = std::rc::Rc::new(std::cell::RefCell::new(None));
        let received = saved.clone();
        ui.on_save_printer(
            move |id, name, folder, pattern, monthly, open_after, start_enabled| {
                *received.borrow_mut() = Some((
                    id,
                    name,
                    folder,
                    pattern,
                    monthly,
                    open_after,
                    start_enabled,
                ));
            },
        );
        activate("Save printer");
        let saved_value = saved.borrow();
        let (_, name, folder, pattern, monthly, open_after, start_enabled) =
            saved_value.as_ref().expect("save callback");
        assert_eq!(name, "Invoices");
        assert_eq!(folder, "C:\\PDFs");
        assert_eq!(pattern, "{title}_{date}_{counter}");
        assert!(!monthly && !open_after && *start_enabled);
        drop(saved_value);
        ui.set_start_saving(false);
        activate("Save printer");
        assert!(!saved.borrow().as_ref().expect("paused creation callback").6);
        activate("Cancel");
        assert!(!ui.get_editor());
        activate("+ New printer");
        ui.window()
            .dispatch_event(slint::platform::WindowEvent::KeyPressed {
                text: slint::platform::Key::Escape.into(),
            });
        assert!(!ui.get_editor(), "Escape dismisses the new-printer dialog");
        activate("Presets");
        activate("Use preset");
        assert!(ui.get_editor());
        assert_eq!(ui.get_printer_name(), "Invoices");
        assert_eq!(ui.get_pattern(), "Invoice_{date}_{counter}");
        activate("Cancel");
        ui.hide().expect("hide");
    }
}
