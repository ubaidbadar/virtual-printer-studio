# Restora order-print test status

The live authenticated order-print flow at https://app.restora.live/ has now been checked at the browser layer; the browser-to-Windows-printer path remains unverified. See the authenticated checks below. Earlier Windows acceptance runs used GDI test documents and the desktop application's own buttons, not Restora or a browser print dialog.

Public-source inspection on 2026-11-17 found the deployed entry bundle `index-RMwExQDm.js`. Its order-print function tries a server `/print-jobs` request, then a branch-configured local print bridge, then a browser iframe fallback. The fallback calls the iframe's `window.print()` and sets an 80 mm custom page size. Receipt and kitchen-slip layouts differ. This is source inspection, not evidence that a live print succeeded.

A real test needs a signed-in Restora browser on the same Windows machine as the installed virtual queue and an existing authorized test order. Record which path is used; a server/bridge success message alone does not prove the local virtual queue received a job. Verify the resulting PDF against the order's items, quantities, prices, tax, totals, payment stamp, notes and any logo. Check receipt width, long-order pagination, clipping, reprinting and duplicate handling. Use the actual order Print action; browser Save as PDF bypasses the Windows printer and is insufficient.

This session has no callable browser-control tool or authenticated Restora session. No orders were created, modified, paid, or queued for printing during this inspection.

## Reuse and structure review

The Rust workspace separates transport/delivery and shared domain data into `crates/core`, with Windows-specific operations under `src/platform`. The UI shares `PrinterCard` and a central `Tokens` global. Those are concrete reusable boundaries.

The source is not fully refactored. `src/service.rs` combines worker orchestration, native operations and persistence, while `src/main.rs` combines CLI startup, UI binding and settings synchronization. Some PowerShell automation locator/wait logic is duplicated. Refactoring should preserve the verified behavior and be followed by relevant native regression tests.

The application's design is **not 100% matched** to the supplied references. See [client testing](client-testing.md) for the measured differences. No new design-conformance or Restora-compatibility claim is made here.

## Authenticated browser checks (2026-11-17)

Using the owner's explicitly supplied test account, the browser test operated an existing order's **Print receipt**, **Print kitchen slip**, and receipt reprint actions. No order items, payment status, or restaurant settings were changed. All three server print requests returned HTTP 409; the site then invoked its browser iframe print fallback. These observations supersede the earlier statement that the authenticated flow had not been exercised.

Private Chromium reference renders contained the order items and receipt total. The kitchen slip fit on one approximately 80 mm wide page. Both receipt renders produced two pages: the staff/printed-time footer appeared alone on page two. This is a reproduced browser-render pagination defect; it is not yet a result from the Windows printer driver. Account data, credentials, HTML and PDFs remain outside the repository and were not published as CI artifacts.

The remaining check is the actual browser-to-Windows-queue path. A proposed temporary Windows 11 test would install the existing tested application, create a uniquely named PDF queue, authenticate in headful Edge, operate those same three existing-order print actions, and verify the resulting Microsoft-driver PDFs. It would create no orders, change no payments, upload no order contents/screenshots/PDFs, log only redacted status and page dimensions, and remove temporary credentials and queues afterward.

That Windows execution has not been run. Automatic approval review rejected the earlier proposed account-data artifact export because that destination and payload were not explicitly authorized. Explicit approval is needed before using the test account in the proposed GitHub-hosted Windows run. The earlier application-only Windows checks remain valid independently of this site-specific check.
