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
    $monitor = 'Virtual Printer Studio 0.2'
    $monitorKey = 'HKLM:\SYSTEM\CurrentControlSet\Control\Print\Monitors\' + $monitor
    if (Test-Path -LiteralPath $monitorKey) {
        foreach ($port in @(Get-PrinterPort | Where-Object PortMonitor -eq $monitor)) { Remove-PrinterPort -Name $port.Name -ErrorAction Stop }
        Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public class VpsUninstall {
 [DllImport("winspool.drv", CharSet=CharSet.Unicode, SetLastError=true)] public static extern bool DeleteMonitor(string server, string environment, string name);
 [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)] public static extern bool MoveFileEx(string existing, string replacement, uint flags);
}
'@
        if (![VpsUninstall]::DeleteMonitor($null,'Windows x64',$monitor)) { throw (New-Object ComponentModel.Win32Exception([Runtime.InteropServices.Marshal]::GetLastWin32Error())) }
        $dll = Join-Path ([Environment]::SystemDirectory) 'vps_port_monitor_0_2.dll'
        if (Test-Path -LiteralPath $dll) {
            try { Remove-Item -LiteralPath $dll -Force -ErrorAction Stop } catch {
                if (![VpsUninstall]::MoveFileEx($dll,$null,4)) { throw 'The monitor was unregistered, but its DLL could not be scheduled for removal.' }
                Write-Host 'The unloaded monitor DLL will be removed at the next Windows restart.'
            }
        }
    }
    Remove-ItemProperty -LiteralPath 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run' -Name VirtualPrinterStudio -ErrorAction SilentlyContinue
    Write-Host 'Application and port monitor removed. User settings, saved PDFs and recovery captures were retained.'

    return
}
foreach ($file in @('virtual-printer-studio.exe','vps_port_monitor.dll')) {
    if (!(Test-Path -LiteralPath (Join-Path $PSScriptRoot $file))) { throw "Missing $file. Extract the full Windows package first." }
}
[IO.Directory]::CreateDirectory($destination) | Out-Null
foreach ($file in @('virtual-printer-studio.exe','vps_port_monitor.dll','install.ps1','README.md')) {
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
