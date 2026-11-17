# Reference design refresh

The desktop UI now uses the supplied screen structure: a 232 px navigation pane, a three-column status-card grid, filter pills, a full-page printer detail view, an activity table with summary cards, grouped settings, and a separate first-run screen. Shared controls and pages consume one token set; view-model reconciliation updates existing list models instead of replacing them for each change.

The new layout retains actual printer creation, repair, pause/resume, test printing, recovery, deletion, backup and file actions. Folder recovery has its own validated service command. “Start saving right away” now controls the initial queue state. Activity supports printer/date/status filters, real file sizes on new records and configurable metadata retention; saved PDFs are never deleted by retention. Unknown legacy timestamps are preserved, and the existing 1,000-row history cap remains.

The tray flyout uses the same component as its preview, with real pause/resume-all, recent-file, Settings and Quit actions. Ctrl+Shift+T opens it from the application. Native Windows notifications use the saved filename, printer and size with Open/Show folder actions. The Windows executable embeds the new multi-size icon; 16/20 px assets use the solid outline.

PDF/A conversion, PDF encryption, secondary-folder delivery and source-application filename tokens are not implemented. Their controls remain disabled with explanatory tooltips and accessibility labels. Page size continues to come from the originating print dialog. These are explicit capability differences from the mockup, rather than working-feature claims.

## Verification

The updated capture command renders all nine references. Scenes 01–07 are actual application client buffers, compared after removing the supplied 40 px native title bar. Scene 08 is a controlled composition of the tray and toast-content components; it is **not** evidence of a native Windows notification or desktop capture. Scene 09 renders the shipped branding assets. No actual image is resized or aligned to improve the comparison.

The acceptance harness checks 100%, 125%, 150% and 200% scale. Windows accessibility automation also opens the production tray flyout and invokes pause/resume-all and Settings. A strict pixel-match gate is enabled by default; a successful measurement alone does not approve a design.

Local formatting, Clippy and 21 portable tests passed during the refresh. The new tests cover model identity, date/size formatting, history retention and legacy records, folder recovery, pause/resume-all, keyboard dismissal, and the initial paused-printer setting. Final Windows client results and reference differences are recorded separately after execution. No 100% pixel-match claim is made here.

Dependency review: `cargo audit` reported seven upstream maintenance warnings, with no reported vulnerability failure. `cargo deny check` remains failing for upstream maintenance advisories and unresolved project/Slint licence policy. This change does not select a distribution licence. The added Windows resource build tool is `winresource` 0.1.34.
