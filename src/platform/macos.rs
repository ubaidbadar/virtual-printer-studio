//! Native CUPS queues. Privilege is limited to installing the shared filter and
//! changing CUPS configuration; PDF delivery always runs as the signed-in user.
use super::*;
use anyhow::{Context, ensure};
use std::{
    fs,
    io::{Read, Write},
    os::unix::{
        fs::{MetadataExt, PermissionsExt},
        net::UnixStream,
    },
    process::Command,
};

const FILTER: &str = "/Library/Printers/VirtualPrinterStudio/vps-cups-filter";
const CONFIG: &str = "/Library/Printers/VirtualPrinterStudio/queues";
unsafe extern "C" {
    fn getuid() -> u32;
    fn getpeereid(fd: i32, uid: *mut u32, gid: *mut u32) -> i32;
}
fn queue(printer: &Printer) -> Result<String> {
    ensure!(vps_core::valid_id(&printer.id), "Invalid printer identity");
    Ok(format!("VPS_{}", printer.id))
}
fn uid() -> u32 {
    // SAFETY: getuid takes no pointers and returns the process real UID.
    unsafe { getuid() }
}
fn peer_uid(stream: &UnixStream) -> Result<u32> {
    use std::os::fd::AsRawFd;
    let (mut user, mut group) = (0, 0);
    // SAFETY: the live socket FD and writable u32 outputs match getpeereid's ABI.
    ensure!(
        unsafe { getpeereid(stream.as_raw_fd(), &mut user, &mut group) } == 0,
        "Could not authenticate the local print connection"
    );
    Ok(user)
}
fn shell(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}
fn apple(value: &str) -> String {
    format!(
        "\"{}\"",
        value
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
    )
}
fn checked(output: std::process::Output) -> Result<()> {
    ensure!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr).trim()
    );
    Ok(())
}
fn administrator(script: &str) -> Result<()> {
    if uid() == 0 {
        return checked(Command::new("/bin/sh").args(["-c", script]).output()?);
    }
    // Respect an existing, noninteractive sudo grant (including CI). Otherwise
    // macOS presents its own authorization sheet; no Terminal is opened.
    if Command::new("/usr/bin/sudo")
        .args(["-n", "/usr/bin/true"])
        .output()?
        .status
        .success()
    {
        return checked(
            Command::new("/usr/bin/sudo")
                .args(["-n", "/bin/sh", "-c", script])
                .output()?,
        );
    }
    checked(
        Command::new("/usr/bin/osascript")
            .args([
                "-e",
                &format!(
                    "do shell script {} with administrator privileges",
                    apple(script)
                ),
            ])
            .output()?,
    )
}
#[derive(serde::Serialize, serde::Deserialize)]
struct QueueConfig {
    user: u32,
    socket: PathBuf,
    route: String,
}
pub fn native_action(action: &str, printer: &Printer, _port: u16, data: &Path) -> Result<()> {
    ensure!(
        ["ensure", "pause", "remove"].contains(&action),
        "Unknown printer action"
    );
    let name = queue(printer)?;
    let name_q = shell(&name);
    let jobs = Command::new("/usr/bin/lpstat")
        .args(["-W", "not-completed", "-o", &name])
        .output()?;
    if action == "remove" {
        ensure!(
            jobs.stdout.is_empty(),
            "This printer still has queued jobs. Resume or cancel them before deleting it."
        );
        return administrator(&format!(
            "set -eu\n/usr/sbin/lpadmin -x {name_q}\n/bin/rm -f {}/{}.json",
            shell(CONFIG),
            name
        ));
    }
    let paused = if printer.enabled {
        "/usr/sbin/cupsenable"
    } else {
        "/usr/sbin/cupsdisable"
    };
    if action == "pause" {
        let output = Command::new(paused).arg(&name).output()?;
        if output.status.success() {
            return Ok(());
        }
        return administrator(&format!("{} {}", paused, name_q));
    }
    let helper = std::env::current_exe()?
        .parent()
        .context("Application executable has no folder")?
        .parent()
        .context("Application bundle is incomplete")?
        .join("Helpers/vps-cups-filter");
    ensure!(
        helper.is_file(),
        "The macOS app bundle is incomplete. Reinstall Virtual Printer Studio.app."
    );
    let staging = tempfile::tempdir()?;
    let ppd = staging.path().join("VirtualPrinterStudio.ppd");
    fs::write(
        &ppd,
        include_str!("../../packaging/macos/VirtualPrinterStudio.ppd"),
    )?;
    let config = staging.path().join("queue.json");
    fs::write(
        &config,
        serde_json::to_vec(&QueueConfig {
            user: uid(),
            socket: capture_socket(data)?,
            route: vps_core::lpr_queue(&printer.id)?,
        })?,
    )?;
    let user = Command::new("/usr/bin/id").args(["-un"]).output()?;
    ensure!(user.status.success(), "Cannot determine your macOS account");
    let user = String::from_utf8(user.stdout)?.trim().to_owned();
    // All executable/configuration destinations are root owned. The CUPS filter
    // has no permission to modify them or write to the user's output folder.
    administrator(&format!(
        "set -eu\n/usr/bin/install -d -o root -g wheel -m 755 /Library/Printers/VirtualPrinterStudio\n/usr/bin/install -d -o root -g _lp -m 750 {config_root}\n/usr/bin/install -o root -g wheel -m 755 {helper} {filter}\n/usr/bin/install -o root -g _lp -m 640 {config} {config_root}/{name}.json\n/usr/sbin/lpadmin -p {name_q} -D {display} -v file:///dev/null -P {ppd} -o printer-is-shared=false -o printer-error-policy=retry-job -u {allow} -E\n{paused} {name_q}",
        config_root = shell(CONFIG),
        helper = shell(&helper.to_string_lossy()),
        filter = shell(FILTER),
        config = shell(&config.to_string_lossy()),
        display = shell(&printer.name),
        ppd = shell(&ppd.to_string_lossy()),
        allow = shell(&format!("allow:{user},root"))
    ))
}
fn ack(stream: &mut UnixStream) -> Result<()> {
    let mut byte = [0];
    stream.read_exact(&mut byte)?;
    ensure!(byte[0] == 0, "The PDF receiver rejected this print job");
    Ok(())
}
/// Executed by CUPS from a root-owned copy, before constructing any GUI object.
pub fn cups_filter() -> Result<()> {
    let args: Vec<_> = std::env::args().collect();
    ensure!(
        args.len() == 6 || args.len() == 7,
        "Invalid CUPS filter arguments"
    );
    let printer = std::env::var("PRINTER")?;
    ensure!(
        printer.strip_prefix("VPS_").is_some_and(vps_core::valid_id),
        "Invalid CUPS queue"
    );
    let config_path = Path::new(CONFIG).join(format!("{printer}.json"));
    let metadata = fs::symlink_metadata(&config_path)?;
    ensure!(
        metadata.is_file() && metadata.uid() == 0 && metadata.mode() & 0o022 == 0,
        "Untrusted CUPS queue configuration"
    );
    let config: QueueConfig = serde_json::from_slice(&fs::read(config_path)?)?;
    let mut stream = UnixStream::connect(&config.socket)
        .context("Open Virtual Printer Studio to receive this print job")?;
    stream.set_read_timeout(Some(std::time::Duration::from_secs(120)))?;
    stream.set_write_timeout(Some(std::time::Duration::from_secs(120)))?;
    ensure!(
        peer_uid(&stream)? == config.user,
        "The PDF receiver belongs to another account"
    );
    // CUPS may supply stdin rather than a filename. Spool to a bounded temporary
    // file so the transport can announce the exact size without loading it in RAM.
    let mut input: Box<dyn Read> = if let Some(file) = args.get(6) {
        Box::new(fs::File::open(file)?)
    } else {
        Box::new(std::io::stdin())
    };
    let mut pdf = tempfile::tempfile()?;
    let size = std::io::copy(
        &mut input.by_ref().take(10 * 1024 * 1024 * 1024 + 1),
        &mut pdf,
    )?;
    ensure!(
        size > 0 && size <= 10 * 1024 * 1024 * 1024,
        "The PDF exceeds the supported print job size"
    );
    use std::io::{Seek, SeekFrom};
    pdf.seek(SeekFrom::Start(0))?;
    let title: String = args[3]
        .chars()
        .filter(|c| !c.is_control())
        .take(1024)
        .collect();
    let job: String = args[1]
        .chars()
        .filter(char::is_ascii_digit)
        .take(20)
        .collect();
    ensure!(!job.is_empty(), "Missing CUPS job number");
    let control = format!(
        "Hlocalhost\nP{}\nJ{title}\nldfA{job}localhost\nN{title}\n",
        config.user
    );
    stream.write_all(format!("\x02{}\n", config.route).as_bytes())?;
    ack(&mut stream)?;
    stream.write_all(format!("\x02{} cfA{job}localhost\n", control.len()).as_bytes())?;
    ack(&mut stream)?;
    stream.write_all(control.as_bytes())?;
    stream.write_all(&[0])?;
    ack(&mut stream)?;
    stream.write_all(format!("\x03{size} dfA{job}localhost\n").as_bytes())?;
    ack(&mut stream)?;
    std::io::copy(&mut pdf, &mut stream)?;
    stream.write_all(&[0])?;
    ack(&mut stream)?;
    Ok(())
}
pub fn print_test(printer: &Printer) -> Result<()> {
    let temp = tempfile::tempdir()?;
    let mut fixture = printer.clone();
    fixture.folder = temp.path().to_owned();
    fixture.monthly_folders = false;
    let pdf = vps_core::test_pdf(&mut fixture)?;
    checked(
        Command::new("/usr/bin/lp")
            .args([
                "-d",
                &queue(printer)?,
                "-t",
                "Virtual Printer Studio test page",
            ])
            .arg(pdf)
            .output()?,
    )
}
pub fn health(printer: &Printer) -> Result<PrinterHealth> {
    checked(
        Command::new("/usr/bin/lpstat")
            .args(["-p", &queue(printer)?])
            .output()?,
    )?;
    Ok(PrinterHealth {
        write: true,
        service: true,
        free_bytes: None,
    })
}
pub fn startup(enabled: bool) -> Result<()> {
    let home = std::env::var_os("HOME").context("Cannot locate your home folder")?;
    let dir = PathBuf::from(home).join("Library/LaunchAgents");
    fs::create_dir_all(&dir)?;
    let file = dir.join("studio.virtualprinterstudio.login.plist");
    let uid = uid().to_string();
    if file.exists() {
        let _ = Command::new("/bin/launchctl")
            .args(["bootout", &format!("gui/{uid}")])
            .arg(&file)
            .output();
        fs::remove_file(&file)?;
    }
    if enabled {
        let exe = std::env::current_exe()?
            .to_string_lossy()
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;");
        fs::write(
            &file,
            format!(
                "<?xml version=\"1.0\"?><!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\"><plist version=\"1.0\"><dict><key>Label</key><string>studio.virtualprinterstudio.login</string><key>ProgramArguments</key><array><string>{exe}</string><string>--background</string></array><key>RunAtLoad</key><true/></dict></plist>"
            ),
        )?;
        // Load at the next login. Loading immediately would launch a second app.
    }
    Ok(())
}
pub fn system_dark() -> bool {
    Command::new("/usr/bin/defaults")
        .args(["read", "-g", "AppleInterfaceStyle"])
        .output()
        .is_ok_and(|o| o.status.success() && String::from_utf8_lossy(&o.stdout).trim() == "Dark")
}
pub fn notify_saved(path: &Path, printer: &str) -> Result<()> {
    checked(
        Command::new("/usr/bin/osascript")
            .args([
                "-e",
                &format!(
                    "display notification {} with title {}",
                    apple(&format!(
                        "{} · {}",
                        printer,
                        path.file_name().unwrap_or_default().to_string_lossy()
                    )),
                    apple("PDF saved")
                ),
            ])
            .output()?,
    )
}
pub fn printer_cli(args: &[String]) -> Result<()> {
    let value = |key: &str| -> Result<&str> {
        let i = args
            .iter()
            .position(|a| a == key)
            .context("Missing printer command argument")?;
        Ok(args
            .get(i + 1)
            .context("Missing printer command value")?
            .as_str())
    };
    let path = Path::new(value("--state")?);
    let (mut state, _) = vps_core::load_state(path)?;
    let action = value("--macos-printer-action")?;
    let p = state
        .printers
        .iter_mut()
        .find(|p| p.id == value("--printer").unwrap_or(""))
        .context("Printer not found")?;
    match action {
        "test" => print_test(p)?,
        "pause" | "resume" => {
            p.enabled = action == "resume";
            native_action(
                "pause",
                p,
                0,
                path.parent().context("Missing state folder")?,
            )?;
        }
        "ensure" | "remove" => {
            native_action(action, p, 0, path.parent().context("Missing state folder")?)?;
            p.installed = action == "ensure";
        }
        _ => anyhow::bail!("Unknown printer action"),
    }
    vps_core::save(path, &state)?;
    Ok(())
}

