use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};
#[derive(Clone, Serialize, Deserialize)]
pub struct Printer {
    pub id: String,
    pub name: String,
    pub folder: PathBuf,
    pub pattern: String,
    pub enabled: bool,
    pub count: u64,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Activity {
    pub printer: String,
    pub file: String,
    pub status: String,
    pub path: String,
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct State {
    pub printers: Vec<Printer>,
    pub activity: Vec<Activity>,
    pub dark: bool,
    pub default_folder: String,
    pub welcomed: bool,
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
