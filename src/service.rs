use crate::{domain::*, platform};
use anyhow::{Context, Result, ensure};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc, RwLock,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Sender},
    },
    thread,
};

#[derive(Clone)]
pub enum Command {
    Refresh,
    CaptureError(String),
    Scan,
    Retry,
    SaveProfile {
        id: String,
        name: String,
        folder: String,
        pattern: String,
        monthly: bool,
        open_after: bool,
        start_enabled: bool,
    },
    Relocate {
        id: String,
        folder: PathBuf,
    },
    Select(String),
    Toggle(String, bool),
    SetAll(bool),
    Delete(String),
    Repair(String),
    Test(String),
    Settings {
        dark: bool,
        theme: i32,
        folder: String,
        tray: bool,
        notifications: bool,
        startup: bool,
        history_days: u32,
    },
    Import(PathBuf),
    Export(PathBuf),
    Welcome,
    ClearHistory,
    Shutdown,
}
pub struct Update {
    pub state: State,
    pub message: String,
    pub editor: Option<Printer>,
    pub close_editor: bool,
    pub sync_settings: bool,
    pub queued: usize,
    pub capture_running: bool,
    pub capture_port: u16,
    pub editor_health: Option<platform::PrinterHealth>,
}
pub struct Service {
    pub sender: Sender<Command>,
    join: Option<thread::JoinHandle<()>>,
}
impl Drop for Service {
    fn drop(&mut self) {
        let _ = self.sender.send(Command::Shutdown);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}
impl Service {
    pub fn start(
        path: PathBuf,
        initial: State,
        root: PathBuf,
        sink: impl Fn(Update) + Send + 'static,
    ) -> Result<Self> {
        let (tx, rx) = mpsc::channel();
        let tx_events = tx.clone();
        let tx_actions = tx.clone();
        let join=thread::Builder::new().name("pdf-delivery".into()).spawn(move||{
            let pending=Arc::new(AtomicBool::new(false));let event_pending=pending.clone();
            let mut watcher:Option<RecommendedWatcher>=notify::recommended_watcher(move|event:notify::Result<notify::Event>|{
                if event.is_ok() && !event_pending.swap(true,Ordering::AcqRel){let _=tx_events.send(Command::Scan);}
            }).ok();
            let mut watched=HashSet::new();let mut state=initial;
            let data=path.parent().unwrap_or(Path::new(".")).to_owned();let log=data.join("logs/app.log");
            let mut failures=HashSet::new();
            let routes=Arc::new(RwLock::new(profile_routes(&state).unwrap_or_default()));
            let mut capture=match start_capture(&mut state,&path,root.clone(),routes.clone(),tx_actions.clone(),pending.clone()) {
                Ok(server)=>server,Err(e)=>{let _=tx_actions.send(Command::CaptureError(format!("{e:#}")));None}
            };
            while let Ok(command)=rx.recv(){
                if matches!(command,Command::Shutdown){break;}
                let sync_settings=matches!(&command,Command::Settings{..});
                let scan_after=matches!(&command,Command::SaveProfile{..}|Command::Relocate{..}|Command::Repair(_)|Command::Toggle(_,true)|Command::SetAll(true));
                let mut edit=None;let mut close=false;let mut editor_health=None;
                let result=(||->Result<String>{
                    match command {
                        Command::Refresh=>Ok(String::new()),
                        Command::CaptureError(message)=>Err(anyhow::anyhow!(message)),
                        Command::Scan|Command::Retry=>{
                            if matches!(command,Command::Retry){
                                failures.clear();
                                if capture.is_none(){capture=start_capture(&mut state,&path,root.clone(),routes.clone(),tx_actions.clone(),pending.clone())?;}
                            }
                            pending.store(false,Ordering::Release);
                            let delivered=scan(&mut state,&path,&root,&data,&mut failures,&log)?;
                            Ok(if delivered>0{format!("Saved {delivered} PDF(s).")}else if matches!(command,Command::Retry){"Retried all pending jobs. Check Activity for any remaining problems.".into()}else{String::new()})
                        }
                        Command::Select(id)=>{
                            let printer=find(&state,&id)?.clone();edit=Some(printer.clone());
                            editor_health=Some(platform::PrinterHealth::default());
                            validate(&state,&id,&printer.name,&printer.folder,&printer.pattern)?;
                            if let Some(health)=editor_health.as_mut(){health.write=true;}
                            #[cfg(windows)] if printer.installed { editor_health=Some(platform::health(&printer)?);return Ok("Printer health checked.".into()); }
                            Ok(if cfg!(windows){"Output folder is writable. Select Install or repair to connect this printer."}else{"Output folder is writable. System printing is available on Windows."}.into())
                        },
                        Command::Welcome=>{let mut next=state.clone();next.welcomed=true;commit(&mut state,&path,next)?;Ok(String::new())},
                        Command::SaveProfile{id,name,folder,pattern,monthly,open_after,start_enabled}=>{
                            let folder=PathBuf::from(folder);validate(&state,&id,&name,&folder,&pattern)?;
                            ensure!(name.trim().len()<=180 && !name.chars().any(|c|c.is_control()||"\\,/".contains(c)),"Use a printer name without slashes, commas or control characters (180 characters maximum).");
                            let mut next=state.clone();
                            let existing=next.printers.iter().position(|p|p.id==id);
                            let mut printer=existing.map(|i|next.printers[i].clone()).unwrap_or_else(||Printer{id:uuid::Uuid::new_v4().to_string(),enabled:start_enabled,..Printer::default()});
                            next.welcomed=true;
                            let native_change=!printer.installed || printer.name!=name.trim();
                            printer.name=name.trim().into();printer.folder=folder;printer.pattern=pattern;printer.monthly_folders=monthly;printer.open_after=open_after;
                            if let Some(i)=existing{next.printers[i]=printer.clone();}else{next.printers.push(printer.clone());}
                            // Commit the identity before provisioning: a cancelled UAC prompt
                            // or failed final commit cannot leave an unmanageable orphan queue.
                            commit(&mut state,&path,next)?;
                            if cfg!(windows) && native_change {
                                if capture.is_none(){edit=Some(printer.clone());anyhow::bail!("The PDF receiver is offline. Select Retry pending, then Install or repair this printer.");}
                                match platform::native_action("ensure",&printer,state.capture_port){
                                    Ok(())=>{let p=find_mut(&mut state,&printer.id)?;p.installed=true;p.last_error.clear();save(&path,&state)?;},
                                    Err(e)=>{find_mut(&mut state,&printer.id)?.last_error=e.to_string();save(&path,&state)?;edit=Some(find(&state,&printer.id)?.clone());return Err(e);}
                                }
                            }
                            failures.clear();close=true;
                            Ok(if cfg!(windows){"Printer saved and ready in Windows Print dialogs.".into()}else{"Profile saved. Windows printer installation is available on Windows.".into()})
                        }
                        Command::Relocate{id,folder}=>{
                            let p=find(&state,&id)?;
                            validate(&state,&id,&p.name,&folder,&p.pattern)?;
                            let mut next=state.clone();let p=find_mut(&mut next,&id)?;p.folder=folder;
                            if p.installed{p.last_error.clear();}
                            commit(&mut state,&path,next)?;failures.clear();
                            Ok("Output folder updated. Pending PDFs will be retried.".into())
                        }
                        Command::Repair(id)=>{
                            let p=find(&state,&id)?.clone();
                            match platform::native_action("ensure",&p,state.capture_port){Ok(())=>{let p=find_mut(&mut state,&id)?;p.installed=true;p.last_error.clear();save(&path,&state)?;failures.clear();Ok("Windows printer repaired. Pending PDFs will be retried.".into())},Err(e)=>{find_mut(&mut state,&id)?.last_error=e.to_string();save(&path,&state)?;Err(e)}}
                        }
                        Command::Toggle(id,enabled)=>{
                            let mut next=state.clone();find_mut(&mut next,&id)?.enabled=enabled;commit(&mut state,&path,next)?;
                            let p=find(&state,&id)?.clone();
                            if cfg!(windows) && p.installed&& let Err(e)=platform::native_action("pause",&p,state.capture_port){find_mut(&mut state,&id)?.last_error=e.to_string();save(&path,&state)?;return Err(e);}
                            failures.clear();Ok(if enabled{"Printer resumed."}else{"Printer paused. Queued documents are retained."}.into())
                        }
                        Command::SetAll(enabled)=>{
                            let mut next=state.clone();for p in &mut next.printers{p.enabled=enabled;}commit(&mut state,&path,next)?;
                            let mut failed=false;
                            for p in state.printers.clone(){
                                if cfg!(windows) && p.installed && let Err(error)=platform::native_action("pause",&p,state.capture_port){find_mut(&mut state,&p.id)?.last_error=error.to_string();failed=true;}
                            }
                            save(&path,&state)?;failures.clear();
                            ensure!(!failed,"Some Windows queues need attention. Open Printers to repair them.");
                            Ok(if enabled{"All printers resumed."}else{"All printers paused. Queued documents are retained."}.into())
                        }
                        Command::Delete(id)=>{
                            let printer=find(&state,&id)?.clone();ensure!(pending_for(&root,&id)==0,"This printer has captured jobs awaiting delivery. Resume or retry them before deleting it.");
                            if cfg!(windows) {platform::native_action("remove",&printer,state.capture_port)?;}
                            let mut next=state.clone();next.printers.retain(|p|p.id!=id);commit(&mut state,&path,next)?;close=true;Ok("Printer removed. Saved PDFs and activity were kept.".into())
                        }
                        Command::Test(id)=>{
                            let p=find(&state,&id)?.clone();ensure!(p.enabled,"Resume this printer before printing a test page.");
                            if cfg!(windows){ensure!(p.installed,"Install or repair this printer first.");platform::print_test(&p)?;Ok("Test page sent through the Windows print queue. It will appear in Activity when captured.".into())}
                            else{let mut next=state.clone();let p=find_mut(&mut next,&id)?;let output=test_pdf(p)?;let name=p.name.clone();next.activity.push(Activity{printer:name,file:output.file_name().unwrap_or_default().to_string_lossy().into(),status:"Saved".into(),path:output.to_string_lossy().into(),printer_id:id,job_id:uuid::Uuid::new_v4().to_string(),created_unix:time::OffsetDateTime::now_utc().unix_timestamp(),size_bytes:fs::metadata(&output).map(|m|m.len()).unwrap_or(0)});commit(&mut state,&path,next)?;Ok(format!("Local test PDF saved to {}. System printing requires Windows.",output.display()))}
                        }
                        Command::Settings{dark,theme,folder,tray,notifications,startup,history_days}=>{
                            ensure!(Path::new(&folder).is_dir(),"Choose an existing default output folder.");
                            ensure!([0,30,90,365].contains(&history_days),"Choose a supported history duration.");
                            if startup!=state.startup {platform::startup(startup)?;}
                            let mut next=state.clone();next.dark=dark;next.theme=theme.clamp(0,2);next.default_folder=folder;next.keep_tray=tray;next.notifications=notifications;next.startup=startup;next.history_days=history_days;
                            if let Err(e)=commit(&mut state,&path,next){let _=platform::startup(state.startup);return Err(e);}
                            Ok("Settings saved.".into())
                        }
                        Command::Import(file)=>{
                            ensure!(fs::metadata(&file)?.len()<=2_000_000,"Backup is too large (maximum 2 MB).");
                            let imported:Vec<Printer>=serde_json::from_slice(&fs::read(file)?)?;
                            ensure!(imported.len()<=1000,"Import at most 1,000 profiles at a time.");let mut next=state.clone();
                            for mut p in imported {validate(&next,"",&p.name,&p.folder,&p.pattern)?;p.id=uuid::Uuid::new_v4().to_string();p.installed=false;p.last_error.clear();p.count=0;next.printers.push(p);}
                            commit(&mut state,&path,next)?;Ok("Profiles imported. Use Install or repair on each imported profile to add it to Windows.".into())
                        }
                        Command::Export(file)=>{let bytes=serde_json::to_vec_pretty(&state.printers)?;fs::write(file,bytes)?;Ok("Printer profiles exported.".into())},
                        Command::ClearHistory=>{let mut next=state.clone();next.activity.clear();commit(&mut state,&path,next)?;Ok("Activity cleared. Your PDFs were kept.".into())},
                        Command::Shutdown=>unreachable!(),
                    }
                })();
                if let Ok(updated)=profile_routes(&state) && let Ok(mut map)=routes.write(){*map=updated;}
                let message=match result{Ok(message)=>message,Err(e)=>{let message=format!("{e:#}");platform::log(&log,&message);message}};
                let mut new_watch=false;
                if let Some(watcher)=watcher.as_mut(){for p in &state.printers{let dir=root.join("jobs").join(&p.id);let _=fs::create_dir_all(&dir);let _=fs::create_dir_all(root.join("acks").join(&p.id));if dir.is_dir() && !watched.contains(&p.id){match watcher.watch(&dir,RecursiveMode::Recursive){Ok(())=>{watched.insert(p.id.clone());new_watch=true;},Err(e)=>platform::log(&log,&format!("Cannot watch capture folder: {e}"))}}}}
                let queued=state.printers.iter().map(|p|pending_for(&root,&p.id)).sum();
                sink(Update{state:state.clone(),message,editor:edit,close_editor:close,sync_settings,queued,capture_running:capture.is_some(),capture_port:state.capture_port,editor_health});
                if new_watch || scan_after {let _=tx_actions.send(Command::Scan);}
            }
        })?;
        let service = Self {
            sender: tx,
            join: Some(join),
        };
        let _ = service.sender.send(Command::Scan);
        Ok(service)
    }
}
fn find<'a>(state: &'a State, id: &str) -> Result<&'a Printer> {
    state
        .printers
        .iter()
        .find(|p| p.id == id)
        .context("Printer no longer exists")
}
fn find_mut<'a>(state: &'a mut State, id: &str) -> Result<&'a mut Printer> {
    state
        .printers
        .iter_mut()
        .find(|p| p.id == id)
        .context("Printer no longer exists")
}
fn commit(current: &mut State, path: &Path, mut next: State) -> Result<()> {
    prune_activity(&mut next, time::OffsetDateTime::now_utc().unix_timestamp());
    if next.activity.len() > 1000 {
        next.activity.drain(..next.activity.len() - 1000);
    }
    save(path, &next)?;
    *current = next;
    Ok(())
}
fn acknowledge(root: &Path, id: &str, job: &str) -> Result<()> {
    let path = root.join("acks").join(id).join(format!("{job}.ack"));
    if !path.exists() {
        let file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)?;
        file.sync_all()?;
    }
    Ok(())
}
pub fn pending_for(root: &Path, id: &str) -> usize {
    fs::read_dir(root.join("jobs").join(id))
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| {
            entry
                .file_name()
                .to_str()
                .and_then(|name| name.strip_suffix(".ready"))
                .is_some_and(|job| {
                    valid_id(job)
                        && !root
                            .join("acks")
                            .join(id)
                            .join(format!("{job}.ack"))
                            .is_file()
                })
        })
        .count()
}
fn scan(
    state: &mut State,
    state_path: &Path,
    root: &Path,
    data: &Path,
    failures: &mut HashSet<String>,
    log: &Path,
) -> Result<usize> {
    let mut delivered = 0;
    for printer in state.printers.clone() {
        if !valid_id(&printer.id) || !printer.enabled {
            continue;
        }
        let entries = match fs::read_dir(root.join("jobs").join(&printer.id)) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => {
                platform::log(log, &e.to_string());
                continue;
            }
        };
        let mut jobs: Vec<_> = entries.flatten().collect();
        jobs.sort_by_key(|e| e.file_name());
        for entry in jobs {
            let name = entry.file_name().to_string_lossy().to_string();
            let Some(id) = name.strip_suffix(".ready").filter(|id| valid_id(id)) else {
                continue;
            };
            if root
                .join("acks")
                .join(&printer.id)
                .join(format!("{id}.ack"))
                .is_file()
            {
                reclaim(
                    &entry.path(),
                    &data.join("deliveries").join(format!("{id}.json")),
                    log,
                );
                continue;
            }
            if failures.contains(id) {
                continue;
            }
            if state
                .activity
                .iter()
                .any(|a| a.job_id == id && a.status == "Saved")
            {
                match acknowledge(root, &printer.id, id) {
                    Ok(()) => reclaim(
                        &entry.path(),
                        &data.join("deliveries").join(format!("{id}.json")),
                        log,
                    ),
                    Err(e) => platform::log(log, &format!("Could not acknowledge {id}: {e}")),
                }
                continue;
            }
            let attempt = (|| -> Result<(JobMetadata, PathBuf)> {
                let meta = entry.path().join("job.json");
                ensure!(
                    fs::metadata(&meta)?.len() < 65536,
                    "Captured metadata is too large"
                );
                let job: JobMetadata = serde_json::from_slice(&fs::read(meta)?)?;
                ensure!(
                    job.id == id && job.printer_id == printer.id,
                    "Capture metadata does not match its folder"
                );
                let latest = find(state, &printer.id)?;
                let dest = deliver_pdf(
                    latest,
                    &job,
                    &entry.path().join("document.pdf"),
                    &data.join("deliveries").join(format!("{id}.json")),
                )?;
                Ok((job, dest))
            })();
            match attempt {
                Ok((job, destination)) => {
                    let mut next = state.clone();
                    let p = find_mut(&mut next, &printer.id)?;
                    p.count = p.count.saturating_add(1);
                    p.last_error.clear();
                    let record = Activity {
                        printer: printer.name.clone(),
                        file: destination
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .into(),
                        status: "Saved".into(),
                        path: destination.to_string_lossy().into(),
                        printer_id: printer.id.clone(),
                        job_id: job.id.clone(),
                        created_unix: job.created_unix,
                        size_bytes: fs::metadata(&destination).map(|m| m.len()).unwrap_or(0),
                    };
                    if let Some(row) = next.activity.iter_mut().find(|a| a.job_id == job.id) {
                        *row = record;
                    } else {
                        next.activity.push(record);
                    }
                    commit(state, state_path, next)?;
                    match acknowledge(root, &printer.id, &job.id) {
                        Ok(()) => reclaim(
                            &entry.path(),
                            &data.join("deliveries").join(format!("{}.json", job.id)),
                            log,
                        ),
                        Err(e) => platform::log(
                            log,
                            &format!("PDF saved but acknowledgement failed: {e}"),
                        ),
                    }
                    if printer.open_after
                        && let Err(e) = open::that(&destination)
                    {
                        platform::log(log, &e.to_string());
                    }
                    if state.notifications
                        && let Err(e) = platform::notify_saved(&destination, &printer.name)
                    {
                        platform::log(log, &format!("Notification unavailable: {e}"));
                    }
                    delivered += 1;
                }
                Err(e) => {
                    let message = format!("{e:#}");
                    platform::log(log, &format!("Delivery {id}: {message}"));
                    failures.insert(id.to_owned());
                    let mut next = state.clone();
                    find_mut(&mut next, &printer.id)?.last_error = message.clone();
                    let record = Activity {
                        printer: printer.name.clone(),
                        file: "Captured document".into(),
                        status: format!("Failed: {message}"),
                        path: String::new(),
                        printer_id: printer.id.clone(),
                        job_id: id.into(),
                        created_unix: time::OffsetDateTime::now_utc().unix_timestamp(),
                        size_bytes: 0,
                    };
                    if let Some(row) = next.activity.iter_mut().find(|a| a.job_id == id) {
                        *row = record;
                    } else {
                        next.activity.push(record);
                    }
                    commit(state, state_path, next)?;
                }
            }
        }
    }
    Ok(delivered)
}

