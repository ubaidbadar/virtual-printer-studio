# Virtual Printer Studio for Apple Silicon

Native arm64 application for Apple Silicon Macs (M1 and later); no Rosetta required. The deployment target is macOS 12 or later. Validation runs on macOS 15, not every macOS release.

Download the DMG, open it, and drag Virtual Printer Studio to Applications. Eject the disk image and open the application from Applications. The ZIP is an alternative containing the same app bundle; extract it with Finder to preserve permissions.

This build uses an ad hoc integrity signature. It is not signed with an Apple Developer ID or notarized. If macOS blocks first launch, use System Settings → Privacy & Security → Open Anyway after confirming you trust this download. Developer ID signing and notarization require the owner's Apple Developer credentials; none are included in the repository.

Create a printer and select its output folder. macOS asks for administrator permission to install its CUPS PDF filter and create the printer. This uses a native authorization dialog and does not open Terminal. Select the printer in another application's Print dialog. Keep the application open or running in the menu bar to receive PDFs.

## macOS printing architecture

The queue uses macOS's built-in PDF print pipeline and a small filter entry point from the bundled executable. Administrator setup installs a root-owned copy under `/Library/Printers/VirtualPrinterStudio`, plus a queue configuration readable only by root and the `_lp` printing account. Queues are not shared and accept jobs only from the configuring macOS account.

The filter and application communicate through a Unix socket under an owner-controlled directory in `/private/tmp`. Both sides check the kernel's peer UID before sending documents. Root and `_lp` are the only capture submitters. A second account cannot impersonate the receiver. No listener is exposed to the network, no printer process writes to the chosen output folder, and no CUPS security setting is weakened.

The same durable, byte-counted capture and PDF delivery engine serves macOS and Windows. Filenames, folders, activity, retry recovery, profile import/export, and PDF opening use the same implementation. Pausing retains queued documents. Quit stops immediate delivery; use Retry pending after reopening if the queue needs attention.

Mac-specific behavior: Finder reveals saved files, Start at login uses a per-user LaunchAgent, the menu bar keeps capture running after closing the window, and appearance reads the macOS system preference when refreshed. Saved notifications use macOS Notification Centre; Windows notification action buttons and live Windows registry theme updates do not apply. The UI retains the supplied Fluent design with a macOS font.

To remove a printer, delete it inside the app after delivering or cancelling pending jobs. Disable Start at login, quit, and delete the app to uninstall. Saved PDFs and application data are preserved. The shared root-owned filter may be removed by an administrator after removing all Virtual Printer Studio queues.

## Verification

The Apple Silicon macOS workflow runs formatting, portable tests, Clippy, native release compilation, bundle signature/architecture checks, CUPS integration, and all nine UI captures. `scripts/qa/macos-smoke.py` creates only temporary printers and synthetic documents and records which native printing/recovery checks completed. Its evidence is separate from the downloadable package.

A successful build is not a claim of pixel-perfect macOS UI or live Restora testing on a Mac. The Windows package remains separate.

Implementation references: [Apple CUPS filter interface](https://apple.github.io/cups/doc/man-filter.html), [PPD filter declarations](https://www.cups.org/doc/spec-ppd.html), and [CUPS file-device policy](https://apple.github.io/cups/doc/man-cups-files.conf.html). The `file:///dev/null` sink is explicitly allowed by CUPS; the custom PDF filter publishes the document to the authenticated receiver before that sink completes.
