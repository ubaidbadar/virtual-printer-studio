# Virtual Printer Studio

A Rust / Slint desktop application based on the supplied Windows 11 UI designs. Create local printer profiles, choose PDF output folders and filename patterns, pause profiles, generate real test PDFs, inspect activity, switch themes, and import/export profiles.

**Status: desktop foundation, not a Windows virtual printer driver.** Profiles do not appear in Windows Print dialogs. This release writes test PDFs; it does not capture documents from other applications. No administrator permission is requested.

## Run

Install current stable Rust, then:

```sh
cargo run --release
```

On Windows, install the Visual Studio C++ build tools. Linux requires a graphical Wayland/X11 session and fontconfig; the file picker uses an XDG desktop portal. The app uses Slint's Fluent style, winit backend, and software renderer. No Qt dependency.

```sh
cargo test --locked
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
```

## Use

1. Dismiss the first-run guide and select **New printer**.
2. Choose an existing writable folder and a unique profile name.
3. Use `{title}`, `{date}`, `{time}`, `{counter}` in the file pattern.
4. Select **Test PDF** to generate an A4 test page. Filename collisions get a numeric suffix; existing documents are never overwritten.
5. Open generated PDFs from Activity. Settings includes profile backup and appearance controls.

Dates and times in output filenames use UTC. Activity is capped at 1,000 entries. Profile counts are lifetime totals. Deleting a profile retains its files and history. Import merges profiles and rejects duplicate names or unsafe paths/patterns. Backups contain profiles only, not PDFs or activity.

State is stored in the OS application-data directory (`directories::ProjectDirs`), with a `state.json.bak` recovery copy. If the saved JSON is unreadable, startup stops with a recovery message rather than replacing user data. PDF writes use exclusive creation and are flushed before success is reported.

## Structure

- `ui/theme/`: shared light/dark design tokens
- `ui/components/`: reusable printer card and display models
- `ui/app.slint`: navigation, conditional pages, virtualized lists, editor
- `src/domain.rs`: serialization, validation, PDF writer, tests
- `src/main.rs`: composition and weak-handle UI callbacks

## Remaining Windows integration

A production virtual printer needs a separately designed and signed Windows print pipeline / port monitor or supported capture service. Before shipping: integrate and test spooler registration and removal, document-to-PDF conversion, service health, elevation, single instance, tray lifetime, actionable notifications, startup registration, and live system theme. Mica, PDF/A, encryption, subfolders, and advanced PDF options are not implemented. Native file dialogs and local storage currently execute synchronously; service work should move off the UI thread before integrating long-running capture.

The supplied nine-screen mockups are design references, not screenshots of this implementation. The printer grid virtualizes rows and switches between two and three columns at the window-width breakpoint. Windows/Narrator, DPI, and idle-resource acceptance testing still needs Windows hardware.

## Licensing

No application distribution license has been selected by the owner. Slint has separate GPLv3, royalty-free, and commercial licensing options. Review https://slint.dev/license/ before distributing. **Made with Slint** — https://slint.dev.

See [validation and release limitations](docs/validation.md) for test evidence and unresolved upstream dependency maintenance/licensing findings.