/// Process the durable inbox once, for recovery tools and unattended validation.
pub fn process_once(state_path: &Path, root: &Path) -> Result<usize> {
    let data = state_path
        .parent()
        .context("State path needs a parent folder")?;
    fs::create_dir_all(data)?;
    let lock = fs::OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(data.join("app.lock"))?;
    lock.try_lock()
        .context("The app is running; use Retry pending from its tray menu")?;
    let (mut state, _) = load_state(state_path)?;
    let delivered = scan(
        &mut state,
        state_path,
        root,
        data,
        &mut HashSet::new(),
        &data.join("logs/app.log"),
    )?;
    let remaining: usize = state
        .printers
        .iter()
        .map(|p| pending_for(root, &p.id))
        .sum();
    ensure!(
        remaining == 0,
        "{remaining} captured job(s) remain pending. Check Activity or the application log."
    );
    Ok(delivered)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn folder_recovery_and_pause_all_preserve_profiles() {
        let dir = tempfile::tempdir().expect("temp");
        let old = dir.path().join("old");
        let new = dir.path().join("new");
        fs::create_dir(&old).expect("old folder");
        fs::create_dir(&new).expect("new folder");
        let state = State {
            notifications: false,
            printers: ["one", "two"]
                .into_iter()
                .map(|id| Printer {
                    id: id.into(),
                    name: id.into(),
                    folder: old.clone(),
                    pattern: "{title}".into(),
                    enabled: true,
                    ..Printer::default()
                })
                .collect(),
            ..State::default()
        };
        let path = dir.path().join("state.json");
        let (tx, rx) = mpsc::channel();
        let worker = Service::start(
            path.clone(),
            state,
            dir.path().join("capture"),
            move |update| {
                let _ = tx.send(update);
            },
        )
        .expect("worker");
        let receive = |predicate: &dyn Fn(&Update) -> bool| {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
            loop {
                let update = rx
                    .recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()))
                    .expect("worker response");
                if predicate(&update) {
                    break update;
                }
            }
        };
        worker
            .sender
            .send(Command::Relocate {
                id: "one".into(),
                folder: new.clone(),
            })
            .expect("relocate");
        let update = receive(&|u| u.message.starts_with("Output folder updated"));
        assert_eq!(update.state.printers[0].folder, new);
        assert_eq!(update.state.printers[1].folder, old);
        worker.sender.send(Command::SetAll(false)).expect("pause");
        let update = receive(&|u| u.message.starts_with("All printers paused"));
        assert!(update.state.printers.iter().all(|p| !p.enabled));
        worker.sender.send(Command::SetAll(true)).expect("resume");
        let update = receive(&|u| u.message.starts_with("All printers resumed"));
        assert!(update.state.printers.iter().all(|p| p.enabled));
        worker
            .sender
            .send(Command::Relocate {
                id: "one".into(),
                folder: dir.path().join("missing"),
            })
            .expect("bad folder");
        receive(&|u| u.message.contains("existing output folder"));
        assert_eq!(
            load_state(&path).expect("persisted").0.printers[0].folder,
            new
        );
        drop(worker);
    }
    #[test]
    fn captured_jobs_recover_after_failure_and_do_not_deliver_twice() {
        let dir = tempfile::tempdir().expect("temp");
        let root = dir.path().join("spool");
        let id = "profile1";
        let job_id = "job1";
        let ready = root.join("jobs").join(id).join(format!("{job_id}.ready"));
        fs::create_dir_all(&ready).expect("capture");
        fs::create_dir_all(root.join("acks").join(id)).expect("acks");
        let output = dir.path().join("output");
        fs::create_dir(&output).expect("output");
        let mut state = State {
            notifications: false,
            history_days: 0,
            printers: vec![Printer {
                id: id.into(),
                name: "Invoices".into(),
                folder: output.clone(),
                pattern: "{title}".into(),
                enabled: true,
                ..Printer::default()
            }],
            ..State::default()
        };
        let job = JobMetadata {
            id: job_id.into(),
            printer_id: id.into(),
            title: "Document".into(),
            created_unix: 1700000000,
        };
        fs::write(
            ready.join("job.json"),
            serde_json::to_vec(&job).expect("json"),
        )
        .expect("metadata");
        fs::write(ready.join("document.pdf"), b"not ready").expect("bad input");
        let state_path = dir.path().join("state.json");
        let mut failures = HashSet::new();
        assert_eq!(
            scan(
                &mut state,
                &state_path,
                &root,
                dir.path(),
                &mut failures,
                &dir.path().join("log")
            )
            .expect("failed scan"),
            0
        );
        assert!(state.activity[0].status.starts_with("Failed:"));
        assert_eq!(pending_for(&root, id), 1);
        fs::write(ready.join("document.pdf"), b"%PDF-1.4\nexample\n%%EOF").expect("valid input");
        failures.clear();
        assert_eq!(
            scan(
                &mut state,
                &state_path,
                &root,
                dir.path(),
                &mut failures,
                &dir.path().join("log")
            )
            .expect("retry"),
            1
        );
        assert_eq!(state.printers[0].count, 1);
        assert_eq!(state.activity.len(), 1);
        assert_eq!(state.activity[0].status, "Saved");
        assert_eq!(pending_for(&root, id), 0);
        state.activity.clear(); // A durable acknowledgement survives clearing UI history.
        assert_eq!(
            scan(
                &mut state,
                &state_path,
                &root,
                dir.path(),
                &mut failures,
                &dir.path().join("log")
            )
            .expect("rescan"),
            0
        );
        assert_eq!(fs::read_dir(output).expect("output").count(), 1);
    }
}

