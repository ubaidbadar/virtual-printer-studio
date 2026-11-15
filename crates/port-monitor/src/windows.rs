//! Windows spooler ABI. All raw pointers here are supplied by the spooler and are
//! checked before use. Handles returned by OpenPort own exactly one boxed mutex.
//! Captured files are SYSTEM-owned; the desktop receives read access, never write
//! access. A separate acknowledgement directory contains no privileged writes.
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    mem::size_of,
    panic::{AssertUnwindSafe, catch_unwind},
    path::PathBuf,
    ptr,
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};
use vps_core::{JobMetadata, MONITOR_NAME, PORT_PREFIX, valid_id};
use windows_sys::{
    Win32::{Foundation::*, Graphics::Printing::*},
    core::{BOOL, PCWSTR, PWSTR},
};

type MResult<T> = Result<T, u32>;
static SEQUENCE: AtomicU64 = AtomicU64::new(0);
fn root() -> PathBuf {
    PathBuf::from(std::env::var_os("ProgramData").unwrap_or_else(|| "C:\\ProgramData".into()))
        .join("VirtualPrinterStudio")
}
fn io_error(e: std::io::Error) -> u32 {
    e.raw_os_error()
        .map(|e| e as u32)
        .unwrap_or(ERROR_WRITE_FAULT)
}
struct SpoolerIdentity(HANDLE);
impl SpoolerIdentity {
    fn enter() -> Self {
        // Safety: this is called only inside spooler callbacks, on their calling
        // thread; the token is restored on that same thread before returning.
        Self(unsafe { RevertToPrinterSelf() })
    }
}
impl Drop for SpoolerIdentity {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // Safety: token was returned by RevertToPrinterSelf in this callback;
            // ImpersonatePrinterClient restores impersonation and closes it.
            unsafe { ImpersonatePrinterClient(self.0) };
        }
    }
}
fn boundary(f: impl FnOnce() -> MResult<()>) -> BOOL {
    let identity = SpoolerIdentity::enter();
    let outcome = catch_unwind(AssertUnwindSafe(f));
    let error = match outcome {
        Ok(Ok(())) => {
            drop(identity);
            return 1;
        }
        Ok(Err(e)) => e,
        _ => ERROR_GEN_FAILURE,
    };
    if let Ok(mut log) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(root().join("monitor.log"))
    {
        let _ = writeln!(log, "Port monitor error {error}");
    }
    drop(identity);
    // Safety: SetLastError has no pointer preconditions. Set it after restoring
    // impersonation so callers receive the actual operation's error.
    unsafe { SetLastError(error) };
    0
}
unsafe fn wide(p: PCWSTR) -> MResult<String> {
    if p.is_null() {
        return Err(ERROR_INVALID_PARAMETER);
    }
    for n in 0..32768 {
        // Safety: spooler supplies a null-terminated UTF-16 string; bound limits scanning.
        if unsafe { *p.add(n) } == 0 {
            return Ok(String::from_utf16_lossy(unsafe {
                std::slice::from_raw_parts(p, n)
            }));
        }
    }
    Err(ERROR_INVALID_PARAMETER)
}
fn id_from_port(port: &str) -> MResult<&str> {
    let id = port.strip_prefix(PORT_PREFIX).ok_or(ERROR_INVALID_NAME)?;
    if valid_id(id) {
        Ok(id)
    } else {
        Err(ERROR_INVALID_NAME)
    }
}
fn port_names() -> MResult<Vec<String>> {
    let mut ports = vec![];
    for entry in fs::read_dir(root().join("ports")).map_err(io_error)? {
        let entry = entry.map_err(io_error)?;
        let name = entry.file_name().to_string_lossy().to_string();
        if entry.file_type().map_err(io_error)?.is_file() && id_from_port(&name).is_ok() {
            ports.push(name);
        }
    }
    ports.sort();
    Ok(ports)
}
fn add(port: &str) -> MResult<()> {
    id_from_port(port)?;
    OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(root().join("ports").join(port))
        .map_err(io_error)?
        .sync_all()
        .map_err(io_error)
}
fn remove(port: &str) -> MResult<()> {
    id_from_port(port)?;
    fs::remove_file(root().join("ports").join(port)).map_err(io_error)
}
fn clean_acknowledged(id: &str) {
    let folder = root().join("jobs").join(id);
    if let Ok(entries) = fs::read_dir(folder) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if let Some(job) = name.strip_suffix(".ready")
                && valid_id(job)
                && root()
                    .join("acks")
                    .join(id)
                    .join(format!("{job}.ack"))
                    .is_file()
            {
                // Only SYSTEM/admin can alter the jobs tree, so this path cannot
                // be redirected through a user-created junction or symlink.
                let _ = fs::remove_dir_all(entry.path());
            }
        }
    }
}
struct Capture {
    file: File,
    partial: PathBuf,
    ready: PathBuf,
    printer: PRINTER_HANDLE,
    job_id: u32,
}
impl Drop for Capture {
    fn drop(&mut self) {
        if !self.printer.Value.is_null() {
            // Safety: printer was opened by OpenPrinterW and is closed once here.
            unsafe { ClosePrinter(self.printer) };
        }
    }
}
struct Port {
    id: String,
    capture: Option<Capture>,
}
unsafe fn context<'a>(h: HANDLE) -> MResult<&'a Mutex<Port>> {
    if h.is_null() {
        return Err(ERROR_INVALID_HANDLE);
    }
    // Safety: only handles allocated by open_port are accepted by the spooler.
    Ok(unsafe { &*h.cast::<Mutex<Port>>() })
}
unsafe extern "system" fn open_port(_monitor: HANDLE, name: PCWSTR, out: *mut HANDLE) -> BOOL {
    boundary(|| {
        if out.is_null() {
            return Err(ERROR_INVALID_PARAMETER);
        }
        // Safety: checked spooler output pointer and input string.
        let port = unsafe { wide(name) }?;
        let id = id_from_port(&port)?.to_owned();
        if !root().join("ports").join(&port).is_file() {
            return Err(ERROR_UNKNOWN_PORT);
        }
        unsafe { *out = Box::into_raw(Box::new(Mutex::new(Port { id, capture: None }))).cast() };
        Ok(())
    })
}
unsafe extern "system" fn start_doc(
    h: HANDLE,
    printer_name: PCWSTR,
    job_id: u32,
    level: u32,
    data: *const u8,
) -> BOOL {
    boundary(|| {
        if level != 1 || data.is_null() {
            return Err(ERROR_INVALID_LEVEL);
        }
        // Safety: DOC_INFO_1W is provided by the spooler for level 1.
        let doc = unsafe { ptr::read_unaligned(data.cast::<DOC_INFO_1W>()) };
        let title = if doc.pDocName.is_null() {
            "Document".into()
        } else {
            unsafe { wide(doc.pDocName) }?
        };
        let mut port = unsafe { context(h) }?
            .lock()
            .map_err(|_| ERROR_GEN_FAILURE)?;
        if port.capture.is_some() {
            return Err(ERROR_BUSY);
        }
        clean_acknowledged(&port.id);
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| ERROR_GEN_FAILURE)?;
        let unique = format!(
            "{}-{}-{}",
            job_id,
            now.as_nanos(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        );
        let dir = root().join("jobs").join(&port.id);
        let partial = dir.join(format!("{unique}.partial"));
        let ready = dir.join(format!("{unique}.ready"));
        fs::create_dir(&partial).map_err(io_error)?;
        let metadata = JobMetadata {
            id: unique,
            printer_id: port.id.clone(),
            title,
            created_unix: now.as_secs() as i64,
        };
        let mut meta = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(partial.join("job.json"))
            .map_err(io_error)?;
        meta.write_all(&serde_json::to_vec(&metadata).map_err(|_| ERROR_INVALID_DATA)?)
            .map_err(io_error)?;
        meta.sync_all().map_err(io_error)?;
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(partial.join("document.pdf"))
            .map_err(io_error)?;
        let mut printer = PRINTER_HANDLE::default();
        // Safety: supplied printer name is valid for the duration of this call;
        // PRINTER_HANDLE output is initialized and owned by Capture after success.
        if unsafe { OpenPrinterW(printer_name, &mut printer, ptr::null()) } == 0 {
            return Err(unsafe { GetLastError() });
        }
        port.capture = Some(Capture {
            file,
            partial,
            ready,
            printer,
            job_id,
        });
        Ok(())
    })
}
unsafe extern "system" fn write_port(
    h: HANDLE,
    buffer: *const u8,
    len: u32,
    written: *mut u32,
) -> BOOL {
    boundary(|| {
        if written.is_null() || (buffer.is_null() && len != 0) {
            return Err(ERROR_INVALID_PARAMETER);
        }
        // Safety: checked output pointer; spooler owns a buffer of len bytes.
        unsafe { *written = 0 };
        let mut port = unsafe { context(h) }?
            .lock()
            .map_err(|_| ERROR_GEN_FAILURE)?;
        let capture = port.capture.as_mut().ok_or(ERROR_INVALID_HANDLE)?;
        if len > 0 {
            capture
                .file
                .write_all(unsafe { std::slice::from_raw_parts(buffer, len as usize) })
                .map_err(io_error)?;
        }
        unsafe { *written = len };
        Ok(())
    })
}
unsafe extern "system" fn end_doc(h: HANDLE) -> BOOL {
    boundary(|| {
        let mut port = unsafe { context(h) }?
            .lock()
            .map_err(|_| ERROR_GEN_FAILURE)?;
        let capture = port.capture.take().ok_or(ERROR_INVALID_HANDLE)?;
        capture.file.sync_all().map_err(io_error)?;
        // The open File permits renaming its containing directory on Windows via
        // Rust's default FILE_SHARE_DELETE. No .ready job is visible until flushed.
        fs::rename(&capture.partial, &capture.ready).map_err(io_error)?;
        // Safety: handle and job id belong to this capture. Once durably captured,
        // the print job is sent; delivery remains recoverable independently of UI.
        unsafe {
            SetJobW(
                capture.printer,
                capture.job_id,
                0,
                ptr::null(),
                JOB_CONTROL_SENT_TO_PRINTER,
            )
        };
        Ok(())
    })
}
unsafe extern "system" fn close_port(h: HANDLE) -> BOOL {
    boundary(|| {
        if h.is_null() {
            return Err(ERROR_INVALID_HANDLE);
        }
        // Safety: ClosePort is the unique release of the Box returned by OpenPort.
        drop(unsafe { Box::from_raw(h.cast::<Mutex<Port>>()) });
        Ok(())
    })
}
unsafe extern "system" fn read_port(
    _h: HANDLE,
    _buffer: *mut u8,
    _len: u32,
    read: *mut u32,
) -> BOOL {
    boundary(|| {
        if !read.is_null() {
            unsafe { *read = 0 };
        }
        Err(ERROR_NOT_SUPPORTED)
    })
}
unsafe extern "system" fn add_port_ex(
    _h: HANDLE,
    _server: PCWSTR,
    level: u32,
    data: *const u8,
    _monitor: PCWSTR,
) -> BOOL {
    boundary(|| {
        if level != 1 || data.is_null() {
            return Err(ERROR_INVALID_LEVEL);
        }
        // Safety: AddPortEx level 1 supplies PORT_INFO_1W.
        let port = unsafe { ptr::read_unaligned(data.cast::<PORT_INFO_1W>()) };
        add(&unsafe { wide(port.pName) }?)
    })
}
unsafe extern "system" fn delete_port(
    _h: HANDLE,
    _server: PCWSTR,
    _window: HWND,
    name: PCWSTR,
) -> BOOL {
    boundary(|| remove(&unsafe { wide(name) }?))
}
unsafe fn put_string(buffer: *mut u8, offset: &mut usize, text: &str) -> PWSTR {
    let utf: Vec<u16> = text.encode_utf16().chain(Some(0)).collect();
    // Safety: EnumPorts computed a sufficient output buffer including all strings.
    *offset -= utf.len() * 2;
    let location = unsafe { buffer.add(*offset).cast::<u16>() };
    unsafe { ptr::copy_nonoverlapping(utf.as_ptr(), location, utf.len()) };
    location
}
unsafe extern "system" fn enum_ports(
    _h: HANDLE,
    _server: PCWSTR,
    level: u32,
    buffer: *mut u8,
    len: u32,
    needed: *mut u32,
    returned: *mut u32,
) -> BOOL {
    boundary(|| {
        if needed.is_null() || returned.is_null() {
            return Err(ERROR_INVALID_PARAMETER);
        }
        unsafe { *returned = 0 };
        let ports = port_names()?;
        let item = match level {
            1 => size_of::<PORT_INFO_1W>(),
            2 => size_of::<PORT_INFO_2W>(),
            _ => return Err(ERROR_INVALID_LEVEL),
        };
        let strings: usize = ports
            .iter()
            .map(|name| {
                (name.encode_utf16().count() + 1) * 2
                    + if level == 2 {
                        (MONITOR_NAME.encode_utf16().count() + 1) * 2
                            + ("PDF capture port".len() + 1) * 2
                    } else {
                        0
                    }
            })
            .sum();
        let bytes = item * ports.len() + strings;
        unsafe { *needed = bytes.try_into().map_err(|_| ERROR_NOT_ENOUGH_MEMORY)? };
        if (len as usize) < bytes || (buffer.is_null() && bytes > 0) {
            return Err(ERROR_INSUFFICIENT_BUFFER);
        }
        // The spooler merges monitors into one buffer, advancing the front by
        // structure count and reserving strings at the end. Packing strings
        // immediately after our structures lets the next monitor overwrite them.
        let mut offset = len as usize & !1;
        for (i, name) in ports.iter().enumerate() {
            let pname = unsafe { put_string(buffer, &mut offset, name) };
            if level == 1 {
                unsafe {
                    ptr::write_unaligned(
                        buffer.add(i * item).cast::<PORT_INFO_1W>(),
                        PORT_INFO_1W { pName: pname },
                    )
                };
            } else {
                let monitor = unsafe { put_string(buffer, &mut offset, MONITOR_NAME) };
                let description = unsafe { put_string(buffer, &mut offset, "PDF capture port") };
                unsafe {
                    ptr::write_unaligned(
                        buffer.add(i * item).cast::<PORT_INFO_2W>(),
                        PORT_INFO_2W {
                            pPortName: pname,
                            pMonitorName: monitor,
                            pDescription: description,
                            fPortType: PORT_TYPE_WRITE,
                            Reserved: 0,
                        },
                    )
                };
            }
        }
        unsafe { *returned = ports.len() as u32 };
        Ok(())
    })
}
struct Xcv {
    access: u32,
}
unsafe extern "system" fn xcv_open(
    _h: HANDLE,
    _name: PCWSTR,
    access: u32,
    out: *mut HANDLE,
) -> BOOL {
    boundary(|| {
        if out.is_null() {
            return Err(ERROR_INVALID_PARAMETER);
        }
        unsafe { *out = Box::into_raw(Box::new(Xcv { access })).cast() };
        Ok(())
    })
}
unsafe extern "system" fn xcv_data(
    h: HANDLE,
    name: PCWSTR,
    input: *const u8,
    input_len: u32,
    _out: *mut u8,
    _out_len: u32,
    needed: *mut u32,
) -> u32 {
    let result = catch_unwind(AssertUnwindSafe(|| -> MResult<()> {
        if h.is_null() {
            return Err(ERROR_INVALID_HANDLE);
        }
        if !needed.is_null() {
            unsafe { *needed = 0 };
        }
        // Safety: Xcv handle is allocated in xcv_open and owned until xcv_close.
        if unsafe { (*h.cast::<Xcv>()).access } & SERVER_ACCESS_ADMINISTER == 0 {
            return Err(ERROR_ACCESS_DENIED);
        }
        let command = unsafe { wide(name) }?;
        if input.is_null() || input_len < 2 || !input_len.is_multiple_of(2) {
            return Err(ERROR_INVALID_PARAMETER);
        }
        let chars =
            unsafe { std::slice::from_raw_parts(input.cast::<u16>(), input_len as usize / 2) };
        let end = chars
            .iter()
            .position(|&c| c == 0)
            .ok_or(ERROR_INVALID_PARAMETER)?;
        let port = String::from_utf16_lossy(&chars[..end]);
        let _identity = SpoolerIdentity::enter();
        match command.as_str() {
            "AddPort" => add(&port),
            "DeletePort" => remove(&port),
            _ => Err(ERROR_NOT_SUPPORTED),
        }
    }));
    match result {
        Ok(Ok(())) => ERROR_SUCCESS,
        Ok(Err(e)) => e,
        _ => ERROR_GEN_FAILURE,
    }
}
unsafe extern "system" fn xcv_close(h: HANDLE) -> BOOL {
    boundary(|| {
        if h.is_null() {
            return Err(ERROR_INVALID_HANDLE);
        }
        unsafe { drop(Box::from_raw(h.cast::<Xcv>())) };
        Ok(())
    })
}
unsafe extern "system" fn shutdown(_h: HANDLE) {}
struct Table(MONITOR2);
// Safety: the immutable table contains only function pointers and its byte size.
unsafe impl Sync for Table {}
static TABLE: std::sync::LazyLock<Table> = std::sync::LazyLock::new(|| {
    Table(MONITOR2 {
        cbSize: size_of::<MONITOR2>() as u32,
        pfnEnumPorts: Some(enum_ports),
        pfnOpenPort: Some(open_port),
        pfnStartDocPort: Some(start_doc),
        pfnWritePort: Some(write_port),
        pfnReadPort: Some(read_port),
        pfnEndDocPort: Some(end_doc),
        pfnClosePort: Some(close_port),
        pfnAddPortEx: Some(add_port_ex),
        pfnDeletePort: Some(delete_port),
        pfnXcvOpenPort: Some(xcv_open),
        pfnXcvDataPort: Some(xcv_data),
        pfnXcvClosePort: Some(xcv_close),
        pfnShutdown: Some(shutdown),
        ..MONITOR2::default()
    })
});
/// Entry point required by AddMonitorW / Windows spoolsv.exe.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn InitializePrintMonitor2(
    _init: *const MONITORINIT,
    out: *mut HANDLE,
) -> *mut MONITOR2 {
    if out.is_null() {
        unsafe { SetLastError(ERROR_INVALID_PARAMETER) };
        return ptr::null_mut();
    }
    unsafe { *out = ptr::without_provenance_mut(1) };
    ptr::addr_of!(TABLE.0).cast_mut()
}
