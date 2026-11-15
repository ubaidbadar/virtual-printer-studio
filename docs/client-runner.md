# Actual Windows 10 / 11 acceptance runner

Use a disposable **x64 Windows 10 or 11 VM**, with an interactive desktop. This runner executes repository code with administrator rights to install/remove temporary printers. Use the manual acceptance workflow on `main`; do not attach an elevated runner on a personal machine to untrusted pull-request jobs.

1. Download [setup-client-runner.ps1](../scripts/qa/setup-client-runner.ps1) to the VM.
2. Open **Windows PowerShell as Administrator** and run:

```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\setup-client-runner.ps1
```

3. Supply the one-hour registration token from [Settings → Actions → Runners → New self-hosted runner](https://github.com/ubaidbadar/virtual-printer-studio/settings/actions/runners/new) when prompted. It is not written into this repository.
4. Keep the terminal and desktop session open. The script verifies official runner/PowerShell ZIP SHA-256 digests and installs a portable runner in LocalAppData. No Windows service is installed. The runner automatically deregisters after one job.
5. Run **Windows client acceptance** from the repository's Actions tab. Select `windows-10-x64-self-hosted` or `windows-11-x64-self-hosted` and the successful QA build's run ID.

The job records the exact OS caption/build, requires Windows client `ProductType = 1`, and rejects Windows Server. Windows 11 ARM64 hosted testing is labeled **x64 emulated** and is not represented as native x64 testing.

The test creates only its randomly named VPS test printers and isolated evidence folders. Keep the evidence artifact; it contains the platform identity, printed PDF results and client-area screenshots. If a VM is interrupted during testing, restore its snapshot or remove the test printer/port before another run.