#[cfg(test)]
mod recovery_and_watcher_tests {
    use super::*;
    fn fixture() -> (tempfile::TempDir, PathBuf, State, PathBuf) {
        let dir = tempfile::tempdir().expect("temp");
        let root = dir.path().join("spool");
        fs::create_dir_all(root.join("jobs/profile1")).expect("jobs");
        fs::create_dir_all(root.join("acks/profile1")).expect("acks");
        let out = dir.path().join("output");
        fs::create_dir(&out).expect("out");
        let state = State {
            welcomed: true,
            notifications: false,
            history_days: 0,
            printers: vec![Printer {
                id: "profile1".into(),
                name: "Invoices".into(),
                folder: out,
                pattern: "{title}".into(),
                enabled: true,
                ..Printer::default()
            }],
            ..State::default()
        };
        let path = dir.path().join("state.json");
        (dir, root, state, path)
    }
    fn capture(root: &Path) {
        let partial = root.join("jobs/profile1/job1.partial");
        fs::create_dir(&partial).expect("partial");
        let metadata = JobMetadata {
            id: "job1".into(),
            printer_id: "profile1".into(),
            title: "Document".into(),
            created_unix: 1700000000,
        };
        fs::write(
            partial.join("job.json"),
            serde_json::to_vec(&metadata).expect("json"),
        )
        .expect("meta");
        fs::write(partial.join("document.pdf"), b"%PDF-1.4\nexample\n%%EOF").expect("PDF");
        fs::rename(partial, root.join("jobs/profile1/job1.ready")).expect("publish");
    }
    #[test]
    fn failed_state_commit_can_retry_without_duplicate_output() {
        let (dir, root, mut state, path) = fixture();
        capture(&root);
        fs::create_dir(&path).expect("block commit");
        let mut failures = HashSet::new();
        let log = dir.path().join("log");
        assert!(scan(&mut state, &path, &root, dir.path(), &mut failures, &log).is_err());
        assert_eq!(state.printers[0].count, 0);
        assert_eq!(pending_for(&root, "profile1"), 1);
        fs::remove_dir(&path).expect("repair state");
        assert_eq!(
            scan(&mut state, &path, &root, dir.path(), &mut failures, &log).expect("retry"),
            1
        );
        assert_eq!(
            fs::read_dir(&state.printers[0].folder)
                .expect("files")
                .count(),
            1
        );
        assert_eq!(state.printers[0].count, 1);
    }
    #[test]
    fn filesystem_events_deliver_without_polling() {
        let (_dir, root, state, path) = fixture();
        let (tx, rx) = mpsc::channel();
        let service = Service::start(path, state, root.clone(), move |update| {
            let _ = tx.send(update);
        })
        .expect("worker");
        rx.recv_timeout(std::time::Duration::from_secs(10))
            .expect("initial scan and watch registration");
        capture(&root);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            let update = rx
                .recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()))
                .expect("notification-driven delivery");
            if update
                .state
                .activity
                .iter()
                .any(|a| a.job_id == "job1" && a.status == "Saved")
            {
                assert_eq!(update.state.printers[0].count, 1);
                break;
            }
        }
        drop(service);
    }
}

