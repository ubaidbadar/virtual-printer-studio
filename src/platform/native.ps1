$ErrorActionPreference = 'Stop'
try {
    $r = [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('__REQUEST__')) | ConvertFrom-Json
    if ($r.id -notmatch '^[A-Za-z0-9-]{1,80}$' -or $r.sid -notmatch '^S-1-5-[0-9-]+$') { throw 'Invalid printer identity.' }
    $stage = 'Preparing secure capture folders'
    $root = Join-Path $env:ProgramData 'VirtualPrinterStudio'
    $monitor = 'Virtual Printer Studio 0.2'
    $port = 'VPS_' + $r.id
    Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public class VpsSpooler {
 [StructLayout(LayoutKind.Sequential, CharSet=CharSet.Unicode)] public struct MonitorInfo { public string Name; public string Environment; public string Dll; }
 [StructLayout(LayoutKind.Sequential, CharSet=CharSet.Unicode)] public struct PortInfo { public string Name; }
 [DllImport("winspool.drv", CharSet=CharSet.Unicode, SetLastError=true)] private static extern bool AddMonitor(string server, uint level, ref MonitorInfo info);
 [DllImport("winspool.drv", CharSet=CharSet.Unicode, SetLastError=true)] private static extern bool AddPortEx(string server, uint level, ref PortInfo info, string monitor);
 public static bool InstallMonitor(ref MonitorInfo info) { return AddMonitor(null,2,ref info); }
 public static bool InstallPort(ref PortInfo info, string monitor) { return AddPortEx(null,1,ref info,monitor); }
 [DllImport("winspool.drv", CharSet=CharSet.Unicode, SetLastError=true)] public static extern bool OpenPrinter(string name, out IntPtr printer, IntPtr defaults);
 [DllImport("winspool.drv", SetLastError=true)] public static extern bool SetPrinter(IntPtr printer, uint level, IntPtr info, uint command);
 [DllImport("winspool.drv")] public static extern bool ClosePrinter(IntPtr printer);
 [StructLayout(LayoutKind.Sequential, CharSet=CharSet.Unicode)] public struct Defaults { public IntPtr DataType; public IntPtr DevMode; public uint Access; }
 [DllImport("winspool.drv", EntryPoint="OpenPrinterW", CharSet=CharSet.Unicode, SetLastError=true)] public static extern bool OpenAdminPrinter(string name, out IntPtr printer, ref Defaults defaults);
}
'@
    function Assert-NotReparse([string]$path) {
        if ((Test-Path -LiteralPath $path) -and ((Get-Item -LiteralPath $path -Force).Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw "Refusing a redirected application folder: $path" }
    }
    function Secure-Directory([string]$path, [string]$sid, [string]$rights) {
        Assert-NotReparse $path
        [IO.Directory]::CreateDirectory($path) | Out-Null
        $acl = New-Object Security.AccessControl.DirectorySecurity
        $acl.SetAccessRuleProtection($true, $false)
        $admins = New-Object Security.Principal.SecurityIdentifier('S-1-5-32-544')
        $acl.SetOwner($admins)
        foreach ($identity in @('S-1-5-18', 'S-1-5-32-544')) {
            $identitySid = New-Object Security.Principal.SecurityIdentifier($identity)
            $rule = New-Object Security.AccessControl.FileSystemAccessRule($identitySid, 'FullControl', 'ContainerInherit,ObjectInherit', 'None', 'Allow')
            $acl.AddAccessRule($rule)
        }
        if ($sid) {
            $identitySid = New-Object Security.Principal.SecurityIdentifier($sid)
            $inherit = if ($sid -eq 'S-1-5-11') { 'None' } else { 'ContainerInherit,ObjectInherit' }
            $rule = New-Object Security.AccessControl.FileSystemAccessRule($identitySid, $rights, $inherit, 'None', 'Allow')
            $acl.AddAccessRule($rule)
        }
        Set-Acl -LiteralPath $path -AclObject $acl
    }
    Secure-Directory $root 'S-1-5-11' 'ReadAndExecute'
    foreach ($name in @('ports','owners')) { Secure-Directory (Join-Path $root $name) '' '' }
    foreach ($name in @('jobs','acks')) { Secure-Directory (Join-Path $root $name) 'S-1-5-11' 'ReadAndExecute' }
    $ownerPath = Join-Path $root ('owners\' + $r.id + '.txt')
    $jobs = Join-Path $root ('jobs\' + $r.id)
    $acks = Join-Path $root ('acks\' + $r.id)
    if (Test-Path -LiteralPath $ownerPath) {
        Assert-NotReparse $ownerPath
        if ((Get-Content -LiteralPath $ownerPath -Raw).Trim() -ne $r.sid) { throw 'This printer belongs to another Windows account.' }
    } elseif ((Test-Path -LiteralPath $jobs) -or (Test-Path -LiteralPath $acks)) {
        throw 'An unregistered capture folder already exists. Contact support before reusing it.'
    }
    if ($r.action -eq 'ensure') { Start-Service Spooler -ErrorAction Stop }
    $stage = 'Reading Windows printers'
    $printer = Get-Printer -ErrorAction Stop | Where-Object PortName -eq $port | Select-Object -First 1
    if ($r.action -eq 'remove') {
        if ($printer -and @(Get-PrintJob -PrinterName $printer.Name -ErrorAction Stop).Count -gt 0) { throw 'The printer has pending Windows print jobs. Resume or cancel those jobs before removing it.' }
        if (Test-Path -LiteralPath $jobs) {
            foreach ($job in Get-ChildItem -LiteralPath $jobs -Directory) {
                if ($job.Name -match '^[A-Za-z0-9-]+\.partial$') { continue } # Incomplete/cancelled jobs are retained for recovery.
                if ($job.Name -notmatch '^[A-Za-z0-9-]+\.ready$') { throw 'Unexpected capture data was retained. Contact support before deleting this printer.' }
                $jobId = $job.Name.Substring(0,$job.Name.Length - 6)
                if (!(Test-Path -LiteralPath (Join-Path $acks ($jobId + '.ack')))) { throw 'Captured jobs are still waiting for delivery. Retry them before deleting this printer.' }
            }
        }
        if ($printer) { Remove-Printer -Name $printer.Name -ErrorAction Stop }
        if (Get-PrinterPort -Name $port -ErrorAction SilentlyContinue) { Remove-PrinterPort -Name $port -ErrorAction Stop }
        # Captured files are kept for recovery; successful jobs are acknowledged.
        # Retain ownership metadata so a failed desktop-state commit can be repaired safely.
    } else {
        if ($r.action -eq 'ensure') {
            if (![Environment]::Is64BitOperatingSystem -or ![Environment]::Is64BitProcess) { throw 'Install the x64 application on a 64-bit Windows installation.' }
            Secure-Directory $jobs $r.sid 'ReadAndExecute'
            Secure-Directory $acks $r.sid 'Modify'
            Set-Content -LiteralPath $ownerPath -Value $r.sid -Encoding ASCII
            $dll = Join-Path ([Environment]::SystemDirectory) 'vps_port_monitor_0_2.dll'
            $monitorKey = 'HKLM:\SYSTEM\CurrentControlSet\Control\Print\Monitors\' + $monitor
            $stage = 'Installing the port monitor'
            if (!(Test-Path -LiteralPath $monitorKey)) {
                if (!(Test-Path -LiteralPath $r.dll)) { throw 'The printer monitor DLL is missing. Extract the complete Windows download before starting the app.' }
                Copy-Item -LiteralPath $r.dll -Destination $dll -Force
                $info = New-Object VpsSpooler+MonitorInfo
                $info.Name = $monitor; $info.Environment = 'Windows x64'; $info.Dll = [IO.Path]::GetFileName($dll)
                if (![VpsSpooler]::InstallMonitor([ref]$info)) { throw (New-Object ComponentModel.Win32Exception([Runtime.InteropServices.Marshal]::GetLastWin32Error())) }
            }
            $stage = 'Checking the Microsoft PDF driver'
            if (!(Get-PrinterDriver -Name 'Microsoft Print To PDF' -ErrorAction SilentlyContinue)) {
                $feature = Enable-WindowsOptionalFeature -Online -FeatureName 'Printing-PrintToPDFServices-Features' -All -NoRestart
                if ($feature.RestartNeeded) { throw 'Restart Windows to finish enabling Microsoft Print to PDF, then repair this printer.' }
                Add-PrinterDriver -Name 'Microsoft Print To PDF' -ErrorAction Stop
            }
            $stage = 'Registering the capture port'
            if (!(Get-PrinterPort -Name $port -ErrorAction SilentlyContinue)) {
                $info = New-Object VpsSpooler+PortInfo; $info.Name = $port
                if (![VpsSpooler]::InstallPort([ref]$info, $monitor)) { throw (New-Object ComponentModel.Win32Exception([Runtime.InteropServices.Marshal]::GetLastWin32Error())) }
            }
            $stage = 'Checking printer name availability'
            $collision = Get-Printer -Name $r.name -ErrorAction SilentlyContinue
            if ($collision -and $collision.PortName -ne $port) { throw 'Another Windows printer already uses this name. Choose another printer name.' }
            $stage = 'Creating the Windows printer'
            if (!$printer) { Add-Printer -Name $r.name -DriverName 'Microsoft Print To PDF' -PortName $port -ErrorAction Stop }
            elseif ($printer.Name -ne $r.name) { Rename-Printer -Name $printer.Name -NewName $r.name -ErrorAction Stop }
            $printer = Get-Printer -Name $r.name -ErrorAction Stop
        }
        if (!$printer) { throw 'The Windows printer is missing. Use Install or repair to restore it.' }
        $stage = 'Applying printer pause or resume'
        $defaults = New-Object VpsSpooler+Defaults; $defaults.Access = 4 # PRINTER_ACCESS_ADMINISTER
        $handle = [IntPtr]::Zero
        if (![VpsSpooler]::OpenAdminPrinter($printer.Name, [ref]$handle, [ref]$defaults)) { throw (New-Object ComponentModel.Win32Exception([Runtime.InteropServices.Marshal]::GetLastWin32Error())) }
        try {
            $control = if ($r.enabled) { 2 } else { 1 } # resume / pause
            if (![VpsSpooler]::SetPrinter($handle,0,[IntPtr]::Zero,$control)) { throw (New-Object ComponentModel.Win32Exception([Runtime.InteropServices.Marshal]::GetLastWin32Error())) }
        } finally { [VpsSpooler]::ClosePrinter($handle) | Out-Null }
    }
    @{ok=$true; error=''} | ConvertTo-Json -Compress | Set-Content -LiteralPath $r.result -Encoding UTF8
    exit 0
} catch {
    Write-Output ("Setup failed at ${stage}: " + $_.Exception.Message + "`n" + $_.ScriptStackTrace)
    if ($r -and $r.result) { @{ok=$false; error=("${stage}: " + $_.Exception.Message)} | ConvertTo-Json -Compress | Set-Content -LiteralPath $r.result -Encoding UTF8 }
    exit 1
}
