# Restora order-print test status

The live authenticated order-print flow at https://app.restora.live/ has **not been tested**. Earlier Windows acceptance runs used GDI test documents and the desktop application's own buttons, not Restora or a browser print dialog.

Public-source inspection on 2026-11-17 found the deployed entry bundle `index-RMwExQDm.js`. Its order-print function tries a server `/print-jobs` request, then a branch-configured local print bridge, then a browser iframe fallback. The fallback calls the iframe's `window.print()` and sets an 80 mm custom page size. Receipt and kitchen-slip layouts differ. This is source inspection, not evidence that a live print succeeded.

A real test needs a signed-in Restora browser on the same Windows machine as the installed virtual queue and an existing authorized test order. Record which path is used; a server/bridge success message alone does not prove the local virtual queue received a job. Verify the resulting PDF against the order's items, quantities, prices, tax, totals, payment stamp, notes and any logo. Check receipt width, long-order pagination, clipping, reprinting and duplicate handling. Use the actual order Print action; browser Save as PDF bypasses the Windows printer and is insufficient.

This session has no callable browser-control tool or authenticated Restora session. No orders were created, modified, paid, or queued for printing during this inspection.

## Reuse and structure review

The Rust workspace separates transport/delivery and shared domain data into `crates/core`, with Windows-specific operations under `src/platform`. The UI shares `PrinterCard` and a central `Tokens` global. Those are concrete reusable boundaries.

The source is not fully refactored. `src/service.rs` combines worker orchestration, native operations and persistence, while `src/main.rs` combines CLI startup, UI binding and settings synchronization. Some PowerShell automation locator/wait logic is duplicated. Refactoring should preserve the verified behavior and be followed by relevant native regression tests.

The application's design is **not 100% matched** to the supplied references. See [client testing](client-testing.md) for the measured differences. No new design-conformance or Restora-compatibility claim is made here.
