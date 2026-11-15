# Validation and release limitations

Version 0.2 includes the Windows printer implementation; the earlier 0.1 restriction to test PDFs no longer describes the source.

## Automated evidence

Checked in the Linux development environment with Rust stable 1.100.0 and Slint 1.18.1:

- Twelve workspace tests pass: filename safety, duplicate names, collision-safe PDFs, delivery receipts, interrupted state commits, failed-job recovery, acknowledgement/replay prevention, corrupted-state recovery, filesystem notification delivery, and UI pages/search/filters/callbacks.
- The Windows application and port monitor are cross-checked against the Windows target; Windows-specific code is also checked with Clippy.
- CI builds/tests both Linux and Windows and packages the Windows executable plus monitor DLL.
- `scripts/windows-smoke.ps1` is a real Windows spooler integration test, not a mock. It installs a temporary printer, prints three documents, verifies their PDF output, verifies retry does not replay them, pauses/resumes the printer, checks missing-folder recovery, and removes the printer. Check the current workflow's **Native Windows print integration** result before treating a package as Windows-validated.

Manual Windows/Narrator testing, DPI/multi-monitor acceptance tests, and confirmation of notification activation under the owner's Windows policies still need a Windows desktop. Toast action handlers operate while the desktop process is alive; they are not registered as a separate out-of-process COM notification activator.

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

- x64 Windows only for native installation; Microsoft Print to PDF and the Print Spooler must be available.
- A first installation may need a Windows feature change/restart; policies may block installation of custom monitors.
- The desktop process must be running for immediate final-folder delivery. When it is stopped, the monitor retains captured jobs for the next launch.
- Incomplete/cancelled captures are retained for diagnosis; acknowledged completed captures are reclaimed when the next print starts.
- PDF/A, password encryption and PDF editing are not exposed as supported controls.
