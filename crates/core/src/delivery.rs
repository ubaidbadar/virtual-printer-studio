use crate::{Printer, State, validate};
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Seek, Write},
    path::{Path, PathBuf},
};

pub const CAPTURE_BACKEND: &str = "Windows Standard TCP/IP (LPR)";
pub const PORT_PREFIX: &str = "VPS_";
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JobMetadata {
    pub id: String,
    pub printer_id: String,
    pub title: String,
    pub created_unix: i64,
}
#[derive(Serialize, Deserialize)]
struct Receipt {
    path: PathBuf,
    digest: String,
}
pub fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 80 && id.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-')
}
pub fn safe_part(text: &str) -> String {
    let cleaned: String = text
        .chars()
        .take(100)
        .map(|c| {
            if c.is_control() || "<>:\"/\\|?*".contains(c) {
                '_'
            } else {
                c
            }
        })
        .collect();
    let trimmed = cleaned.trim().trim_end_matches(['.', ' ']);
    let stem = trimmed.split('.').next().unwrap_or("").to_ascii_uppercase();
    let reserved = [
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
        "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];
    if trimmed.is_empty() {
        "Document".into()
    } else if reserved.contains(&stem.as_str()) {
        format!("_{trimmed}")
    } else {
        trimmed.into()
    }
}
fn digest_file(path: &Path) -> Result<String> {
    let mut file = fs::File::open(path)?;
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 65536];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        digest.update(&buffer[..n]);
    }
    Ok(digest
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}
fn atomic_json(path: &Path, value: &impl Serialize) -> Result<()> {
    let parent = path.parent().context("Missing parent folder")?;
    fs::create_dir_all(parent)?;
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    file.write_all(&serde_json::to_vec(value)?)?;
    file.as_file().sync_all()?;
    file.persist(path).map_err(|e| e.error)?;
    Ok(())
}
/// A durable receipt allows a delivery to resume after a crash between publishing a
/// PDF and committing application state. Publication never overwrites a document.
pub fn deliver_pdf(
    printer: &Printer,
    job: &JobMetadata,
    source: &Path,
    receipt_path: &Path,
) -> Result<PathBuf> {
    ensure!(
        valid_id(&job.id) && valid_id(&job.printer_id),
        "Invalid captured job identifier"
    );
    ensure!(
        job.printer_id == printer.id,
        "Captured job belongs to another printer"
    );
    ensure!(
        printer.enabled,
        "Printer is paused. Resume it to deliver queued documents."
    );
    validate(
        &State::default(),
        "",
        &printer.name,
        &printer.folder,
        &printer.pattern,
    )?;
    let mut file = fs::File::open(source).context("Captured PDF is unavailable")?;
    let mut magic = [0; 5];
    file.read_exact(&mut magic)
        .context("The captured document is incomplete")?;
    ensure!(
        &magic == b"%PDF-",
        "The Windows driver did not produce a PDF. Repair this printer before retrying."
    );
    let digest = digest_file(source)?;
    if receipt_path.exists() {
        let receipt: Receipt = serde_json::from_slice(&fs::read(receipt_path)?)?;
        ensure!(
            receipt.digest == digest,
            "Captured data changed after delivery began"
        );
        if receipt.path.exists() && digest_file(&receipt.path)? == digest {
            return Ok(receipt.path);
        }
        // The previous target may have been removed or changed by the user. Pick
        // a fresh name; never replace a file whose content does not match.
    }
    let utc = time::OffsetDateTime::from_unix_timestamp(job.created_unix)?;
    let now = utc.to_offset(time::UtcOffset::local_offset_at(utc).unwrap_or(time::UtcOffset::UTC));
    let date = format!(
        "{:04}-{:02}-{:02}",
        now.year(),
        u8::from(now.month()),
        now.day()
    );
    let clock = format!("{:02}-{:02}-{:02}", now.hour(), now.minute(), now.second());
    let base = safe_part(
        &printer
            .pattern
            .replace("{title}", &safe_part(&job.title))
            .replace("{date}", &date)
            .replace("{time}", &clock)
            .replace("{counter}", &printer.count.saturating_add(1).to_string()),
    );
    let folder = if printer.monthly_folders {
        printer
            .folder
            .join(format!("{:04}-{:02}", now.year(), u8::from(now.month())))
    } else {
        printer.folder.clone()
    };
    fs::create_dir_all(&folder)?;
    for suffix in 0..10000 {
        let name = if suffix == 0 {
            format!("{base}.pdf")
        } else {
            format!("{base}_{suffix}.pdf")
        };
        let destination = folder.join(name);
        if destination.exists() {
            continue;
        }
        atomic_json(
            receipt_path,
            &Receipt {
                path: destination.clone(),
                digest: digest.clone(),
            },
        )?;
        let mut temp = tempfile::NamedTempFile::new_in(&folder)?;
        file.rewind()?;
        std::io::copy(&mut file, &mut temp)?;
        temp.as_file().sync_all()?;
        match temp.persist_noclobber(&destination) {
            Ok(_) => return Ok(destination),
            Err(e) if e.error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e.error.into()),
        }
    }
    bail!("Too many documents with this filename. Change the filename pattern.")
}

