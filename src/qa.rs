//! Deterministic visual fixtures: no service, printer installation, or user state writes.
use crate::{ActivityView, AppWindow, PrinterView, Tokens};
use slint::{ComponentHandle, ModelRc, VecModel};
use std::{
    cell::RefCell,
    path::{Path, PathBuf},
    rc::Rc,
    time::Duration,
};

pub fn snapshots(output: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(output)?;
    let ui = AppWindow::new()?;
    if cfg!(target_os = "linux") {
        ui.global::<Tokens>()
            .set_font_family("Liberation Sans".into());
    }
    #[cfg(target_os = "macos")]
    {
        ui.global::<Tokens>()
            .set_font_family("Helvetica Neue".into());
        ui.set_macos(true);
    }
    ui.set_native_available(true);
    ui.set_capture_running(true);
    ui.set_welcome(false);
    ui.set_default_folder(r"D:\PDFs".into());
    let printers: Vec<_> = [
        ("Invoices", "Ready", 5),
        ("Receipts", "Ready", 4),
        ("Contracts", "Paused", 0),
        ("Reports", "Ready", 2),
        ("Scans archive", "Folder missing", 0),
    ]
    .into_iter()
    .enumerate()
    .map(|(i, (name, status, count))| PrinterView {
        id: i.to_string().into(),
        name: name.into(),
        folder: if i == 4 {
            r"E:\Archive\Scans".into()
        } else {
            format!(r"D:\PDFs\{name}").into()
        },
        status: status.into(),
        count,
        enabled: status != "Paused",
        last_saved: [
            "Invoice_2026-11-15_14-32.pdf · 2 min ago",
            "Receipt_2026-11-15_13-05.pdf · 1 h ago",
            "Nothing saved today",
            "Report_2026-11-14_17-40.pdf · Yesterday",
            "Can't save until the folder is available",
        ][i]
            .into(),
        tone: i as i32,
        is_new: false,
    })
    .collect();
    ui.set_printer_count(5);
    ui.set_saved_count(11);
    ui.set_active_count(3);
    ui.set_attention_count(1);
    ui.set_failed_count(1);
    ui.set_saved_space("1.4 MB".into());
    ui.set_activity_printers(ModelRc::new(VecModel::from(vec![
        "All printers".into(),
        "Invoices".into(),
        "Receipts".into(),
    ])));
    ui.set_attention_id("4".into());
    ui.set_attention_name("Scans archive".into());
    ui.set_attention_folder(r"E:\Archive\Scans".into());
    ui.set_detail_folder_ok(true);
    ui.set_detail_write_ok(true);
    ui.set_detail_free_space("48 GB free".into());
    ui.set_detail_service_ok(true);
    ui.set_detail_space_ok(true);
    ui.set_startup(true);
    ui.set_notifications(true);
    ui.set_keep_tray(true);
    let activity: Vec<_> = [
        (
            "Invoices",
            "Invoice_2026-11-15_14-32.pdf",
            "2 min ago",
            "148 KB",
            false,
        ),
        (
            "Receipts",
            "Receipt_2026-11-15_13-05.pdf",
            "1 h ago",
            "64 KB",
            false,
        ),
        (
            "Scans archive",
            "Scan_2026-11-15_12-20.pdf",
            "2 h ago",
            "",
            true,
        ),
        (
            "Invoices",
            "Invoice_2026-11-15_11-10.pdf",
            "3 h ago",
            "212 KB",
            false,
        ),
        (
            "Receipts",
            "Receipt_2026-11-15_10-41.pdf",
            "4 h ago",
            "58 KB",
            false,
        ),
        (
            "Invoices",
            "Invoice_2026-11-15_09-02.pdf",
            "5 h ago",
            "96 KB",
            false,
        ),
        (
            "Reports",
            "Report_2026-11-14_17-40.pdf",
            "Yesterday",
            "1.1 MB",
            false,
        ),
    ]
    .into_iter()
    .map(|(printer, file, date, size, failed)| ActivityView {
        printer: printer.into(),
        file: file.into(),
        date: date.into(),
        size: size.into(),
        failed,
        status: if failed {
            "Folder not found".into()
        } else {
            "Saved".into()
        },
        ..ActivityView::default()
    })
    .collect();
    ui.set_recent_files(ModelRc::new(VecModel::from(
        activity
            .iter()
            .filter(|a| a.printer == "Invoices")
            .cloned()
            .collect::<Vec<_>>(),
    )));
    ui.set_tray_files(ModelRc::new(VecModel::from(
        activity
            .iter()
            .filter(|a| !a.failed)
            .take(3)
            .cloned()
            .collect::<Vec<_>>(),
    )));
    ui.set_activity(ModelRc::new(VecModel::from(activity)));
    ui.on_preview_pattern({
        let weak = ui.as_weak();
        move || {
            if let Some(ui) = weak.upgrade() {
                ui.set_pattern_preview(
                    crate::view_model::filename_preview(
                        ui.get_pattern().as_str(),
                        time::OffsetDateTime::from_unix_timestamp(1794753120)
                            .unwrap_or(time::OffsetDateTime::UNIX_EPOCH),
                    )
                    .into(),
                );
            }
        }
    });
    let mut grid_printers = printers.clone();
    grid_printers.push(PrinterView {
        is_new: true,
        ..PrinterView::default()
    });
    ui.set_printers(ModelRc::new(VecModel::from(printers.clone())));
    ui.set_printer_rows(ModelRc::new(VecModel::from(
        grid_printers
            .chunks(3)
            .map(|r| ModelRc::new(VecModel::from(r.to_vec())))
            .collect::<Vec<_>>(),
    )));
    let error = Rc::new(RefCell::new(None));
    ui.show()?;
    let delay = std::env::var("VPS_QA_HOLD_SECONDS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(0)
        .min(60);
    let weak = ui.as_weak();
    let directory = output.to_owned();
    let failures = error.clone();
    slint::Timer::single_shot(Duration::from_secs(delay), move || {
        scene(weak, directory, 0, failures, 0)
    });
    slint::run_event_loop()?;
    if let Some(e) = error.borrow_mut().take() {
        return Err(e);
    }
    std::fs::write(
        output.join("capture.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "os": std::env::consts::OS, "architecture": std::env::consts::ARCH,
            "fixture": "deterministic UI state; no native printer backend",
            "scale_factor": ui.window().scale_factor(),
            "font_family": ui.global::<Tokens>().get_font_family().as_str(),
            "reference_normalization": "remove title bar (40 pixels) for scenes 01–07 only; no resizing or alignment",
            "scene_08": "controlled component preview; not a native Windows notification or desktop capture",
            "scene_09": "brand component preview using the shipped SVG icon assets"
        }))?,
    )?;
    Ok(())
}
fn scene(
    weak: slint::Weak<AppWindow>,
    output: PathBuf,
    index: usize,
    error: Rc<RefCell<Option<anyhow::Error>>>,
    resize_attempt: u8,
) {
    let names = [
        "01-printers",
        "02-new-printer",
        "03-printer-detail",
        "04-activity",
        "05-settings",
        "06-first-run",
        "07-printers-dark",
        "08-tray-and-toast",
        "09-logo",
    ];
    let Some(ui) = weak.upgrade() else {
        return;
    };
    if index == names.len() {
        let _ = slint::quit_event_loop();
        return;
    }
    ui.set_page(if index == 3 {
        1
    } else if index == 4 {
        3
    } else {
        0
    });
    ui.set_editor(index == 1 || index == 2);
    ui.set_welcome(index == 5);
    ui.global::<Tokens>().set_dark(index == 6 || index == 7);
    ui.set_showcase(if index == 7 {
        1
    } else if index == 8 {
        2
    } else {
        0
    });
    ui.set_edit_id(if index == 2 { "0" } else { "" }.into());
    ui.set_printer_name(
        if index == 1 || index == 2 {
            "Invoices"
        } else {
            ""
        }
        .into(),
    );
    ui.set_pattern("Invoice_{date}_{time}".into());
    ui.set_pattern_preview("Invoice_2026-11-15_14-32.pdf".into());
    ui.set_monthly(index == 2);
    ui.set_advanced(false);
    ui.set_folder(r"D:\PDFs\Invoices".into());
    let height = if index == 2 {
        960.0
    } else if index == 4 {
        1200.0
    } else if index == 7 {
        800.0
    } else {
        760.0
    };
    let scale = ui.window().scale_factor();
    ui.window().set_size(slint::PhysicalSize::new(
        (1280.0 * scale).round() as u32,
        (height * scale).round() as u32,
    ));
    ui.window().request_redraw();
    slint::Timer::single_shot(Duration::from_millis(500), move || {
        let result = (|| -> anyhow::Result<bool> {
            let ui = weak
                .upgrade()
                .ok_or_else(|| anyhow::anyhow!("Window closed during capture"))?;
            let buffer = ui.window().take_snapshot()?;
            let expected = (
                (1280.0 * scale).round() as u32,
                (height * scale).round() as u32,
            );
            if (buffer.width(), buffer.height()) != expected {
                anyhow::ensure!(
                    resize_attempt < 5,
                    "Native window did not reach requested client size: {:?}, expected {:?}",
                    (buffer.width(), buffer.height()),
                    expected
                );
                return Ok(false);
            }
            let file = std::fs::File::create(output.join(format!("{}.png", names[index])))?;
            let mut encoder = png::Encoder::new(file, buffer.width(), buffer.height());
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder
                .write_header()?
                .write_image_data(buffer.as_bytes())?;
            Ok(true)
        })();
        match result {
            Err(e) => {
                *error.borrow_mut() = Some(e);
                let _ = slint::quit_event_loop();
            }
            Ok(true) => scene(weak, output, index + 1, error, 0),
            Ok(false) => scene(weak, output, index, error, resize_attempt + 1),
        }
    });
}
