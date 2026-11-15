# Virtual Printer Studio

A Rust / Slint desktop app for Windows 10/11. Create named virtual printers, print from other applications, and automatically save each document as a PDF in its configured folder.

Version 0.2 uses **Microsoft Print to PDF** with Windows' built-in **Standard TCP/IP port monitor in LPR mode**. All capture traffic stays on `127.0.0.1`. No custom driver, port-monitor DLL, Ghostscript, or third-party rendering service is installed.

## Install and use on Windows

Download `virtual-printer-studio-windows` from a successful [Desktop checks run](https://github.com/ubaidbadar/virtual-printer-studio/actions/workflows/check.yml). Extract the package.

1. Run `virtual-printer-studio.exe`, or run `install.ps1` from an administrator PowerShell terminal for a Program Files installation and Start menu shortcut.
2. Choose **New printer**, enter a unique name, select an existing writable folder, and save.
3. Approve administrator access to create the Windows printer. Failed or cancelled setup retains the profile; **Install or repair** retries setup.
4. Choose that printer in another application's Print dialog. PDFs appear in Activity and the configured folder.
5. Keep the app in its tray for immediate saving. If the app is stopped, the **Windows print queue retains jobs** until the app starts again.

Requires the Windows Print Spooler and Microsoft Print to PDF feature. Setup enables the feature if missing and explains when Windows needs a restart. The downloadable build is x64 and unsigned.

## Features

- Windows printer installation, rename, repair, pause/resume and removal.
- File patterns using `{title}`, `{date}`, `{time}`, `{counter}`. LPR control records preserve the original document title; filenames are sanitized.
- Existing files are never overwritten. Monthly subfolders and opening after saving are optional.
- Byte-counted capture, atomic PDF publication, SHA-256 delivery receipts, and durable acknowledgements after state is committed.
- A repeated transfer of the same LPR control/data is acknowledged without creating another output.
- Failed deliveries remain captured. Fix the folder and select **Retry pending**, or restart the app.
- Activity search, saved/failed filters, Open, Show folder, Fix printer, and clear-history confirmation.
- Working invoice, receipt, contract and report presets.
- System/light/dark appearance, including live Windows theme changes.
- Windows startup, close-to-tray, native tray menu and single-instance activation.
- Saved-PDF notifications with Open / Show folder actions while the app is running.
- Profile backups, damaged-settings recovery, diagnostics and rotating logs.

Counts are lifetime totals; visible activity is capped at 1,000 rows. Clearing history preserves PDFs and does not replay acknowledged captures. Imported profiles receive fresh identities and require installation on the destination computer.

## Capture and storage

State is stored in the OS application-data directory. Atomic replacement retains `state.json.bak`. If the primary JSON is damaged, it is preserved and the readable backup is restored.

A persisted local TCP port is assigned to the receiver. Each installed printer uses its own LPR queue identity at that address. The receiver accepts **only connections owned by the actual Windows Print Spooler PID**, queried through the Service Control Manager and Windows TCP owner table. Printer permissions restrict submission to the configuring account, SYSTEM and administrators.

Complete captures are stored under the same user's application-data directory, then delivered by the unprivileged worker. Source PDFs are removed after successful delivery and state acknowledgement. Small acknowledgement markers are retained to prevent transport retries from replaying completed jobs. No privileged process writes directly into a user-selected output folder.

Do not run multiple copies against one state file; the app enforces a state lock. If another process occupies the saved receiver port, capture reports an error and Windows retains queued jobs. Close the conflicting process and select Retry pending. Different app states/users receive independent ports.

## Build and test

Install current stable Rust and, on Windows, the Visual Studio C++ build tools:

```sh
cargo build --release --locked
cargo test --workspace --locked
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
```

Linux development requires fontconfig development libraries and X11/Wayland with an XDG file-dialog portal. Linux supports profiles, local test PDFs and inbox delivery; native printer installation is Windows-only.

The Windows integration test creates a temporary real printer and exercises normal printing, authenticated capture, multiple PDFs, job titles, pause/resume, printing while the app is stopped, missing-folder recovery, replay prevention and native cleanup:

```powershell
./scripts/windows-smoke.ps1 -App target/release/virtual-printer-studio.exe
```

Run it from an elevated terminal. CI publishes the Windows package and integration evidence separately; check the **Native Windows print integration** step, not just artifact availability.

For unattended processing, with the desktop app stopped:

```powershell
virtual-printer-studio.exe --process-pending
virtual-printer-studio.exe --serve --state C:\path\state.json
```

`--serve` runs the same authenticated receiver and delivery worker without the GUI. Stop that process before opening the same state in the desktop app.

## Code layout

- `crates/core`: domain, persistence, safe naming, PDF delivery, RFC 1179 receiver and portable tests.
- `src/platform`: Windows provisioning, connection authentication, notifications, startup and appearance.
- `src/service.rs`: serialized background worker and event-driven delivery; no inbox polling.
- `src/main.rs`: composition, single-instance handling and asynchronous UI callbacks.
- `ui/`: Fluent pages, conditional dialogs, theme tokens and virtualized printer rows.

Slint 1.18's native `SystemTrayIcon` supplies the tray. Microsoft v4 drivers reject third-party port monitors; see [Microsoft's connectivity documentation](https://learn.microsoft.com/en-us/windows-hardware/drivers/print/v4-driver-connectivity-architecture). The earlier experimental DLL route was removed.

## Uninstall and licensing

Delete installed printers through the app, quit it, then run `install.ps1 -Uninstall` in an administrator terminal. User settings, recovery data and saved PDFs are preserved.

No application distribution license has been selected by the owner. Review Slint's GPLv3, royalty-free and commercial options at https://slint.dev/license/ before distribution. **Made with Slint** — https://slint.dev.

See [validation](docs/validation.md) for evidence and dependency-review findings. PDF/A conversion, password encryption and PDF editing are outside the supported feature set.
