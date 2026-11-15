# Run from an elevated Windows PowerShell or PowerShell terminal.
param([switch]$Uninstall)
$ErrorActionPreference = 'Stop'
$principal = New-Object Security.Principal.WindowsPrincipal([Security.Principal.WindowsIdentity]::GetCurrent())
if (!$principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) { throw 'Run this installer from an administrator PowerShell terminal.' }
$destination = Join-Path $env:ProgramFiles 'Virtual Printer Studio'
$shortcut = Join-Path ([Environment]::GetFolderPath('CommonPrograms')) 'Virtual Printer Studio.lnk'
if ($Uninstall) {
    if (@(Get-Printer | Where-Object { $_.PortName -like 'VPS_*' }).Count -gt 0) { throw 'Delete the app printers from Virtual Printer Studio before uninstalling. This protects pending captured documents.' }
    if (Test-Path -LiteralPath $shortcut) { Remove-Item -LiteralPath $shortcut }
    if (Test-Path -LiteralPath $destination) { Remove-Item -LiteralPath $destination -Recurse -Force }
    Remove-ItemProperty -LiteralPath 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run' -Name VirtualPrinterStudio -ErrorAction SilentlyContinue
    Write-Host 'Application removed. User settings, saved PDFs and recovery captures were retained.'

    return
}
foreach ($file in @('virtual-printer-studio.exe')) {
    if (!(Test-Path -LiteralPath (Join-Path $PSScriptRoot $file))) { throw "Missing $file. Extract the full Windows package first." }
}
[IO.Directory]::CreateDirectory($destination) | Out-Null
foreach ($file in @('virtual-printer-studio.exe','install.ps1','README.md')) {
    $source = Join-Path $PSScriptRoot $file
    if (Test-Path -LiteralPath $source) { Copy-Item -LiteralPath $source -Destination $destination -Force }
}
$shell = New-Object -ComObject WScript.Shell
$link = $shell.CreateShortcut($shortcut)
$link.TargetPath = Join-Path $destination 'virtual-printer-studio.exe'
$link.WorkingDirectory = $destination
$link.Description = 'Virtual printers that automatically save PDFs'
$link.Save()
Write-Host "Installed to $destination. Start Virtual Printer Studio from the Start menu."
