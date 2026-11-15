mod domain;
use domain::*;
use slint::{ComponentHandle, ModelRc, VecModel};
use std::{cell::RefCell, path::PathBuf, rc::Rc};
slint::include_modules!();
fn refresh(ui: &AppWindow, s: &State) {
    let query = ui.get_search().to_lowercase();
    let filter = ui.get_filter();
    let rows: Vec<_> = s
        .printers
        .iter()
        .filter(|p| {
            p.name.to_lowercase().contains(&query)
                && (filter != 1 || p.enabled)
                && (filter != 2 || !p.folder.is_dir())
        })
        .map(|p| PrinterView {
            id: p.id.clone().into(),
            name: p.name.clone().into(),
            folder: p.folder.to_string_lossy().to_string().into(),
            enabled: p.enabled,
            count: p.count.min(i32::MAX as u64) as i32,
            status: if !p.folder.is_dir() {
                "Folder missing"
            } else if p.enabled {
                "Ready"
            } else {
                "Paused"
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
    ui.set_activity(ModelRc::new(VecModel::from(
        s.activity
            .iter()
            .rev()
            .map(|a| ActivityView {
                printer: a.printer.clone().into(),
                file: a.file.clone().into(),
                status: a.status.clone().into(),
                path: a.path.clone().into(),
            })
            .collect::<Vec<_>>(),
    )));
}
fn main() -> anyhow::Result<()> {
    let dirs =
        directories::ProjectDirs::from("studio", "VirtualPrinterStudio", "VirtualPrinterStudio")
            .ok_or_else(|| anyhow::anyhow!("Cannot locate app data directory"))?;
    let path = Rc::new(dirs.data_dir().join("state.json"));
    let (initial, warning) = if path.exists() {
        match std::fs::read(&*path)
            .map_err(anyhow::Error::from)
            .and_then(|b| Ok(serde_json::from_slice::<State>(&b)?))
        {
            Ok(s) => (s, String::new()),
            Err(e) => {
                anyhow::bail!(
                    "Cannot read saved state at {}. Restore state.json.bak before restarting: {e}",
                    path.display()
                );
            }
        }
    } else {
        (
            State {
                default_folder: std::env::var("USERPROFILE")
                    .or_else(|_| std::env::var("HOME"))
                    .unwrap_or_default(),
                ..State::default()
            },
            String::new(),
        )
    };
    let state = Rc::new(RefCell::new(initial));
    let ui = AppWindow::new()?;
    ui.set_message(warning.into());
    ui.set_welcome(!state.borrow().welcomed);
    ui.set_default_folder(state.borrow().default_folder.clone().into());
    ui.global::<Tokens>().set_dark(state.borrow().dark);
    refresh(&ui, &state.borrow());
    macro_rules! bind { ($method:ident, |$u:ident,$s:pat,$p:ident$(,$arg:ident)*| $body:block) => {{ let weak=ui.as_weak(); let state=state.clone(); let path=path.clone(); ui.$method(move |$($arg),*| { if let Some($u)=weak.upgrade() { let $s=state.borrow_mut(); let $p=&*path; $body } }); }}; }
    bind!(on_search_changed, |u, s, _p, _query, _filter| {
        refresh(&u, &s);
    });
    bind!(on_dismiss_welcome, |u, mut s, p| {
        s.welcomed = true;
        match save(p, &s) {
            Ok(()) => u.set_welcome(false),
            Err(e) => u.set_message(e.to_string().into()),
        }
    });
    bind!(on_save_printer, |u, mut s, p, id, name, folder, pattern| {
        let folder = PathBuf::from(folder.as_str());
        if let Err(e) = validate(&s, &id, &name, &folder, &pattern) {
            u.set_message(e.to_string().into());
            return;
        }
        let mut next = s.clone();
        if let Some(printer) = next.printers.iter_mut().find(|v| v.id == id.as_str()) {
            printer.name = name.trim().into();
            printer.folder = folder;
            printer.pattern = pattern.to_string();
        } else {
            let id = time::OffsetDateTime::now_utc()
                .unix_timestamp_nanos()
                .to_string();
            next.printers.push(Printer {
                id,
                name: name.trim().into(),
                folder,
                pattern: pattern.to_string(),
                enabled: true,
                count: 0,
            });
        }
        match save(p, &next) {
            Ok(()) => {
                *s = next;
                u.set_editor(false);
                u.set_message("Printer profile saved. Use Test PDF to check its output.".into());
                refresh(&u, &s);
            }
            Err(e) => u.set_message(e.to_string().into()),
        }
    });
    bind!(on_select_printer, |u, s, _p, id| {
        if let Some(v) = s.printers.iter().find(|v| v.id == id.as_str()) {
            u.set_edit_id(id);
            u.set_printer_name(v.name.clone().into());
            u.set_folder(v.folder.to_string_lossy().to_string().into());
            u.set_pattern(v.pattern.clone().into());
            u.set_message("".into());
            u.set_editor(true);
        }
    });
    bind!(on_toggle_printer, |u, mut s, p, id, enabled| {
        let mut next = s.clone();
        if let Some(v) = next.printers.iter_mut().find(|v| v.id == id.as_str()) {
            v.enabled = enabled;
        }
        match save(p, &next) {
            Ok(()) => *s = next,
            Err(e) => u.set_message(e.to_string().into()),
        }
        refresh(&u, &s);
    });
    bind!(on_delete_printer, |u, mut s, p, id| {
        let mut next = s.clone();
        next.printers.retain(|v| v.id != id.as_str());
        match save(p, &next) {
            Ok(()) => {
                *s = next;
                u.set_editor(false);
                refresh(&u, &s);
            }
            Err(e) => u.set_message(e.to_string().into()),
        }
    });
    bind!(on_test_printer, |u, mut s, p, id| {
        if let Some(v) = s.printers.iter_mut().find(|v| v.id == id.as_str()) {
            let name = v.name.clone();
            let result = test_pdf(v);
            let (file, status, path) = match result {
                Ok(path) => (
                    path.file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default(),
                    "Saved".to_string(),
                    path.to_string_lossy().to_string(),
                ),
                Err(e) => ("Test PDF".into(), e.to_string(), String::new()),
            };
            u.set_message(
                if path.is_empty() {
                    status.clone()
                } else {
                    format!("PDF saved to {path}")
                }
                .into(),
            );
            s.activity.push(Activity {
                printer: name,
                file,
                status,
                path,
            });
            if s.activity.len() > 1000 {
                s.activity.remove(0);
            }
            if let Err(e) = save(p, &s) {
                u.set_message(
                    format!("Output processed, but history could not be saved: {e}").into(),
                );
            }
            refresh(&u, &s);
        }
    });
    bind!(on_browse, |u, _s, _p, settings| {
        if let Some(folder) = rfd::FileDialog::new().pick_folder() {
            if settings {
                u.set_default_folder(folder.to_string_lossy().to_string().into());
            } else {
                u.set_folder(folder.to_string_lossy().to_string().into());
            }
        }
    });
    bind!(on_open_path, |u, _s, _p, path| {
        if let Err(e) = open::that(path.as_str()) {
            u.set_message(format!("Could not open file: {e}").into());
        }
    });
    bind!(on_save_settings, |u, mut s, p, dark, folder| {
        let mut next = s.clone();
        next.dark = dark;
        next.default_folder = folder.to_string();
        match save(p, &next) {
            Ok(()) => {
                *s = next;
                u.set_message("Settings saved.".into());
            }
            Err(e) => u.set_message(e.to_string().into()),
        }
    });
    bind!(on_export_backup, |u, s, _p| {
        if let Some(path) = rfd::FileDialog::new()
            .set_file_name("printer-profiles.json")
            .save_file()
        {
            let result = serde_json::to_vec_pretty(&s.printers)
                .map_err(anyhow::Error::from)
                .and_then(|b| Ok(std::fs::write(path, b)?));
            u.set_message(match result {
                Ok(()) => "Profiles exported.".into(),
                Err(e) => e.to_string().into(),
            });
        }
    });
    bind!(on_import_backup, |u, mut s, p| {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("JSON", &["json"])
            .pick_file()
        {
            let result = (|| -> anyhow::Result<State> {
                let bytes = std::fs::read(path)?;
                anyhow::ensure!(bytes.len() < 2_000_000, "Backup is too large");
                let printers: Vec<Printer> = serde_json::from_slice(&bytes)?;
                let mut next = s.clone();
                for mut printer in printers {
                    validate(&next, "", &printer.name, &printer.folder, &printer.pattern)?;
                    printer.id = time::OffsetDateTime::now_utc()
                        .unix_timestamp_nanos()
                        .to_string();
                    next.printers.push(printer);
                }
                save(p, &next)?;
                Ok(next)
            })();
            match result {
                Ok(next) => {
                    *s = next;
                    refresh(&u, &s);
                    u.set_message("Profiles imported.".into());
                }
                Err(e) => u.set_message(e.to_string().into()),
            }
        }
    });
    ui.run()?;
    Ok(())
}

#[cfg(test)]
mod ui_tests {
    use super::*;
    #[test]
    fn pages_and_profile_actions_work() {
        i_slint_backend_testing::init_no_event_loop();
        let ui = AppWindow::new().expect("create UI");
        ui.set_welcome(false);
        let folder = std::env::temp_dir();
        let state = State {
            printers: vec![Printer {
                id: "1".into(),
                name: "Invoices".into(),
                folder,
                pattern: "{title}".into(),
                enabled: true,
                count: 0,
            }],
            ..State::default()
        };
        refresh(&ui, &state);
        use slint::Model;
        assert_eq!(ui.get_printers().row_count(), 1);
        ui.set_search("missing".into());
        refresh(&ui, &state);
        assert_eq!(ui.get_printers().row_count(), 0);
        for page in 0..4 {
            ui.set_page(page);
            ui.show().expect("show page");
            ui.hide().expect("hide page");
        }
        let called = Rc::new(RefCell::new(String::new()));
        let result = called.clone();
        ui.on_select_printer(move |id| *result.borrow_mut() = id.to_string());
        ui.invoke_select_printer("1".into());
        assert_eq!(&*called.borrow(), "1");
    }
}
