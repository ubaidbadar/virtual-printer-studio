use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Printer {
    pub id: String,
    pub name: String,
    pub folder: PathBuf,
    pub pattern: String,
    pub enabled: bool,
    pub count: u64,
    pub installed: bool,
    pub last_error: String,
    pub open_after: bool,
    pub monthly_folders: bool,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Activity {
    pub printer: String,
    pub file: String,
    pub status: String,
    pub path: String,
    #[serde(default)]
    pub printer_id: String,
    #[serde(default)]
    pub job_id: String,
    #[serde(default)]
    pub created_unix: i64,
    #[serde(default)]
    pub size_bytes: u64,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct State {
    pub printers: Vec<Printer>,
    pub activity: Vec<Activity>,
    pub dark: bool,
    pub default_folder: String,
    pub welcomed: bool,
    pub keep_tray: bool,
    pub notifications: bool,
    pub startup: bool,
    pub theme: i32,
    pub capture_port: u16,
    pub history_days: u32,
}
impl Default for State {
    fn default() -> Self {
        Self {
            printers: vec![],
            activity: vec![],
            dark: false,
            default_folder: String::new(),
            welcomed: false,
            keep_tray: true,
            notifications: true,
            startup: false,
            theme: 2,
            capture_port: 0,
            history_days: 90,
        }
    }
}
/// Retention applies only to history metadata. Unknown legacy dates are retained.
pub fn prune_activity(state: &mut State, now: i64) {
    if state.history_days != 0 {
        let cutoff = now.saturating_sub(i64::from(state.history_days) * 86_400);
        state
            .activity
            .retain(|entry| entry.created_unix == 0 || entry.created_unix >= cutoff);
    }
}
pub fn validate(state: &State, id: &str, name: &str, folder: &Path, pattern: &str) -> Result<()> {
    if name.trim().is_empty() {
        bail!("Enter a printer name.");
    }
    if state
        .printers
        .iter()
        .any(|p| p.id != id && p.name.eq_ignore_ascii_case(name.trim()))
    {
        bail!("A printer with this name already exists.");
    }
    if !folder.is_dir() {
        bail!("Choose an existing output folder.");
    }
    if pattern.is_empty() {
        bail!("Enter a file name pattern.");
    }
    let mut remaining = pattern.to_owned();
    for token in ["{title}", "{date}", "{time}", "{counter}"] {
        remaining = remaining.replace(token, "sample");
    }
    if remaining
        .chars()
        .any(|c| c.is_control() || "<>:\"/\\|?*{}".contains(c))
        || remaining.ends_with(['.', ' '])
    {
        bail!("Use a filename without reserved characters or unknown tokens.");
    }
    let probe = folder.join(format!(".vps-write-check-{}", std::process::id()));
    let f = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe)
        .context("The output folder is not writable")?;
    drop(f);
    fs::remove_file(probe)?;
    Ok(())
}
pub fn save(path: &Path, state: &State) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    // Keep the previous valid state available for recovery.
    if path.exists() {
        fs::copy(path, path.with_extension("json.bak"))?;
    }
    let parent = path
        .parent()
        .context("State file needs a parent directory")?;
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    file.write_all(&serde_json::to_vec_pretty(state)?)?;
    file.as_file().sync_all()?;
    file.persist(path).map_err(|e| e.error)?;
    Ok(())
}
pub fn test_pdf(p: &mut Printer) -> Result<PathBuf> {
    if !p.enabled {
        bail!("Enable this profile before saving a test PDF.");
    }
    let now = time::OffsetDateTime::now_utc();
    let date = format!(
        "{:04}-{:02}-{:02}",
        now.year(),
        u8::from(now.month()),
        now.day()
    );
    let clock = format!("{:02}-{:02}-{:02}", now.hour(), now.minute(), now.second());
    let base = p
        .pattern
        .replace("{title}", "Test page")
        .replace("{date}", &date)
        .replace("{time}", &clock)
        .replace("{counter}", &(p.count + 1).to_string());
    // Defensive validation even for imported profiles.
    validate(&State::default(), "", &p.name, &p.folder, &p.pattern)?;
    let bytes = pdf_bytes();
    for suffix in 0..10000 {
        let name = if suffix == 0 {
            format!("{base}.pdf")
        } else {
            format!("{base}_{suffix}.pdf")
        };
        let path = p.folder.join(name);
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut f) => {
                if let Err(e) = f.write_all(&bytes).and_then(|_| f.sync_all()) {
                    let _ = fs::remove_file(&path);
                    return Err(e.into());
                }
                p.count += 1;
                return Ok(path);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e.into()),
        }
    }
    bail!("Too many files with this name. Change the file name pattern.")
}
fn pdf_bytes() -> Vec<u8> {
    let stream = "BT /F1 24 Tf 72 760 Td (Virtual Printer Studio) Tj 0 -40 Td /F1 12 Tf (Your PDF output folder is working.) Tj ET";
    let objects = ["<< /Type /Catalog /Pages 2 0 R >>".to_string(), "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".into(), "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>".into(), "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into(), format!("<< /Length {} >>\nstream\n{stream}\nendstream", stream.len())];
    let mut out = b"%PDF-1.4\n".to_vec();
    let mut offsets = vec![0];
    for (i, object) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend(format!("{} 0 obj\n{object}\nendobj\n", i + 1).as_bytes());
    }
    let xref = out.len();
    out.extend(b"xref\n0 6\n0000000000 65535 f \n");
    for offset in offsets.iter().skip(1) {
        out.extend(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend(
        format!("trailer\n<< /Size 6 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
    );
    out
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn history_retention_keeps_recent_and_undated_metadata() {
        let now = 2_000_000_000;
        let row = |timestamp| Activity {
            printer: String::new(),
            file: String::new(),
            status: "Saved".into(),
            path: "existing.pdf".into(),
            printer_id: String::new(),
            job_id: String::new(),
            created_unix: timestamp,
            size_bytes: 100,
        };
        let mut state = State {
            activity: vec![
                row(0),
                row(now - 91 * 86_400),
                row(now - 90 * 86_400),
                row(now),
            ],
            ..State::default()
        };
        prune_activity(&mut state, now);
        assert_eq!(
            state
                .activity
                .iter()
                .map(|a| a.created_unix)
                .collect::<Vec<_>>(),
            vec![0, now - 90 * 86_400, now]
        );
        state.history_days = 0;
        state.activity.push(row(1));
        prune_activity(&mut state, now);
        assert_eq!(state.activity.len(), 4);
        let legacy:State=serde_json::from_str(r#"{"activity":[{"printer":"Old","file":"old.pdf","path":"old.pdf","status":"Saved"}]}"#).expect("old schema");
        assert_eq!(legacy.history_days, 90);
        assert_eq!(legacy.activity[0].size_bytes, 0);
    }
    #[test]
    fn pdf_offsets_are_valid() {
        let bytes = pdf_bytes();
        let text = String::from_utf8(bytes.clone()).expect("ascii");
        let xref: usize = text
            .split("startxref\n")
            .nth(1)
            .expect("xref")
            .lines()
            .next()
            .expect("offset")
            .parse()
            .expect("number");
        assert!(bytes[xref..].starts_with(b"xref"));
        for line in text
            .split("xref\n0 6\n")
            .nth(1)
            .expect("table")
            .lines()
            .skip(1)
            .take(5)
        {
            let n: usize = line[..10].parse().expect("offset");
            assert!(text[n..].contains("0 obj"));
        }
    }
    #[test]
    fn rejects_unsafe_patterns_and_duplicates() {
        let dir = std::env::temp_dir();
        let mut state = State::default();
        state.printers.push(Printer {
            id: "1".into(),
            name: "Invoices".into(),
            folder: dir.clone(),
            pattern: "{title}".into(),
            enabled: true,
            count: 0,
            ..Printer::default()
        });
        assert!(validate(&state, "", "invoices", &dir, "{title}").is_err());
        assert!(validate(&state, "2", "Other", &dir, "../escape").is_err());
        assert!(validate(&state, "2", "Other", &dir, "{unknown}").is_err());
    }
    #[test]
    fn collision_does_not_overwrite() {
        let dir = std::env::temp_dir().join(format!("vps-test-{}", std::process::id()));
        fs::create_dir_all(&dir).expect("folder");
        let mut p = Printer {
            id: "1".into(),
            name: "Test".into(),
            folder: dir.clone(),
            pattern: "test".into(),
            enabled: true,
            count: 0,
            ..Printer::default()
        };
        let first = test_pdf(&mut p).expect("first");
        let second = test_pdf(&mut p).expect("second");
        assert_ne!(first, second);
        assert_eq!(p.count, 2);
        fs::remove_dir_all(dir).expect("cleanup");
    }
}

#[cfg(test)]
mod persistence_tests {
    use super::*;
    #[test]
    fn persists_settings_and_preserves_previous_state() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("state.json");
        let mut state = State::default();
        save(&path, &state).expect("initial save");
        state.dark = true;
        save(&path, &state).expect("second save");
        let loaded: State = serde_json::from_slice(&fs::read(&path).expect("read")).expect("state");
        assert!(loaded.dark);
        let previous: State =
            serde_json::from_slice(&fs::read(path.with_extension("json.bak")).expect("backup"))
                .expect("old state");
        assert!(!previous.dark);
    }
}

pub mod delivery;
pub use delivery::*;

/// Recover from the previous atomic commit without discarding the damaged file.
pub fn load_state(path: &Path) -> Result<(State, Option<String>)> {
    if !path.exists() {
        return Ok((State::default(), None));
    }
    let parse = |p: &Path| -> Result<State> {
        let json: serde_json::Value = serde_json::from_slice(&fs::read(p)?)?;
        let mut state: State = serde_json::from_value(json.clone())?;
        if json.get("theme").is_none() {
            state.theme = if state.dark { 1 } else { 0 };
        }
        Ok(state)
    };
    match parse(path) {
        Ok(state) => Ok((state, None)),
        Err(original) => {
            let backup = path.with_extension("json.bak");
            let state = parse(&backup).with_context(|| {
                format!(
                    "Settings and recovery copy could not be read at {}: {original}",
                    path.display()
                )
            })?;
            let corrupt = path.with_extension(format!(
                "corrupt-{}",
                time::OffsetDateTime::now_utc().unix_timestamp_nanos()
            ));
            fs::rename(path, &corrupt)?;
            fs::copy(&backup, path)?;
            Ok((
                state,
                Some(format!(
                    "Recovered your settings from the previous saved copy. The damaged file was kept at {}.",
                    corrupt.display()
                )),
            ))
        }
    }
}
#[cfg(test)]
mod recovery_tests {
    use super::*;
    #[test]
    fn damaged_primary_recovers_without_destroying_evidence() {
        let dir = tempfile::tempdir().expect("folder");
        let path = dir.path().join("state.json");
        let original = State {
            welcomed: true,
            ..State::default()
        };
        save(&path, &original).expect("initial");
        save(&path, &original).expect("backup");
        fs::write(&path, b"broken JSON").expect("corrupt");
        let (state, message) = load_state(&path).expect("recover");
        assert!(state.welcomed);
        assert!(message.is_some());
        assert_eq!(fs::read_dir(dir.path()).expect("files").count(), 3);
    }
}

pub mod lpr;
