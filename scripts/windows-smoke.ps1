param(
    [Parameter(Mandatory=$true)][string]$App,
    [string]$TestRoot = (Join-Path $env:TEMP ('VpsSmoke-' + [guid]::NewGuid().ToString()))
)
$ErrorActionPreference = 'Stop'
$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
$principal = New-Object Security.Principal.WindowsPrincipal($identity)
if (!$principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) { throw 'Run this integration test from an elevated PowerShell terminal.' }
$App = (Resolve-Path -LiteralPath $App).Path
[IO.Directory]::CreateDirectory($TestRoot) | Out-Null
$TestRoot = (Resolve-Path -LiteralPath $TestRoot).Path
$id = [guid]::NewGuid().ToString()
$name = 'VPS integration ' + $id.Substring(0,8)
$queue = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData([Text.Encoding]::UTF8.GetBytes($id))).Substring(0,24).ToLowerInvariant()
$output = Join-Path $TestRoot 'PDFs'
[IO.Directory]::CreateDirectory($output) | Out-Null
$statePath = Join-Path $TestRoot 'state.json'
$printer = @{id=$id;name=$name;folder=$output;pattern='{title}_{counter}';enabled=$true;count=0;installed=$true;last_error='';open_after=$false;monthly_folders=$false}
@{printers=@($printer);activity=@();default_folder=$output;welcomed=$true;notifications=$false;keep_tray=$false;capture_port=0} | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $statePath -Encoding utf8NoBOM
$template = Get-Content -LiteralPath (Join-Path $PSScriptRoot '..\src\platform\native.ps1') -Raw
$script:server = $null
function Read-State { Get-Content -LiteralPath $statePath -Raw | ConvertFrom-Json }
function Start-Capture {
    $script:server = Start-Process -FilePath $App -ArgumentList @('--serve','--state',('"' + $statePath + '"')) -PassThru -RedirectStandardOutput (Join-Path $TestRoot 'server-out.log') -RedirectStandardError (Join-Path $TestRoot 'server-error.log')
    $deadline = (Get-Date).AddSeconds(15)
    while ((Get-Date) -lt $deadline) {
        $script:server.Refresh()
        if ($script:server.HasExited) { throw ('Capture worker exited: ' + (Get-Content -LiteralPath (Join-Path $TestRoot 'server-error.log') -Raw)) }
        $state = Read-State
        if ($state.capture_port -gt 0) {
            $listener = Get-NetTCPConnection -LocalPort $state.capture_port -State Listen -ErrorAction SilentlyContinue
            if ($listener) { return }
        }
        Start-Sleep -Milliseconds 150
    }
    throw 'The capture receiver did not start.'
}
function Stop-Capture {
    if ($script:server) {
        $script:server.Refresh()
        if (!$script:server.HasExited) { Stop-Process -Id $script:server.Id -Force; $script:server.WaitForExit() }
        $script:server=$null
    }
}
function Configure([string]$action,[bool]$enabled=$true) {
    $result = Join-Path $TestRoot 'admin-result.json'
    if (Test-Path -LiteralPath $result) { Remove-Item -LiteralPath $result }
    $port = (Read-State).capture_port
    $request = @{action=$action;id=$id;name=$name;enabled=$enabled;sid=$identity.User.Value;port=$port;queue=$queue;result=$result} | ConvertTo-Json -Compress
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
function Wait-Saved([int]$count) {
    $deadline = (Get-Date).AddSeconds(90)
    while ((Get-Date) -lt $deadline) {
        $state=Read-State
        if ($state.printers[0].count -eq $count) { return }
        Start-Sleep -Milliseconds 200
    }
    $log=Join-Path $TestRoot 'logs/app.log'
    if (Test-Path -LiteralPath $log) { Get-Content -LiteralPath $log -Tail 40 | Write-Host }
    if (Test-Path -LiteralPath (Join-Path $TestRoot 'server-out.log')) { Get-Content -LiteralPath (Join-Path $TestRoot 'server-out.log') -Tail 40 | Write-Host }
    Get-PrintJob -PrinterName $name | Format-List * | Out-String | Write-Host
    throw "Timed out waiting for $count delivered PDFs."
}
$installed=$false
try {
    Start-Capture
    Configure 'ensure';$installed=$true
    Write-Host 'Installed PDF printer using the inbox Standard TCP/IP LPR monitor.'
    # An ordinary local process must be unable to inject a print into the receiver.
    $client=New-Object Net.Sockets.TcpClient('127.0.0.1',[int](Read-State).capture_port)
    try { $stream=$client.GetStream();$bytes=[Text.Encoding]::ASCII.GetBytes(([char]2)+$queue+"`n");$stream.Write($bytes,0,$bytes.Length);$stream.ReadTimeout=3000;try{$answer=$stream.ReadByte()}catch{$answer=-1};if($answer -eq 0){throw 'The receiver accepted a non-spooler connection.'} } finally {$client.Dispose()}
    foreach ($title in @('Invoice A','Invoice B','Invoice C')) { Print-Document $title }
    Wait-Saved 3
    $files=@(Get-ChildItem -LiteralPath $output -Filter '*.pdf')
    if ($files.Count -ne 3) { throw 'Expected three distinct output PDFs.' }
    foreach ($file in $files) {
        $stream=[IO.File]::OpenRead($file.FullName)
        try{$header=New-Object byte[] 5;$stream.Read($header,0,5)|Out-Null;if([Text.Encoding]::ASCII.GetString($header) -ne '%PDF-'){throw 'Output is not a PDF.'}}finally{$stream.Dispose()}
    }
    if (!($files.Name -match 'Invoice A')) { throw 'The document title was not preserved in the output filename.' }
    Configure 'pause' $false
    Print-Document 'Paused job'
    Start-Sleep -Milliseconds 400
    if ((Read-State).printers[0].count -ne 3) { throw 'A paused printer delivered a new job.' }
    Configure 'pause' $true
    Wait-Saved 4
    Write-Host 'PASS: multiple PDFs, actual document titles, authenticated capture and pause/resume.'
    Stop-Capture
    Print-Document 'Printed while app stopped'
    if (@(Get-PrintJob -PrinterName $name).Count -lt 1) { throw 'Windows did not retain a job while the app was stopped.' }
    Start-Capture
    Get-PrintJob -PrinterName $name | ForEach-Object { Restart-PrintJob -PrinterName $name -ID $_.ID -ErrorAction SilentlyContinue }
    Wait-Saved 5
    Write-Host 'PASS: Windows retained and delivered printing while the app was stopped.'
    Rename-Item -LiteralPath $output -NewName 'Previous PDFs'
    Print-Document 'Recovery test'
    $deadline=(Get-Date).AddSeconds(60)
    while ((Get-Date) -lt $deadline -and @((Read-State).activity | Where-Object status -like 'Failed:*').Count -eq 0) { Start-Sleep -Milliseconds 200 }
    if (@((Read-State).activity | Where-Object status -like 'Failed:*').Count -eq 0) { throw 'Missing-folder failure was not recorded.' }
    Stop-Capture
    [IO.Directory]::CreateDirectory($output) | Out-Null
    Start-Capture
    Wait-Saved 6
    Stop-Capture
    $process=Start-Process -FilePath $App -ArgumentList @('--process-pending','--state',('"'+$statePath+'"')) -Wait -PassThru
    if ($process.ExitCode -ne 0) { throw 'Final retry reported undelivered jobs.' }
    if (@(Get-ChildItem -LiteralPath $TestRoot -Recurse -Filter '*.pdf').Count -ne 6) { throw 'Retry duplicated or lost a delivered PDF.' }
    Write-Host 'PASS: missing-folder recovery, application restart and replay prevention.'
} finally {
    Stop-Capture
    if ($installed) { Configure 'remove';Write-Host 'Removed the temporary Windows printer and port.' }
    Write-Host "Evidence: $TestRoot"
}
