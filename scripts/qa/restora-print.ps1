param(
    [Parameter(Mandatory=$true)][string]$App,
    [string]$Report = (Join-Path $env:TEMP 'vps-restora-redacted.json'),
    [switch]$BrowserFixture,
    [string]$SyntheticEvidence
)
$ErrorActionPreference='Stop'
if (!$BrowserFixture -and (!$env:RESTORA_TEST_EMAIL -or !$env:RESTORA_TEST_PASSWORD)) {
    throw 'Supply RESTORA_TEST_EMAIL and RESTORA_TEST_PASSWORD through protected environment variables.'
}
$privateRoot=Join-Path $env:TEMP ('VpsRestora-'+[guid]::NewGuid().ToString())
$installed=$false
$defaultPrinter=$null
$network=$null
$exitCode=1
try {
    . (Join-Path $PSScriptRoot '..\windows-smoke.ps1') -App $App -TestRoot $privateRoot -FixtureOnly
    # Avoid order titles in filenames. State, logs and browser profile remain private.
    $state=Read-State
    $state.printers[0].pattern='restora_{counter}'
    $state|ConvertTo-Json -Depth 12|Set-Content -LiteralPath $statePath -Encoding utf8NoBOM
    $defaultPrinter=(Get-CimInstance Win32_Printer|Where-Object Default|Select-Object -First 1).Name
    $network=New-Object -ComObject WScript.Network
    Start-Capture *> $null
    $installed=$true
    Configure 'ensure' *> $null
    $network.SetDefaultPrinter($name)
    if (!(Get-CimInstance Win32_Printer|Where-Object { $_.Default -and $_.Name -eq $name })) {
        throw 'Could not select the temporary default printer.'
    }
    $testScript=if($BrowserFixture){'browser-print.py'}else{'restora-print.py'}
    & python (Join-Path $PSScriptRoot $testScript) --printer $name --state $statePath --output $output --work (Join-Path $TestRoot 'browser') --report $Report
    $exitCode=$LASTEXITCODE
} catch {
    # Never print raw browser, spooler or worker errors containing order metadata.
    Write-Host 'FAIL: Windows Restora fixture could not finish; private diagnostics suppressed.'
    $exitCode=1
} finally {
    Remove-Item Env:RESTORA_TEST_EMAIL,Env:RESTORA_TEST_PASSWORD -ErrorAction SilentlyContinue
    # This opt-in export is restricted to the static, unauthenticated fixture.
    # The live Restora path never copies its PDFs or diagnostics out of TEMP.
    if ($BrowserFixture -and $SyntheticEvidence -and $output -and (Test-Path -LiteralPath $output)) {
        try {
            New-Item -ItemType Directory -Path $SyntheticEvidence -Force|Out-Null
            Get-ChildItem -LiteralPath $output -Filter '*.pdf'|Copy-Item -Destination $SyntheticEvidence
            Get-PrintConfiguration -PrinterName $name|Select-Object PaperSize,Color,DuplexingMode|ConvertTo-Json|Set-Content (Join-Path $SyntheticEvidence 'paper.json')
            & powershell.exe -NoProfile -NonInteractive -File (Join-Path $PSScriptRoot 'inspect-paper.ps1') -PrinterName $name -OutputPath (Join-Path $SyntheticEvidence 'driver-paper.json')
            if ($LASTEXITCODE -ne 0) { throw 'Driver media inspection failed.' }
            if (Test-Path -LiteralPath $Report) { Copy-Item -LiteralPath $Report -Destination (Join-Path $SyntheticEvidence 'report.json') }
        } catch { Write-Host 'FAIL: Could not collect synthetic evidence.';$exitCode=1 }
    }
    if ($network -and $defaultPrinter) {
        try { $network.SetDefaultPrinter($defaultPrinter) }
        catch { Write-Host 'FAIL: Could not restore the previous default printer.';$exitCode=1 }
    }
    if (Get-Command Stop-Capture -ErrorAction SilentlyContinue) {
        try { Stop-Capture }
        catch { Write-Host 'FAIL: Could not stop the capture worker.';$exitCode=1 }
    }
    if ($installed) {
        try {
            # Safe here: this uniquely named test queue belongs only to this run.
            Get-PrintJob -PrinterName $name -ErrorAction SilentlyContinue|Remove-PrintJob -ErrorAction SilentlyContinue
            Configure 'remove' *> $null
        } catch { Write-Host 'FAIL: Could not remove the temporary queue.';$exitCode=1 }
    }
    if (Test-Path -LiteralPath $privateRoot) {
        try { Remove-Item -LiteralPath $privateRoot -Recurse -Force }
        catch { Write-Host 'FAIL: Could not remove private test files.';$exitCode=1 }
    }
}
exit $exitCode
