param([Parameter(Mandatory=$true)][string]$App,[Parameter(Mandatory=$true)][string]$OutputDirectory)
$ErrorActionPreference='Stop'
if($env:GITHUB_ACTIONS -ne 'true' -or $env:RUNNER_ENVIRONMENT -ne 'github-hosted'){throw 'This acceptance script requires a disposable GitHub Actions Windows desktop.'}
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
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
    if(@(Get-ChildItem -LiteralPath $output -Filter '*.pdf').Count -ne 1){throw 'GUI deletion removed a saved PDF.'}
    $results.Add('GUI deletion removed the native printer and retained the PDF')
    $results|ConvertTo-Json|Set-Content (Join-Path $OutputDirectory 'results.json') -Encoding UTF8
    Write-Host 'PASS: live GUI onboarding, profile creation, native installation, test printing, pause/resume and deletion.'
} finally {
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