fn profile_routes(state: &State) -> Result<HashMap<String, String>> {
    let mut routes = HashMap::new();
    for p in &state.printers {
        ensure!(
            routes.insert(lpr_queue(&p.id)?, p.id.clone()).is_none(),
            "Two printer identities map to the same LPR queue"
        );
    }
    Ok(routes)
}
fn start_capture(
    state: &mut State,
    path: &Path,
    root: PathBuf,
    routes: Arc<RwLock<HashMap<String, String>>>,
    tx: Sender<Command>,
    pending: Arc<AtomicBool>,
) -> Result<Option<vps_core::lpr::CaptureServer>> {
    if !cfg!(windows) {
        return Ok(None);
    }
    let server = vps_core::lpr::CaptureServer::start_with_title_resolver(
        std::net::SocketAddr::from(([127, 0, 0, 1], state.capture_port)),
        root,
        routes,
        Arc::new(platform::authorize_capture),
        Arc::new(move |event| match event {
            Ok(_) => {
                if !pending.swap(true, Ordering::AcqRel) {
                    let _ = tx.send(Command::Scan);
                }
            }
            Err(e) => {
                let _ = tx.send(Command::CaptureError(format!("PDF capture: {e:#}")));
            }
        }),
        Arc::new(platform::capture_title),
    )?;
    if state.capture_port != server.address().port() {
        let mut next = state.clone();
        next.capture_port = server.address().port();
        commit(state, path, next)?;
    }
    Ok(Some(server))
}

fn reclaim(capture: &Path, receipt: &Path, log: &Path) {
    match fs::remove_dir_all(capture) {
        Ok(()) => {
            let _ = fs::remove_file(receipt);
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let _ = fs::remove_file(receipt);
        }
        Err(e) => platform::log(
            log,
            &format!("PDF saved; capture cleanup can be retried: {e}"),
        ),
    }
}
