# Validation and release limitations

Checked in a Linux build environment on 2026-11-15 using Rust stable 1.100.0 and Slint 1.18.1.

- `cargo test --locked`: five tests pass (validation, collision-safe PDF output, PDF offsets, persistence and backup recovery, UI pages/search/callbacks).
- `cargo fmt --check`: passes.
- `cargo clippy --locked --all-targets -- -D warnings`: passes.
- `cargo build --locked`: passes.
- Graphical smoke check under Xvfb: launch, populated printer view, click Test PDF, state persisted after restart. The output is readable by the independent Poppler `pdftotext` parser.
- Linux/Windows build workflow is provided; Windows behavior has not been manually tested in this environment.

## Dependency review

`cargo deny check` is **not clean**. Default source and dependency-ban checks pass. Its advisory check reports these upstream **unmaintained** transitive dependencies; none has an available compatible safe upgrade in the selected current Slint dependency graph:

| Crate | Advisory |
| --- | --- |
| pin-utils | RUSTSEC-2026-0441 |
| ttf-parser | RUSTSEC-2026-0192 |
| zbus_names | RUSTSEC-2026-0371 |
| zvariant | RUSTSEC-2026-0370 |
| zvariant_derive | RUSTSEC-2026-0372 |

The default license check also fails because no project license allowlist has been approved. This includes ordinary MIT/Apache dependencies as well as Slint's alternative license terms; it is not evidence that every dependency is incompatible. No advisories were suppressed and no application/Slint distribution license was selected on the owner's behalf. Review the dependency graph and select a distribution license policy before publishing a release.

The app does not install a Windows virtual printer and is not a replacement for a print driver. The remaining Windows-specific work is listed in the README.
