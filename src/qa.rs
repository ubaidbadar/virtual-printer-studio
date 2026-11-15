//! Deterministic visual fixtures: no service, printer installation, or user state writes.
use crate::{AppWindow, PrinterView, Tokens};
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
        folder: format!(r"D:\PDFs\{name}").into(),
        status: status.into(),
        count,
        enabled: status != "Paused",
    })
    .collect();
    ui.set_printer_count(5);
    ui.set_saved_count(11);
    ui.set_printers(ModelRc::new(VecModel::from(printers.clone())));
    ui.set_printer_rows(ModelRc::new(VecModel::from(
        printers
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
            "reference_normalization": "remove reference top 40 pixels (window title bar); no resizing or alignment"
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
    ui.global::<Tokens>().set_dark(index == 6);
    ui.set_edit_id(if index == 2 { "0" } else { "" }.into());
    ui.set_printer_name(if index == 2 { "Invoices" } else { "" }.into());
    ui.set_folder(r"D:\PDFs\Invoices".into());
    let height = if index == 2 {
        960.0
    } else if index == 4 {
        1200.0
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
