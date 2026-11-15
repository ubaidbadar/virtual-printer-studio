param(
    [Parameter(Mandatory=$true)][string]$App,
    [Parameter(Mandatory=$true)][string]$Monitor,
    [string]$TestRoot = (Join-Path $env:TEMP ('VpsSmoke-' + [guid]::NewGuid().ToString()))
)
$ErrorActionPreference = 'Stop'
$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
$principal = New-Object Security.Principal.WindowsPrincipal($identity)
if (!$principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) { throw 'Run the printer integration test from an elevated PowerShell terminal.' }
$App = (Resolve-Path -LiteralPath $App).Path
$Monitor = (Resolve-Path -LiteralPath $Monitor).Path
[IO.Directory]::CreateDirectory($TestRoot) | Out-Null
$TestRoot = (Resolve-Path -LiteralPath $TestRoot).Path
$id = [guid]::NewGuid().ToString()
$name = 'VPS integration ' + $id.Substring(0,8)
$output = Join-Path $TestRoot 'PDFs'
[IO.Directory]::CreateDirectory($output) | Out-Null
$statePath = Join-Path $TestRoot 'state.json'
$printer = @{id=$id;name=$name;folder=$output;pattern='{title}_{counter}';enabled=$true;count=0;installed=$true;last_error='';open_after=$false;monthly_folders=$false}
@{printers=@($printer);activity=@();default_folder=$output;welcomed=$true;notifications=$false;keep_tray=$false} | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $statePath -Encoding utf8NoBOM
$template = Get-Content -LiteralPath (Join-Path $PSScriptRoot '..\src\platform\native.ps1') -Raw
function Configure([string]$action,[bool]$enabled=$true) {
    $result = Join-Path $TestRoot 'admin-result.json'
    if (Test-Path -LiteralPath $result) { Remove-Item -LiteralPath $result }
    $request = @{action=$action;id=$id;name=$name;enabled=$enabled;sid=$identity.User.Value;dll=$Monitor;result=$result} | ConvertTo-Json -Compress
    $script = $template.Replace('__REQUEST__',[Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($request)))
    $file = Join-Path $TestRoot 'configure.ps1'
    Set-Content -LiteralPath $file -Value $script -Encoding utf8NoBOM
    & powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File $file
    if (!(Test-Path -LiteralPath $result)) { throw "No result from native action $action" }
    $reply = Get-Content -LiteralPath $result -Raw | ConvertFrom-Json
    if (!$reply.ok) { throw "Native action ${action}: $($reply.error)" }
}
function Print-Document([string]$title) {
    Add-Type -AssemblyName System.Drawing
    $doc = New-Object Drawing.Printing.PrintDocument
    $doc.PrinterSettings.PrinterName = $name
    $doc.PrintController = New-Object Drawing.Printing.StandardPrintController
    $doc.DocumentName = $title
    if (!$doc.PrinterSettings.IsValid) { throw 'Temporary printer was not registered.' }
    $doc.add_PrintPage({param($sender,$e)
        $font = New-Object Drawing.Font('Arial',16)
        try { $e.Graphics.DrawString('Virtual Printer Studio integration test',$font,[Drawing.Brushes]::Black,72,72) } finally { $font.Dispose() }
        $e.HasMorePages = $false
    })
    try { $doc.Print() } finally { $doc.Dispose() }
}
$jobs = Join-Path $env:ProgramData ('VirtualPrinterStudio\jobs\' + $id)
$acks = Join-Path $env:ProgramData ('VirtualPrinterStudio\acks\' + $id)
function Wait-Captured([int]$count) {
    $deadline = (Get-Date).AddSeconds(90)
    while ((Get-Date) -lt $deadline) {
        $ready = @(Get-ChildItem -LiteralPath $jobs -Directory -Filter '*.ready')
        $unacked = @($ready | Where-Object { !(Test-Path -LiteralPath (Join-Path $acks ([IO.Path]::GetFileNameWithoutExtension($_.Name) + '.ack'))) })
        if ($unacked.Count -ge $count) { return }
        Start-Sleep -Milliseconds 300
    }
    Get-PrintJob -PrinterName $name | Format-List * | Out-String | Write-Host
    $log = Join-Path $env:ProgramData 'VirtualPrinterStudio\monitor.log'
    if (Test-Path -LiteralPath $log) { Get-Content -LiteralPath $log -Tail 40 | Write-Host }
    throw "Timed out waiting for $count captured jobs."
}
function Deliver([bool]$shouldSucceed=$true) {
    $p = Start-Process -FilePath $App -ArgumentList @('--process-pending','--state',('"' + $statePath + '"')) -Wait -PassThru
    if ($shouldSucceed -and $p.ExitCode -ne 0) { throw "Delivery failed with exit code $($p.ExitCode)." }
    if (!$shouldSucceed -and $p.ExitCode -eq 0) { throw 'Delivery unexpectedly succeeded with a missing output folder.' }
}
$installed = $false
try {
    Start-Service Spooler
    Get-PrinterDriver | Select-Object Name,PrinterEnvironment,MajorVersion | Format-Table | Out-String | Write-Host
    $controlName = 'VPS native control ' + $id.Substring(0,8)
    $base = Get-Printer | Where-Object DriverName -eq 'Microsoft Print To PDF' | Select-Object -First 1
    $controlPort = if ($base) { $base.PortName } else { 'PORTPROMPT:' }
    try {
        Add-Printer -Name $controlName -DriverName 'Microsoft Print To PDF' -PortName $controlPort -ErrorAction Stop
        Write-Host "Control queue created using $controlPort"
    } finally { if (Get-Printer -Name $controlName -ErrorAction SilentlyContinue) { Remove-Printer -Name $controlName } }
    Configure 'ensure'; $installed = $true
    Write-Host 'Installed temporary native printer.'
    foreach ($title in @('Invoice A','Invoice B','Invoice C')) { Print-Document $title }
    Wait-Captured 3
    Deliver
    $state = Get-Content -LiteralPath $statePath -Raw | ConvertFrom-Json
    if ($state.printers[0].count -ne 3 -or @($state.activity | Where-Object status -eq 'Saved').Count -ne 3) { throw 'Three distinct printed documents were not recorded.' }
    $files = @(Get-ChildItem -LiteralPath $output -Filter '*.pdf')
    if ($files.Count -ne 3) { throw 'Expected three distinct output PDFs.' }
    foreach ($file in $files) {
        $stream = [IO.File]::OpenRead($file.FullName)
        try { $header = New-Object byte[] 5; $stream.Read($header,0,5) | Out-Null; if ([Text.Encoding]::ASCII.GetString($header) -ne '%PDF-') { throw 'The print driver did not output PDF.' } } finally { $stream.Dispose() }
    }
    Deliver
    if (@(Get-ChildItem -LiteralPath $output -Filter '*.pdf').Count -ne 3) { throw 'Retry duplicated delivered PDFs.' }
    Configure 'pause' $false
    Configure 'pause' $true
    Rename-Item -LiteralPath $output -NewName 'Previous PDFs'
    Print-Document 'Recovery test'
    Wait-Captured 1
    Deliver $false
    [IO.Directory]::CreateDirectory($output) | Out-Null
    Deliver
    $state = Get-Content -LiteralPath $statePath -Raw | ConvertFrom-Json
    if ($state.printers[0].count -ne 4) { throw 'The failed print did not recover exactly once.' }
    Write-Host 'PASS: capture, multiple jobs, PDF delivery, no replay, pause/resume and missing-folder recovery.'
} finally {
    if ($installed) { Configure 'remove'; Write-Host 'Removed temporary printer.' }
    Write-Host "Evidence: $TestRoot"
}
