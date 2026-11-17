# Actual Windows client and visual testing

## Verified results

The tested x64 application source is `abb9cc1`, packaged by [Desktop checks 43773442326](https://github.com/ubaidbadar/virtual-printer-studio/actions/runs/43773442326). It passes 17 portable tests, an additional Windows installer-lock regression, formatting, Clippy and release builds.

| Target | Actual environment | Result | Evidence |
|---|---|---|---|
| Windows 11 | Enterprise build 26200, ARM64 host running the x64 application through Windows emulation | Full printer, GUI, background and scale checks passed | [Run 43775185171](https://github.com/ubaidbadar/virtual-printer-studio/actions/runs/43775185171) |
| Windows 10 | Enterprise LTSC Evaluation build 19044, native x64 guest in QEMU/KVM | Real driver/PDF checks passed; GUI automation is not yet verified | [Driver evidence 43775018710](https://github.com/ubaidbadar/virtual-printer-studio/actions/runs/43775018710) |
| Windows Server / Linux | Hosted Desktop checks | Build, lint and tests passed; six additional driver PDFs on Windows Server | [Run 43773442326](https://github.com/ubaidbadar/virtual-printer-studio/actions/runs/43773442326) |
| Design fidelity | Seven actual client-area renders against the supplied images | **Not pixel perfect** | [Measured Windows 11 differences](evidence/windows11/visual-comparison.json) |

The Windows 11 GUI test also closes the native window, prints from an external process while the app is hidden, and starts a second copy to verify that it reopens the original instance. Its two resulting PDFs are parsed independently. Both client OS driver outputs identify their PDF producer as **Microsoft: Print To PDF**.

[Download the tested x64 package](https://github.com/ubaidbadar/virtual-printer-studio/actions/runs/43773442326/artifacts/14185413148). ZIP SHA256: `210f1613a819d204a72a371d567203651a24b8e7242a9d953c5d52028097d29b`. The executable has the Windows GUI subsystem, statically linked CRT and only inbox Windows DLL imports.

Windows 10 evidence: [OS identity](evidence/windows10/platform.json), [successful guest result](evidence/windows10/result.json), [live GUI actions](evidence/windows10/gui-actions.json), and [visual differences](evidence/windows10/visual-comparison.json). The completed run independently parsed ten driver PDFs plus the GUI-generated test PDF. Its [full evidence artifact](https://github.com/ubaidbadar/virtual-printer-studio/actions/runs/44038396389/artifacts/14308857022) includes the actual PDFs and logs.

## How the tests run

The `windows-latest` build runner is Windows Server. Its printing tests are useful, but do not establish Windows 10 or Windows 11 client compatibility.

The manual **Windows client acceptance** workflow downloads the packaged x64 build and runs on actual Windows 11 ARM64, using Windows x64 emulation. Self-hosted native x64 choices also remain available. The OS guard records `Win32_OperatingSystem.ProductType`, build, processor architecture, session and source commit, and rejects Windows Server when a client OS is requested. The package artifact run is recorded by the workflow download step.

The **Windows 10 evaluation VM acceptance** workflow installs Microsoft's official Windows 10 Enterprise LTSC evaluation (build 19044.1288) inside a disposable QEMU/KVM VM on a hosted Linux runner. The image is downloaded directly from Microsoft and checked against SHA256 `e4ab2e3535be5748252a8d5d57539a6e59be8d6726345ee10e7afd2cb89fefb5`. No existing computer, licence key or self-hosted setup is needed. Only evidence is uploaded; VM disks, installation media and temporary credentials are excluded. A separate boot-progress artifact makes setup failures visible.

## Functional cases

The extended real-driver test installs an actual Microsoft Print to PDF queue using the inbox Standard TCP/IP LPR monitor. It covers idempotent repair, printer rename, authenticated capture, actual document titles, pause/resume, refusal to delete pending jobs, a print submitted while the app is stopped, missing-folder recovery, process restart, replay prevention, three-page documents, Unicode titles/content, repeated filenames and removal of the queue/port. Ten PDFs are parsed independently with pypdf to check page counts, dimensions and the printed text.

Native Windows UI Automation operates navigation and the new-printer dialog, including disabled background controls while a modal is open. A second test operates the normal application on a fresh disposable account: onboarding, entering a name/folder through native editable controls, saving a profile, installing its Windows printer, clicking Test PDF, pausing/resuming with the switch, and confirming deletion while retaining the PDF. That test PDF is parsed independently too. It refuses existing application data and cleans up its own temporary queue.

The Linux/Windows workspace tests cover core validation, persistence, delivery and transport. The Windows-only installer regression test specifically checks that the installer remains readable by PowerShell while other writers are denied. This guards a bug the live GUI test found which the direct printer script could not exercise.

## Visual result: not pixel perfect

`--ui-snapshot DIRECTORY` renders the actual application component with deterministic fixture data. It does not install printers or read/write user configuration. Seven client-area scenes are captured at 100%, 125% and 150% forced render scale. Each PNG must have the requested physical dimensions and nonempty content; native asynchronous resize events are allowed to settle before capture. These are render-scale checks, not physical multi-monitor acceptance.

`compare-visuals.py` crops only the supplied reference's 40-pixel title bar. It never resizes or aligns the actual image. It reports exact changed pixels, differences exceeding eight channel levels and mean absolute channel error, and writes raw difference images. Fixture content is not identical in every scene, so the numerical difference includes content as well as styling. The overall layout is visibly different regardless of fixture content: navigation, card contents, filter controls, activity presentation and the detail dialog do not match the supplied designs.

The [actual printer-page fixture](evidence/windows11/printers-fixture.png) and [capture provenance](evidence/windows11/provenance.json) are checked in for review. The named profiles in that image are deterministic visual fixtures; the real acceptance queues use unique temporary names.

The comparison currently reports **PIXEL-PERFECT: NO**. A successful measurement step means the comparison ran; it does not approve the design. Add `--require-pixel-perfect` to make any difference or uncaptured reference a failing gate. The tray/toast and logo design boards are reported as uncaptured; they are not silently treated as matches.

## Limits

Windows 11 ARM64 with x64 emulation is distinct from a native Windows 11 x64 machine. Windows 10 Enterprise LTSC is distinct from every Windows 10 edition/build. UI Automation verifies exposed controls and callbacks; it does not certify physical mouse targeting. Full-desktop diagnostic grabs can include overlapping OS setup windows, so visual comparisons use the application’s client buffer. The stopped-app driver case explicitly restarts the retained Windows job; natural monitor backoff timing is not certified by that case.

Neither one proves all hardware, enterprise policies, application print producers, physical monitor arrangements, Narrator speech, UAC secure-desktop interaction or notification activation. Actual OS/run results belong in the evidence artifacts; configuration alone is not a passing test.

## Fixes found by these tests

- The GUI installer held a write handle on its PowerShell source. PowerShell could not read that file. The app now closes the writer, reopens a read-only lock, verifies the contents under that lock, and retains it through elevation. The native regression checks execution and refusal of another writer; the Windows 11 GUI test verifies actual printer creation.
- The Windows LPR monitor encodes titles using an ANSI code page, losing Unicode characters. The receiver now queries the active Windows spooler job for its original UTF-16 title before acknowledging the transfer. Actual Unicode filenames and printed Unicode content passed on both client OSes.
- Windows accessibility support was omitted from the application features. It is now enabled. Native UI Automation passed on both client OSes. The Windows 10 harness now selects the actual application window by process ID, title and size instead of a 16-pixel helper window, and waits for native printer operations to complete before its next action.
- Modal dialogs previously allowed background controls to remain interactive. Background controls are now disabled and the overlay consumes pointer input.
