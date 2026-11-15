$ErrorActionPreference = 'Stop'
try {
    $r = [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('__REQUEST__')) | ConvertFrom-Json
    if ($r.id -notmatch '^[A-Za-z0-9-]{1,80}$' -or $r.sid -notmatch '^S-1-5-[0-9-]+$' -or $r.queue -notmatch '^[A-Za-z0-9]{1,32}$' -or $r.port -lt 1 -or $r.port -gt 65535) { throw 'Invalid printer identity or capture address.' }
    $portName = 'VPS_' + $r.id
    $stage = 'Checking the Windows Print Spooler'
    Start-Service Spooler -ErrorAction Stop
    Add-Type -TypeDefinition @'
using System;using System.Runtime.InteropServices;
public class VpsSpooler {
 [StructLayout(LayoutKind.Sequential)] public struct Defaults { public IntPtr DataType; public IntPtr DevMode; public uint Access; }
 [DllImport("winspool.drv", EntryPoint="OpenPrinterW", CharSet=CharSet.Unicode, SetLastError=true)] private static extern bool OpenPrinter(string name, out IntPtr printer, ref Defaults defaults);
 [DllImport("winspool.drv", SetLastError=true)] private static extern bool SetPrinter(IntPtr printer,uint level,IntPtr info,uint command);
 [DllImport("winspool.drv")] private static extern bool ClosePrinter(IntPtr printer);
 [DllImport("winspool.drv", EntryPoint="DeletePortW", CharSet=CharSet.Unicode, SetLastError=true)] private static extern bool DeletePort(string server,IntPtr window,string name);
 public static bool RemovePort(string name){return DeletePort(null,IntPtr.Zero,name);}
 public static void Pause(string name,bool enabled) {
  Defaults defaults=new Defaults();defaults.Access=4;IntPtr handle;
  if(!OpenPrinter(name,out handle,ref defaults))throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());
  try{if(!SetPrinter(handle,0,IntPtr.Zero,enabled?2u:1u))throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());}finally{ClosePrinter(handle);}
 }
}
'@
    $ownerRoot = 'HKLM:\SOFTWARE\VirtualPrinterStudio\Owners'
    if (!(Test-Path -LiteralPath $ownerRoot)) { New-Item -Path $ownerRoot -Force | Out-Null }
    $owner = Get-ItemPropertyValue -LiteralPath $ownerRoot -Name $r.id -ErrorAction SilentlyContinue
    if ($owner -and $owner -ne $r.sid) { throw 'This printer belongs to another Windows account.' }
    $printer = Get-Printer | Where-Object PortName -eq $portName | Select-Object -First 1
    if ($r.action -eq 'remove') {
        $stage = 'Removing the Windows printer'
        if ($printer -and @(Get-PrintJob -PrinterName $printer.Name).Count -gt 0) { throw 'Windows still has pending print jobs. Resume or cancel them before deleting this printer.' }
        if ($printer) { Remove-Printer -Name $printer.Name -ErrorAction Stop }
        $existing = Get-CimInstance -ClassName Win32_TCPIPPrinterPort -Filter "Name='$portName'"
        if ($existing) {
            $removed = $false
            for ($i=0;$i -lt 25;$i++) {
                if ([VpsSpooler]::RemovePort($portName)) { $removed=$true;break }
                Start-Sleep -Milliseconds 200
            }
            if (!$removed) { throw 'Windows has not released the printer port. Wait a moment and retry deletion.' }
        }
        # Retain the ownership record so a failed desktop commit can be repaired.
    } else {
        if ($r.action -eq 'ensure') {
            $stage = 'Enabling Microsoft Print to PDF'
            if (!(Get-PrinterDriver -Name 'Microsoft Print To PDF' -ErrorAction SilentlyContinue)) {
                $feature = Enable-WindowsOptionalFeature -Online -FeatureName 'Printing-PrintToPDFServices-Features' -All -NoRestart
                if ($feature.RestartNeeded) { throw 'Restart Windows to finish enabling Print to PDF, then repair this printer.' }
                Add-PrinterDriver -Name 'Microsoft Print To PDF' -ErrorAction Stop
            }
            $stage = 'Configuring the built-in LPR transport'
            $existing = Get-CimInstance -ClassName Win32_TCPIPPrinterPort -Filter "Name='$portName'"
            if ($existing) {
                if ($existing.HostAddress -ne '127.0.0.1' -or $existing.Queue -ne $r.queue -or $existing.Protocol -ne 2) { throw 'This port is not owned by the configured virtual printer.' }
                if ($existing.PortNumber -ne $r.port -or !$existing.ByteCount -or $existing.SNMPEnabled) {
                    Set-CimInstance -InputObject $existing -Property @{PortNumber=[uint32]$r.port;ByteCount=$true;SNMPEnabled=$false} | Out-Null
                }
            } else {
                New-CimInstance -ClassName Win32_TCPIPPrinterPort -Property @{Name=$portName;Protocol=[uint32]2;HostAddress='127.0.0.1';PortNumber=[uint32]$r.port;Queue=$r.queue;ByteCount=$true;SNMPEnabled=$false} | Out-Null
            }
            $stage = 'Creating the Windows printer'
            $collision = Get-Printer -Name $r.name -ErrorAction SilentlyContinue
            if ($collision -and $collision.PortName -ne $portName) { throw 'Another Windows printer uses this name. Choose another name.' }
            if (!$printer) { Add-Printer -Name $r.name -DriverName 'Microsoft Print To PDF' -PortName $portName }
            elseif ($printer.Name -ne $r.name) { Rename-Printer -Name $printer.Name -NewName $r.name }
            $printer = Get-Printer -Name $r.name -ErrorAction Stop
            # Only this account, SYSTEM and administrators may submit jobs.
            $permissions = 'O:BAG:SYD:(A;;GA;;;SY)(A;;GA;;;BA)(A;;0x00020008;;;' + $r.sid + ')'
            Set-Printer -Name $printer.Name -PermissionSDDL $permissions
            Set-ItemProperty -LiteralPath $ownerRoot -Name $r.id -Value $r.sid
        }
        if (!$printer) { throw 'The Windows printer is missing. Select Install or repair.' }
        $stage = 'Applying pause or resume'
        [VpsSpooler]::Pause($printer.Name,[bool]$r.enabled)
    }
    @{ok=$true;error=''} | ConvertTo-Json -Compress | Set-Content -LiteralPath $r.result -Encoding UTF8
    exit 0
} catch {
    if ($r -and $r.result) { @{ok=$false;error=("${stage}: " + $_.Exception.Message)} | ConvertTo-Json -Compress | Set-Content -LiteralPath $r.result -Encoding UTF8 }
    Write-Output ("${stage}: " + $_.Exception.Message)
    exit 1
}
