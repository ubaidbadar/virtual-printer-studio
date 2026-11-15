# Virtual Printer Studio

A Rust / Slint desktop application for Windows 10/11: create named virtual printers, print from other applications, and automatically save each job as a PDF in its configured folder.

Version 0.2 adds a native **Windows port monitor DLL** backed by the built-in **Microsoft Print to PDF** driver. It captures each job independently, including concurrent jobs; there is no shared output filename and no Save As prompt in the application's pipeline.

## Windows installation

Download the `virtual-printer-studio-windows` artifact from a successful [Desktop checks workflow](../../actions). Extract the **whole package**, keeping `virtual-printer-studio.exe` and `vps_port_monitor.dll` together.

- Run the executable directly, or run `install.ps1` from an administrator PowerShell terminal to install into Program Files and create a Start menu shortcut.
- Select **New printer**, choose an existing writable folder, enter a unique name, and select **Save printer**.
- Approve the Windows administrator prompt to install the native printer. A cancelled or failed installation retains the profile and offers **Install or repair**.
- Select the named printer from another application's Print dialog. Completed PDFs appear in Activity.
- Keep the app running in its system tray for immediate delivery. The native monitor captures jobs even when the desktop app is stopped; queued captures are delivered at the next launch.

Requires x64 Windows with the Print Spooler and Microsoft Print to PDF feature available. Setup enables the Windows feature when needed; some Windows editions require a restart. The Windows binaries are unsigned. Organizational policies may restrict custom print monitors.

## Features

- Native Windows printer creation, rename, repair, pause/resume and removal.
- Collision-safe output with `{title}`, `{date}`, `{time}`, `{counter}` filename tokens; document titles are sanitized for Windows filenames.
- Monthly subfolders and optional opening after saving.
- Separate durable capture per job, atomic publication, SHA-256 delivery receipts, and acknowledgement **after** state has been saved.
- Event-driven delivery: no periodic inbox polling. Failed jobs remain captured and can be retried after fixing a folder or permission problem.
- Activity search, saved/failed filters, Open, Show folder, Fix printer, retry, and clear-history confirmation.
- Working invoice/receipt/contract/report presets.
- System/light/dark themes, including live Windows appearance changes.
- Windows startup registration, close-to-tray, native tray menu, single-instance activation, saved-PDF toast notifications with Open / Show folder actions while the app is running.
- Profile import/export, automatic settings backup recovery, diagnostics copying, and rotating application logs.

Counts are lifetime totals. The visible activity history is capped at 1,000 entries. Clearing history does not remove PDFs or replay acknowledged captures. Imported profiles receive fresh identities and require installation on the destination machine. Settings and PDF files are not included in profile exports.

## Storage and security boundaries

Desktop state is stored under the OS application-data directory. `state.json.bak` retains the previous commit. Corrupt primary settings are preserved and recovered automatically when the backup is readable.

The privileged monitor stores jobs under `%ProgramData%\VirtualPrinterStudio\jobs\<profile-id>`. Only SYSTEM and administrators can modify those captured files; the configuring Windows account receives read access. A separate `acks\<profile-id>` directory grants that account write access for acknowledgements. The monitor uses the documented spooler impersonation APIs and never writes PDFs directly into a user-writable output folder as SYSTEM. The unprivileged desktop worker performs final delivery.

Acknowledged captures are reclaimed by the monitor when the next job for that printer starts. Incomplete/cancelled captures are retained for diagnosis. Deleting a printer refuses pending Windows jobs and undelivered completed captures. It retains saved PDFs, activity and recovery data.

Changing native printer state requests elevation. File-name edits and output-folder edits on an already installed printer do not need elevation. Imported profiles are installed explicitly through **Install or repair**.

## Build and verify

Install current stable Rust and the Windows Visual Studio C++ build tools:

```sh
cargo build --workspace --release --locked
cargo test --workspace --locked
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
```

The runnable Windows package needs both files from `target/release`: `virtual-printer-studio.exe` and `vps_port_monitor.dll`. Linux development needs fontconfig development libraries and X11/Wayland plus an XDG portal for file dialogs. Linux supports profiles, local test PDFs and the delivery worker; native printer installation and Windows integration are Windows-only.

The Windows CI integration script installs an isolated temporary printer, prints several real documents, checks PDF delivery and replay prevention, exercises pause/resume and missing-folder recovery, and removes its temporary printer:

```powershell
./scripts/windows-smoke.ps1 -App target/release/virtual-printer-studio.exe -Monitor target/release/vps_port_monitor.dll
```

Run that test in an elevated terminal on a Windows installation with the PDF driver available. It retains test evidence under the printed test folder. CI publishes the executable/DLL package and integration evidence separately. An uploaded package alone is not proof that the integration step passed; check the workflow result.

For unattended recovery (the desktop app must be stopped):

```powershell
virtual-printer-studio.exe --process-pending
virtual-printer-studio.exe --process-pending --state C:\path\state.json
```

## Structure

- `crates/core`: domain, persistence, naming, PDF delivery and recovery tests; no UI or Windows APIs.
- `crates/port-monitor`: Windows spooler ABI and durable per-job capture; unsafe code is confined to the Windows adapter.
- `src/platform`: Windows provisioning, notifications, startup and system appearance.
- `src/service.rs`: serialized worker, native actions and event-driven delivery.
- `src/main.rs`: UI composition and callbacks; file picking is asynchronous.
- `ui/theme`, `ui/components`, `ui/app.slint`: Fluent UI, theme tokens, reusable cards, pages and dialogs.

The app uses Slint 1.18's native `SystemTrayIcon` rather than adding a second tray toolkit. This supersedes the older tray-library suggestion in the supplied design reference.

## Uninstall

Delete installed printers through the app first, then quit it and run the packaged `install.ps1 -Uninstall` from an administrator terminal. Recovery data, user settings and saved PDFs are kept.

## Distribution and licensing

No application distribution license has been selected by the owner. Slint has GPLv3, royalty-free and commercial options; review https://slint.dev/license/ before distribution. **Made with Slint** — https://slint.dev.

See [validation and release limitations](docs/validation.md) for test evidence and dependency maintenance/licensing findings. PDF/A conversion, PDF encryption and document post-processing are outside this version's supported feature set.
