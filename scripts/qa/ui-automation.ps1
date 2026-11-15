param([Parameter(Mandatory=$true)][string]$App,[Parameter(Mandatory=$true)][string]$OutputDirectory)
$ErrorActionPreference='Stop'
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
. (Join-Path $PSScriptRoot 'process-exit.ps1')
. (Join-Path $PSScriptRoot 'ui-diagnostics.ps1')
[IO.Directory]::CreateDirectory($OutputDirectory)|Out-Null
$App=(Resolve-Path -LiteralPath $App).Path
$OutputDirectory=(Resolve-Path -LiteralPath $OutputDirectory).Path
$env:VPS_QA_HOLD_SECONDS='60'
$env:SLINT_BACKEND='winit-software'
$env:SLINT_SCALE_FACTOR='1'
$process=Start-Process -FilePath $App -ArgumentList @('--ui-snapshot',('"'+(Join-Path $OutputDirectory 'scenes')+'"')) -PassThru -RedirectStandardError (Join-Path $OutputDirectory 'stderr.log')
$nativeHandle=$process.Handle
$results=New-Object Collections.Generic.List[string]
try {
    $deadline=(Get-Date).AddSeconds(15)
    do {
        $process.Refresh()
        if($process.HasExited){throw 'Application exited before UI Automation connected.'}
        if($process.MainWindowHandle -ne [IntPtr]::Zero){break}
        Start-Sleep -Milliseconds 100
    } while((Get-Date) -lt $deadline)
    if($process.MainWindowHandle -eq [IntPtr]::Zero){throw 'No application window.'}
    $windowHandle=$process.MainWindowHandle
    [VpsQaProcess]::ShowWindow($windowHandle,9)|Out-Null
    [VpsQaProcess]::SetForegroundWindow($windowHandle)|Out-Null
    Start-Sleep -Milliseconds 500
    $root=[Windows.Automation.AutomationElement]::FromHandle($windowHandle)
    Write-Host ("Native root: "+$root.Current.Name+"; process "+$root.Current.ProcessId)
    if($root.Current.ProcessId -ne $process.Id){throw 'UI Automation attached to the wrong process.'}
    function Find-Button([string]$Label) {
        $condition=New-Object Windows.Automation.AndCondition(
            (New-Object Windows.Automation.PropertyCondition([Windows.Automation.AutomationElement]::NameProperty,$Label)),
            (New-Object Windows.Automation.PropertyCondition([Windows.Automation.AutomationElement]::ControlTypeProperty,[Windows.Automation.ControlType]::Button)))
        $deadline=(Get-Date).AddSeconds(15)
        do {
            $root=[Windows.Automation.AutomationElement]::FromHandle($windowHandle)
            $button=$root.FindFirst([Windows.Automation.TreeScope]::Descendants,$condition)
            if($button){return $button}
            Start-Sleep -Milliseconds 100
        } while((Get-Date) -lt $deadline)
        throw "Native UI Automation cannot find button: $Label"
    }
    function Invoke-Button([string]$Label) {
        $button=Find-Button $Label
        if(!$button.Current.IsEnabled){throw "Button is disabled: $Label"}
        $pattern=$button.GetCurrentPattern([Windows.Automation.InvokePattern]::Pattern)
        $pattern.Invoke()
        Start-Sleep -Milliseconds 150
        $results.Add("Invoked $Label via native Windows UI Automation")
    }
    Invoke-Button 'Activity'
    Find-Button 'Retry pending'|Out-Null
    Invoke-Button 'Presets'
    Find-Button 'Use preset'|Out-Null
    Invoke-Button 'Settings'
    Find-Button 'Save settings'|Out-Null
    Invoke-Button 'Printers'
    Invoke-Button '+ New printer'
    Find-Button 'Save printer'|Out-Null
    if((Find-Button 'Printers').Current.IsEnabled){throw 'Modal leaves background navigation enabled.'}
    Invoke-Button 'Cancel'
    Find-Button '+ New printer'|Out-Null
    $exitCode=Wait-VpsProcess $process $nativeHandle 90000
    if($exitCode -ne 0){throw "Application failed: $exitCode"}
    $results|ConvertTo-Json|Set-Content -LiteralPath (Join-Path $OutputDirectory 'results.json') -Encoding UTF8
    Write-Host 'PASS: native Windows UI Automation exposes and operates navigation and the new-printer dialog.'
} finally {
    if($root -and !$process.HasExited){Save-VpsUiDiagnostics $root $OutputDirectory 'accessibility-window'}
    $env:VPS_QA_HOLD_SECONDS=$null
    $process.Refresh()
    if(!$process.HasExited){Stop-Process -Id $process.Id -Force}
}
