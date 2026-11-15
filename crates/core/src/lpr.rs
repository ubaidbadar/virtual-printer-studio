//! RFC 1179 receive-job transport for the inbox Windows Standard TCP/IP monitor.
//! The caller authorizes each connection before acknowledging it. Byte-counted
//! files and job-name control records avoid guessing document boundaries.
use crate::{JobMetadata, valid_id};
use anyhow::{Context, Result, bail, ensure};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    fs::{self, File},
    io::{BufRead, BufReader, Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    path::{Path, PathBuf},
    sync::{
        Arc, RwLock,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    },
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
static SEQUENCE: AtomicU64 = AtomicU64::new(0);
pub type Authorize = Arc<dyn Fn(&TcpStream) -> Result<()> + Send + Sync>;
pub type Event = Arc<dyn Fn(Result<JobMetadata>) + Send + Sync>;
pub struct CaptureServer {
    address: SocketAddr,
    stop: Arc<AtomicBool>,
    join: Option<thread::JoinHandle<()>>,
}
impl CaptureServer {
    pub fn start(
        address: SocketAddr,
        root: PathBuf,
        profiles: Arc<RwLock<HashMap<String, String>>>,
        authorize: Authorize,
        event: Event,
    ) -> Result<Self> {
        ensure!(
            address.ip().is_loopback(),
            "PDF capture must listen only on loopback"
        );
        let listener=TcpListener::bind(address).with_context(||format!("Cannot start PDF capture at {address}. Another application may be using the LPR port."))?;
        let address = listener.local_addr()?;
        let stop = Arc::new(AtomicBool::new(false));
        let child_stop = stop.clone();
        let active = Arc::new(AtomicUsize::new(0));
        let join = thread::Builder::new()
            .name("lpr-capture".into())
            .spawn(move || {
                for connection in listener.incoming() {
                    if child_stop.load(Ordering::Acquire) {
                        break;
                    }
                    let stream = match connection {
                        Ok(stream) => stream,
                        Err(e) => {
                            event(Err(e.into()));
                            continue;
                        }
                    };
                    // Authentication happens before the LPR handshake. Windows waits
                    // for our ACK, so its socket still has a stable owning PID.
                    if let Err(e) = authorize(&stream) {
                        event(Err(e.context("Rejected a non-spooler capture connection")));
                        continue;
                    }
                    if active.fetch_add(1, Ordering::AcqRel) >= 64 {
                        active.fetch_sub(1, Ordering::AcqRel);
                        event(Err(anyhow::anyhow!(
                            "Too many simultaneous print connections; Windows will retry."
                        )));
                        continue;
                    }
                    let (root, profiles, event, active) = (
                        root.clone(),
                        profiles.clone(),
                        event.clone(),
                        active.clone(),
                    );
                    thread::spawn(move || {
                        let result = receive(stream, &root, &profiles);
                        if let Err(e) = &result {
                            event(Err(anyhow::anyhow!("{e:#}")));
                        } else if let Ok(Some(job)) = result {
                            event(Ok(job));
                        }
                        active.fetch_sub(1, Ordering::AcqRel);
                    });
                }
            })?;
        Ok(Self {
            address,
            stop,
            join: Some(join),
        })
    }
    pub fn address(&self) -> SocketAddr {
        self.address
    }
}
impl Drop for CaptureServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        let _ = TcpStream::connect(self.address);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}
fn line(reader: &mut BufReader<TcpStream>) -> Result<Vec<u8>> {
    let mut line = Vec::new();
    let count = reader.by_ref().take(4097).read_until(b'\n', &mut line)?;
    ensure!(
        count > 0 && line.len() <= 4096 && line.last() == Some(&b'\n'),
        "Invalid or oversized LPR command"
    );
    line.pop();
    Ok(line)
}
fn ack(reader: &mut BufReader<TcpStream>) -> Result<()> {
    reader.get_mut().write_all(&[0])?;
    reader.get_mut().flush()?;
    Ok(())
}
fn receive(
    stream: TcpStream,
    root: &Path,
    profiles: &RwLock<HashMap<String, String>>,
) -> Result<Option<JobMetadata>> {
    stream.set_read_timeout(Some(Duration::from_secs(120)))?;
    stream.set_write_timeout(Some(Duration::from_secs(120)))?;
    let mut reader = BufReader::new(stream);
    let first = line(&mut reader)?;
    ensure!(!first.is_empty(), "Empty LPR request");
    let text = std::str::from_utf8(&first[1..])?;
    let queue = text
        .split_whitespace()
        .next()
        .context("Missing LPR queue")?;
    let id = profiles
        .read()
        .map_err(|_| anyhow::anyhow!("Capture profile lock failed"))?
        .get(queue)
        .cloned()
        .context("This printer does not belong to this application instance")?;
    ensure!(valid_id(&id), "Invalid capture profile");
    if first[0] == 3 || first[0] == 4 {
        reader.get_mut().write_all(b"no entries\n")?;
        return Ok(None);
    }
    if first[0] == 1 {
        ack(&mut reader)?;
        return Ok(None);
    }
    ensure!(first[0] == 2, "Only LPR receive-job requests are accepted");
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?;
    let folder = root.join("jobs").join(&id);
    fs::create_dir_all(&folder)?;
    fs::create_dir_all(root.join("acks").join(&id))?;
    let partial = folder.join(format!(
        "capture-{}-{}.partial",
        now.as_nanos(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&partial)?;
    ack(&mut reader)?;
    let result = (|| -> Result<Option<JobMetadata>> {
        let mut control: Option<Vec<u8>> = None;
        let mut control_name = Vec::new();
        let mut data_digest = None;
        for _ in 0..3 {
            let command = line(&mut reader)?;
            if command.first() == Some(&1) {
                return Ok(None);
            }
            let code = *command.first().context("Missing LPR subcommand")?;
            ensure!(code == 2 || code == 3, "Unsupported LPR subcommand");
            let space = command
                .iter()
                .position(|&b| b == b' ')
                .context("Missing LPR file size")?;
            let size: u64 = std::str::from_utf8(&command[1..space])?.parse()?;
            ensure!(size > 0, "Empty print file");
            ensure!(
                (code != 2 || size <= 65536) && (code != 3 || size <= 10 * 1024 * 1024 * 1024),
                "Print file exceeds the capture size limit"
            );
            ensure!(
                (code != 2 || control.is_none()) && (code != 3 || data_digest.is_none()),
                "Duplicate LPR file in one print job"
            );
            ack(&mut reader)?;
            if code == 2 {
                let mut bytes = Vec::with_capacity(size as usize);
                reader.by_ref().take(size).read_to_end(&mut bytes)?;
                ensure!(bytes.len() as u64 == size, "Incomplete LPR control file");
                control = Some(bytes);
                control_name = command[space + 1..].to_vec();
            } else {
                let mut file = fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(partial.join("document.pdf"))?;
                let mut remaining = size;
                let mut buffer = [0u8; 65536];
                let mut digest = Sha256::new();
                while remaining > 0 {
                    let take = remaining.min(buffer.len() as u64) as usize;
                    let count = reader.read(&mut buffer[..take])?;
                    ensure!(
                        count > 0,
                        "The Windows print connection ended before its PDF was complete"
                    );
                    file.write_all(&buffer[..count])?;
                    digest.update(&buffer[..count]);
                    remaining -= count as u64;
                }
                file.sync_all()?;
                data_digest = Some(digest.finalize().to_vec());
            }
            let mut terminator = [0u8];
            reader.read_exact(&mut terminator)?;
            ensure!(
                terminator[0] == 0,
                "Windows reported an incomplete LPR file"
            );
            if let (Some(control), Some(digest)) = (&control, &data_digest) {
                let mut header = [0u8; 5];
                File::open(partial.join("document.pdf"))?.read_exact(&mut header)?;
                ensure!(
                    &header == b"%PDF-",
                    "The printer driver did not send a PDF. Repair this printer."
                );
                let mut identity = Sha256::new();
                identity.update(id.as_bytes());
                identity.update([0]);
                identity.update(&control_name);
                identity.update([0]);
                identity.update(control);
                identity.update(digest);
                let job_id: String = identity
                    .finalize()
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect();
                let text = String::from_utf8_lossy(control);
                let title = text
                    .lines()
                    .find_map(|line| line.strip_prefix('J'))
                    .or_else(|| text.lines().find_map(|line| line.strip_prefix('N')))
                    .unwrap_or("Document")
                    .chars()
                    .take(512)
                    .collect();
                let job = JobMetadata {
                    id: job_id.clone(),
                    printer_id: id.clone(),
                    title,
                    created_unix: now.as_secs() as i64,
                };
                let ready = folder.join(format!("{job_id}.ready"));
                let already = root
                    .join("acks")
                    .join(&id)
                    .join(format!("{job_id}.ack"))
                    .is_file()
                    || ready.is_dir();
                if !already {
                    let mut metadata = fs::OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(partial.join("job.json"))?;
                    metadata.write_all(&serde_json::to_vec(&job)?)?;
                    metadata.sync_all()?;
                    drop(metadata);
                    fs::rename(&partial, &ready)?;
                }
                if already {
                    fs::remove_dir_all(&partial)?;
                }
                // Only ACK the final LPR file after the whole job is durable.
                // A retry of the same control+data maps to the same capture id.
                ack(&mut reader)?;
                return Ok(if already { None } else { Some(job) });
            }
            ack(&mut reader)?;
        }
        bail!("LPR job did not include both control and PDF files")
    })();
    if partial.exists() {
        let _ = fs::remove_dir_all(&partial);
    }
    if result.is_err() {
        let _ = reader.get_mut().write_all(&[1]);
    }
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    fn send_job(address: SocketAddr, data_first: bool) {
        let mut stream = TcpStream::connect(address).expect("connect");
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .expect("timeout");
        let read_ack = |stream: &mut TcpStream| {
            let mut ack = [1];
            stream.read_exact(&mut ack).expect("ACK");
            assert_eq!(ack, [0]);
        };
        stream.write_all(b"\x02profile1\n").expect("request");
        read_ack(&mut stream);
        let control = b"Hlocalhost\nPowner\nJInvoice from Word\nNInvoice.pdf\n";
        let pdf = b"%PDF-1.4\nexample\n%%EOF\n";
        let parts: Vec<(u8, &[u8], &str)> = if data_first {
            vec![(3, pdf, "dfA001host"), (2, control, "cfA001host")]
        } else {
            vec![(2, control, "cfA001host"), (3, pdf, "dfA001host")]
        };
        for (code, data, name) in parts {
            stream
                .write_all(format!("{}{} {name}\n", code as char, data.len()).as_bytes())
                .expect("command");
            read_ack(&mut stream);
            stream.write_all(data).expect("data");
            stream.write_all(&[0]).expect("terminator");
            read_ack(&mut stream);
        }
    }
    #[test]
    fn framing_job_names_and_transport_retry_are_correct() {
        let dir = tempfile::tempdir().expect("folder");
        let profiles = Arc::new(RwLock::new(HashMap::from([(
            "profile1".into(),
            "profile1".into(),
        )])));
        let (tx, rx) = mpsc::channel();
        let server = CaptureServer::start(
            "127.0.0.1:0".parse().expect("address"),
            dir.path().into(),
            profiles,
            Arc::new(|_| Ok(())),
            Arc::new(move |event| {
                let _ = tx.send(event);
            }),
        )
        .expect("server");
        send_job(server.address(), true);
        let job = rx
            .recv_timeout(Duration::from_secs(5))
            .expect("event")
            .expect("capture");
        assert_eq!(job.title, "Invoice from Word");
        send_job(server.address(), false);
        assert_eq!(
            fs::read_dir(dir.path().join("jobs/profile1"))
                .expect("jobs")
                .count(),
            1
        );
        assert_eq!(
            fs::read(
                dir.path()
                    .join("jobs/profile1")
                    .join(format!("{}.ready/document.pdf", job.id))
            )
            .expect("PDF"),
            b"%PDF-1.4\nexample\n%%EOF\n"
        );
        drop(server);
    }
    #[test]
    fn refuses_non_loopback_listeners() {
        let dir = tempfile::tempdir().expect("folder");
        assert!(
            CaptureServer::start(
                "0.0.0.0:0".parse().expect("address"),
                dir.path().into(),
                Arc::new(RwLock::new(HashMap::new())),
                Arc::new(|_| Ok(())),
                Arc::new(|_| {})
            )
            .is_err()
        );
    }
}
