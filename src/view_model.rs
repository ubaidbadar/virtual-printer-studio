//! Display formatting and model reconciliation; no filesystem or platform calls.
use slint::{Model, ModelRc, VecModel};

pub fn activity_row(a: &vps_core::Activity, now: time::OffsetDateTime) -> crate::ActivityView {
    crate::ActivityView {
        printer: a.printer.clone().into(),
        file: a.file.clone().into(),
        status: if a.status == "Saved" {
            "Saved".into()
        } else if a.status.to_lowercase().contains("folder") {
            "Folder not found".into()
        } else {
            "Couldn't save PDF".into()
        },
        path: a.path.clone().into(),
        printer_id: a.printer_id.clone().into(),
        failed: a.status != "Saved",
        date: relative_time(a.created_unix, now).into(),
        size: if a.size_bytes == 0 {
            "—".into()
        } else {
            bytes_label(a.size_bytes).into()
        },
    }
}

pub fn reconcile<T: Clone + PartialEq + 'static>(model: ModelRc<T>, rows: Vec<T>) -> ModelRc<T> {
    if let Some(target) = model.as_any().downcast_ref::<VecModel<T>>() {
        while target.row_count() > rows.len() {
            target.remove(target.row_count() - 1);
        }
        for (index, row) in rows.into_iter().enumerate() {
            if index >= target.row_count() {
                target.push(row);
            } else if target.row_data(index).as_ref() != Some(&row) {
                target.set_row_data(index, row);
            }
        }
        model
    } else {
        ModelRc::new(VecModel::from(rows))
    }
}

pub fn bytes_label(bytes: u64) -> String {
    if bytes >= 1_048_576 {
        format!("{:.1} MB", bytes as f64 / 1_048_576.0)
    } else {
        format!("{} KB", bytes.div_ceil(1024))
    }
}

pub fn relative_time(timestamp: i64, now: time::OffsetDateTime) -> String {
    if timestamp == 0 {
        return "Earlier".into();
    }
    let seconds = now.unix_timestamp().saturating_sub(timestamp).max(0);
    let Ok(date) = time::OffsetDateTime::from_unix_timestamp(timestamp) else {
        return "Earlier".into();
    };
    let local = date.to_offset(now.offset());
    if now.date().previous_day() == Some(local.date()) {
        return "Yesterday".into();
    }
    if seconds < 60 {
        "Just now".into()
    } else if seconds < 3600 {
        format!("{} min ago", seconds / 60)
    } else if seconds < 86_400 {
        format!("{} h ago", seconds / 3600)
    } else {
        format!(
            "{:04}-{:02}-{:02}",
            local.year(),
            u8::from(local.month()),
            local.day()
        )
    }
}

pub fn filename_preview(pattern: &str, now: time::OffsetDateTime) -> String {
    format!(
        "{}.pdf",
        pattern
            .replace(
                "{date}",
                &format!(
                    "{:04}-{:02}-{:02}",
                    now.year(),
                    u8::from(now.month()),
                    now.day()
                )
            )
            .replace("{time}", &format!("{:02}-{:02}", now.hour(), now.minute()))
            .replace("{title}", "Document")
            .replace("{counter}", "1")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preserves_model_identity_and_updates_rows() {
        let original = ModelRc::new(VecModel::from(vec![1, 2, 3]));
        let changed = reconcile(original.clone(), vec![1, 4]);
        assert_eq!(original, changed);
        assert_eq!(changed.row_count(), 2);
        assert_eq!(changed.row_data(1), Some(4));
    }
    #[test]
    fn formats_local_dates_sizes_and_patterns() {
        let now = time::OffsetDateTime::from_unix_timestamp(1_700_000_000).expect("date");
        assert_eq!(relative_time(now.unix_timestamp() - 120, now), "2 min ago");
        assert_eq!(relative_time(now.unix_timestamp() + 60, now), "Just now");
        assert_eq!(relative_time(0, now), "Earlier");
        assert_eq!(bytes_label(148 * 1024), "148 KB");
        assert_eq!(bytes_label(1_468_006), "1.4 MB");
        assert_eq!(
            filename_preview("Invoice_{date}_{time}_{counter}", now),
            "Invoice_2023-11-14_22-13_1.pdf"
        );
    }
}
