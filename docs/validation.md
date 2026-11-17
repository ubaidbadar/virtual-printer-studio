# Validation and release limitations

Version 0.2 includes the Windows printer implementation; the earlier 0.1 restriction to test PDFs no longer describes the source.

## Automated evidence

- Seventeen portable workspace tests cover validation, filename collisions, delivery receipts, interrupted commits, failed-job recovery, acknowledgements, state recovery, filesystem events, LPR framing/retry, malformed and interrupted transfers, replay after payload reclamation, Unicode metadata, and actual widget accessibility actions. An additional Windows test executes the locked installer with PowerShell while verifying that another writer is refused.
- The application cross-compiles against the Windows target and is checked with Clippy for both platforms.
- The standalone Windows LPR acceptance run [43695921190](https://github.com/ubaidbadar/virtual-printer-studio/actions/runs/43695921190) installed a real Microsoft PDF printer, captured its PDF and original job title over a configurable Standard TCP/IP LPR port, and removed the printer/port successfully.
- The driver/receiver integration run [43753732917](https://github.com/ubaidbadar/virtual-printer-studio/actions/runs/43753732917) passed on Windows and Linux. Its native test delivered six real PDFs, preserved document titles, rejected a non-spooler connection, paused/resumed printing, recovered a print submitted while the app was stopped, recovered a missing output folder after restart, verified no duplicate output, and removed the temporary printer and port.
- Windows builds statically link the C runtime so the package does not require a separate VC++ redistributable. Each integration run uses an isolated evidence subdirectory.

See [client testing](client-testing.md) for actual client OS evidence and the visual comparison. Manual Narrator speech, physical multi-monitor DPI changes and notification-activation acceptance remain separate checks. Toast actions operate while the process is alive; no separate out-of-process COM activator is registered.

## Dependency review

`cargo deny check` has unresolved upstream maintenance and license-policy findings. Prior source and dependency-ban checks pass. Slint's dependency graph includes these unmaintained transitive dependencies, without a compatible safe upgrade in the selected current graph:

| Crate | Advisory |
| --- | --- |
| bincode | RUSTSEC-2025-0141 |
| pin-utils | RUSTSEC-2026-0441 |
| ttf-parser | RUSTSEC-2026-0192 |
| zbus_names | RUSTSEC-2026-0371 |
| zvariant | RUSTSEC-2026-0370 |
| zvariant_derive | RUSTSEC-2026-0372 |

`cargo audit` completes with six unmaintained-dependency warnings and no reported vulnerability errors. The default license check fails because no project license allowlist has been approved. This includes ordinary MIT/Apache dependencies as well as Slint's alternative terms; it is not evidence that every dependency is incompatible. No advisories were suppressed and no application/Slint distribution license was selected on the owner's behalf. Review the graph and select a distribution license policy before publishing a release.

## Operational boundaries

- Windows Print Spooler and Microsoft Print to PDF must be available. A feature installation can require a restart or be blocked by organizational policy.
- Immediate saving needs the app/tray receiver. When it is stopped, Windows retains queued print jobs for retry after restart.
- A receiver-port conflict is reported; close the conflicting process and retry.
- Completed failed deliveries remain in the local inbox. Acknowledged source payloads are reclaimed; acknowledgement identities are retained for replay protection.
- PDF/A, password encryption and PDF editing are not exposed as supported controls.