pub fn lpr_queue(id: &str) -> Result<String> {
    ensure!(valid_id(id), "Invalid printer identity");
    // Keep below the inbox monitor's queue-name limit. All 128 profile-id bits
    // influence the 96-bit transport key; routing also checks for collisions.
    Ok(Sha256::digest(id.as_bytes())
        .iter()
        .take(12)
        .map(|b| format!("{b:02x}"))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (tempfile::TempDir, Printer, JobMetadata, PathBuf, PathBuf) {
        let dir = tempfile::tempdir().expect("temp folder");
        let source = dir.path().join("input.pdf");
        fs::write(&source, b"%PDF-1.4\ncontent\n%%EOF").expect("PDF");
        let output = dir.path().join("out");
        fs::create_dir(&output).expect("output");
        let p = Printer {
            id: "profile1".into(),
            name: "Invoices".into(),
            folder: output,
            pattern: "{title}_{counter}".into(),
            enabled: true,
            ..Printer::default()
        };
        let j = JobMetadata {
            id: "job1".into(),
            printer_id: p.id.clone(),
            title: "Q4: revenue / report".into(),
            created_unix: 1700000000,
        };
        let receipt = dir.path().join("receipts/job1.json");
        (dir, p, j, source, receipt)
    }
    #[test]
    fn retries_are_idempotent_and_collisions_survive() {
        let (_dir, p, j, source, receipt) = fixture();
        let first = deliver_pdf(&p, &j, &source, &receipt).expect("delivery");
        assert_eq!(
            first,
            deliver_pdf(&p, &j, &source, &receipt).expect("retry")
        );
        fs::write(&first, b"different user's file").expect("replace");
        let second = deliver_pdf(&p, &j, &source, &receipt).expect("collision");
        assert_ne!(first, second);
        assert_eq!(fs::read(first).expect("original"), b"different user's file");
    }
    #[test]
    fn rejects_non_pdf_and_cross_profile_jobs() {
        let (_dir, p, mut j, source, receipt) = fixture();
        j.printer_id = "other".into();
        assert!(deliver_pdf(&p, &j, &source, &receipt).is_err());
        j.printer_id = p.id.clone();
        fs::write(&source, b"not a PDF").expect("input");
        assert!(deliver_pdf(&p, &j, &source, &receipt).is_err());
    }
    #[test]
    fn sanitizes_titles_and_rejects_path_identifiers() {
        assert_eq!(safe_part("CON"), "_CON");
        assert_eq!(safe_part("../a:b\\c"), ".._a_b_c");
        assert!(!valid_id("../escape"));
        assert!(!valid_id(""));
    }
}