pub fn capture_socket(data: &Path) -> Result<PathBuf> {
    use sha2::{Digest, Sha256};
    let data = std::fs::canonicalize(data)?;
    let digest = Sha256::digest(data.as_os_str().as_encoded_bytes());
    let tag: String = digest[..12].iter().map(|b| format!("{b:02x}")).collect();
    let dir = PathBuf::from(format!("/private/tmp/vps-{}-{tag}", uid()));
    match fs::create_dir(&dir) {
        Ok(()) => (),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => (),
        Err(e) => return Err(e.into()),
    }
    let meta = fs::symlink_metadata(&dir)?;
    ensure!(
        meta.is_dir() && !meta.file_type().is_symlink() && meta.uid() == uid(),
        "The local print socket folder belongs to another account"
    );
    // Only the owning account can create/replace entries. CUPS may traverse it;
    // getpeereid then authenticates both ends before any document is sent.
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o711))?;
    Ok(dir.join("capture.sock"))
}
pub fn authorize_unix(stream: &UnixStream) -> Result<()> {
    let peer = peer_uid(stream)?;
    let lp = Command::new("/usr/bin/id").args(["-u", "_lp"]).output()?;
    ensure!(
        lp.status.success(),
        "Cannot identify the macOS printing account"
    );
    let lp: u32 = String::from_utf8(lp.stdout)?.trim().parse()?;
    ensure!(
        peer == 0 || peer == lp,
        "Only the macOS print service may send captured PDFs"
    );
    Ok(())
}
