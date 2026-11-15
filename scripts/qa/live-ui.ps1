param([Parameter(Mandatory=$true)][string]$App,[Parameter(Mandatory=$true)][string]$OutputDirectory,[switch]$TestBackground)
$ErrorActionPreference='Stop'
if(($env:GITHUB_ACTIONS -ne 'true' -or $env:RUNNER_ENVIRONMENT -ne 'github-hosted') -and $env:VPS_DISPOSABLE_TEST_VM -ne '1'){throw 'This acceptance script requires a disposable GitHub Actions Windows desktop.'}
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
. (Join-Path $PSScriptRoot 'ui-diagnostics.ps1')
. (Join-Path $PSScriptRoot 'process-exit.ps1')
$data=Join-Path ([Environment]::GetFolderPath('ApplicationData')) 'VirtualPrinterStudio\VirtualPrinterStudio\data'
if(Test-Path -LiteralPath $data){throw 'Live GUI acceptance requires a fresh Windows user profile; existing application data will not be changed.'}
[IO.Directory]::CreateDirectory($OutputDirectory)|Out-Null
$OutputDirectory=(Resolve-Path -LiteralPath $OutputDirectory).Path
$output=Join-Path $OutputDirectory 'PDFs'
[IO.Directory]::CreateDirectory($output)|Out-Null
$stateFile=Join-Path $data 'state.json'
$name='VPS GUI '+[guid]::NewGuid().ToString().Substring(0,8)
$env:VPS_QA_HOLD_SECONDS=$null
$env:SLINT_SCALE_FACTOR='1'
$App=(Resolve-Path -LiteralPath $App).Path
$process=Start-Process -FilePath $App -PassThru -RedirectStandardError (Join-Path $OutputDirectory 'stderr.log')
$created=$null
$second=$null
$expectedPdfCount=1
$results=New-Object Collections.Generic.List[string]
function Read-State {if(Test-Path -LiteralPath $stateFile){Get-Content -LiteralPath $stateFile -Raw|ConvertFrom-Json}}
function Wait-State([scriptblock]$Predicate) {
    $deadline=(Get-Date).AddSeconds(90)
    do {
        $process.Refresh()
        if($process.HasExited){throw 'Application exited during live GUI acceptance.'}
        $state=Read-State
        if($state -and (& $Predicate $state)){return $state}
        Start-Sleep -Milliseconds 200
    }while((Get-Date) -lt $deadline)
    throw 'Timed out waiting for persisted GUI action.'
}
function Find-Element([string]$Label,$ControlType=$null) {
    $condition=New-Object Windows.Automation.PropertyCondition([Windows.Automation.AutomationElement]::NameProperty,$Label)
    if($ControlType){$condition=New-Object Windows.Automation.AndCondition($condition,(New-Object Windows.Automation.PropertyCondition([Windows.Automation.AutomationElement]::ControlTypeProperty,$ControlType)))}
    $deadline=(Get-Date).AddSeconds(8)
    do {
        $element=$root.FindFirst([Windows.Automation.TreeScope]::Descendants,$condition)
        if($element -and $element.Current.IsEnabled){return $element}
        Start-Sleep -Milliseconds 100
    }while((Get-Date) -lt $deadline)
    throw "Missing enabled native control: $Label"
}
function Invoke-Control([string]$Label) {
    $element=Find-Element $Label ([Windows.Automation.ControlType]::Button)
    $element.GetCurrentPattern([Windows.Automation.InvokePattern]::Pattern).Invoke()
    Start-Sleep -Milliseconds 200
    $results.Add("Invoked $Label")
}
function Set-Field([string]$Label,[string]$Value) {
    $element=Find-Element $Label ([Windows.Automation.ControlType]::Edit)
    $element.GetCurrentPattern([Windows.Automation.ValuePattern]::Pattern).SetValue($Value)
    $results.Add("Entered $Label")
}
try {
    $deadline=(Get-Date).AddSeconds(15)
    do {$process.Refresh();if($process.MainWindowHandle -ne [IntPtr]::Zero){break};Start-Sleep -Milliseconds 100}while((Get-Date) -lt $deadline)
    if($process.MainWindowHandle -eq [IntPtr]::Zero){throw 'No live application window.'}
    [VpsQaProcess]::ShowWindow($process.MainWindowHandle,9)|Out-Null
    [VpsQaProcess]::SetForegroundWindow($process.MainWindowHandle)|Out-Null
    $root=[Windows.Automation.AutomationElement]::FromHandle($process.MainWindowHandle)
    Invoke-Control 'Get started'
    Invoke-Control '+ New printer'
    Set-Field 'Printer name' $name
    Set-Field 'Output folder' $output
    Invoke-Control 'Save printer'
    $state=Wait-State {param($s) @($s.printers|Where-Object {$_.name -eq $name -and $_.installed}).Count -eq 1}
    $created=$state.printers|Where-Object name -eq $name
    if(!(Get-Printer -Name $name -ErrorAction SilentlyContinue)){throw 'GUI did not create a native Windows printer.'}
    $created|ConvertTo-Json -Depth 8|Set-Content (Join-Path $OutputDirectory 'created-printer.json') -Encoding UTF8
    Invoke-Control 'Test PDF'
    $state=Wait-State {param($s) @($s.printers|Where-Object {$_.name -eq $name -and $_.count -eq 1}).Count -eq 1}
    if(@(Get-ChildItem -LiteralPath $output -Filter '*.pdf').Count -ne 1){throw 'GUI test-page action did not save its PDF.'}
    $results.Add('GUI test-page action saved a real Windows driver PDF')
    Save-VpsUiDiagnostics $root $OutputDirectory 'after-print'

    if($TestBackground){
        Add-Type -TypeDefinition @'
using System;using System.Runtime.InteropServices;
public static class VpsQaWindow {
 [DllImport("user32.dll",SetLastError=true)] public static extern bool PostMessage(IntPtr window,uint message,IntPtr wparam,IntPtr lparam);
 [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr window);
}
'@
        $process.Refresh();$windowHandle=$process.MainWindowHandle
        if(![VpsQaWindow]::PostMessage($windowHandle,0x0010,[IntPtr]::Zero,[IntPtr]::Zero)){throw 'Cannot request native window close.'}
        $deadline=(Get-Date).AddSeconds(5)
        do {Start-Sleep -Milliseconds 100}while([VpsQaWindow]::IsWindowVisible($windowHandle) -and (Get-Date) -lt $deadline)
        $process.Refresh()
        if($process.HasExited -or [VpsQaWindow]::IsWindowVisible($windowHandle)){throw 'Close-to-tray did not retain a hidden running application.'}
        $producer=@'
Add-Type -AssemblyName System.Drawing
$name=[Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('__PRINTER__'))
$doc=New-Object Drawing.Printing.PrintDocument
$doc.PrinterSettings.PrinterName=$name
$doc.DocumentName='Tray continuation'
$doc.PrintController=New-Object Drawing.Printing.StandardPrintController
$doc.add_PrintPage({param($sender,$e)
 $font=New-Object Drawing.Font('Arial',16)
 try{$e.Graphics.DrawString('Your virtual printer is working.',$font,[Drawing.Brushes]::Black,72,72)}finally{$font.Dispose()}
 $e.HasMorePages=$false
})
try{$doc.Print()}finally{$doc.Dispose()}
'@
        $producerPath=Join-Path $OutputDirectory 'external-print.ps1'
        $producer.Replace('__PRINTER__',[Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($name)))|Set-Content $producerPath -Encoding UTF8
        & powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File $producerPath
        if($LASTEXITCODE -ne 0){throw 'External printing while hidden failed.'}
        Wait-State {param($s) $s.printers[0].count -eq 2}|Out-Null
        $expectedPdfCount=2
        $second=Start-Process -FilePath $App -PassThru
        $deadline=(Get-Date).AddSeconds(15)
        do {Start-Sleep -Milliseconds 100;$second.Refresh()}while((![VpsQaWindow]::IsWindowVisible($windowHandle) -or !$second.HasExited) -and (Get-Date) -lt $deadline)
        if(![VpsQaWindow]::IsWindowVisible($windowHandle) -or !$second.HasExited){throw 'Second launch did not reactivate the existing instance.'}
        $root=[Windows.Automation.AutomationElement]::FromHandle($windowHandle)
        $results.Add('Native window close kept the receiver alive; an external application printed while hidden; second launch reactivated the original instance')
    }
    $toggle=Find-Element 'Enable printer'
    $toggle.GetCurrentPattern([Windows.Automation.TogglePattern]::Pattern).Toggle()
    Wait-State {param($s) $s.printers[0].enabled -eq $false}|Out-Null
    $toggle=Find-Element 'Enable printer'
    $toggle.GetCurrentPattern([Windows.Automation.TogglePattern]::Pattern).Toggle()
    Wait-State {param($s) $s.printers[0].enabled -eq $true}|Out-Null
    $results.Add('GUI switch paused and resumed the native printer')
    Invoke-Control 'Details'
    Invoke-Control 'Delete printer'
    Invoke-Control 'Delete printer'
    Wait-State {param($s) @($s.printers).Count -eq 0}|Out-Null
    if(Get-Printer -Name $name -ErrorAction SilentlyContinue){throw 'GUI deletion left the Windows printer installed.'}
    if(@(Get-ChildItem -LiteralPath $output -Filter '*.pdf').Count -ne $expectedPdfCount){throw 'GUI deletion removed a saved PDF.'}
    $results.Add('GUI deletion removed the native printer and retained the PDF')
    $results|ConvertTo-Json|Set-Content (Join-Path $OutputDirectory 'results.json') -Encoding UTF8
    Write-Host 'PASS: live GUI onboarding, profile creation, native installation, test printing, pause/resume and deletion.'
} finally {
    if($second){$second.Refresh();if(!$second.HasExited){Stop-Process -Id $second.Id -Force}}
    if($root){Save-VpsUiDiagnostics $root $OutputDirectory 'final-window'}
    $process.Refresh()
    if(!$process.HasExited){Stop-Process -Id $process.Id -Force}
    if(Test-Path -LiteralPath $data){Copy-Item -LiteralPath $data -Destination (Join-Path $OutputDirectory 'application-data') -Recurse -Force}
    # Only the test profile in this previously empty application directory may be removed.
    if(!$created){$state=Read-State;if($state){$created=$state.printers|Where-Object name -eq $name|Select-Object -First 1}}
    $native=Get-Printer -Name $name -ErrorAction SilentlyContinue
    if(!$created -and $native -and $native.PortName -match '^VPS_([A-Za-z0-9-]{1,80})$'){$created=@{id=$Matches[1]}}
    if($created -and $native){
        Get-PrintJob -PrinterName $name|Remove-PrintJob -ErrorAction SilentlyContinue
        $sid=[Security.Principal.WindowsIdentity]::GetCurrent().User.Value
        $queue=[BitConverter]::ToString([Security.Cryptography.SHA256]::Create().ComputeHash([Text.Encoding]::UTF8.GetBytes($created.id))).Replace('-','').Substring(0,24).ToLowerInvariant()
        $request=@{action='remove';id=$created.id;name=$name;sid=$sid;queue=$queue;port=515;enabled=$true;result=(Join-Path $OutputDirectory 'cleanup.json')}|ConvertTo-Json -Compress
        $template=Get-Content -LiteralPath (Join-Path $PSScriptRoot '..\..\src\platform\native.ps1') -Raw
        $cleanup=Join-Path $OutputDirectory 'cleanup.ps1'
        $template.Replace('__REQUEST__',[Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($request)))|Set-Content $cleanup -Encoding UTF8
        & powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File $cleanup
    }
    if(Test-Path -LiteralPath $data){Remove-Item -LiteralPath $data -Recurse -Force}
}
