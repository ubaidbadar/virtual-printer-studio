param(
    [string]$Repository = 'ubaidbadar/virtual-printer-studio',
    [string]$Directory = (Join-Path $env:LOCALAPPDATA 'VirtualPrinterStudio-QA'),
    [string]$RegistrationToken
)
$ErrorActionPreference='Stop'
[Net.ServicePointManager]::SecurityProtocol=[Net.SecurityProtocolType]::Tls12
$identity=[Security.Principal.WindowsIdentity]::GetCurrent()
$principal=New-Object Security.Principal.WindowsPrincipal($identity)
if(!$principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)){throw 'Open an Administrator Windows PowerShell terminal, then run this script.'}
$os=Get-CimInstance Win32_OperatingSystem
if($os.ProductType -ne 1 -or [int]$os.BuildNumber -lt 10240){throw 'This runner setup requires actual Windows 10 or Windows 11 client, not Windows Server.'}
$cpu=Get-CimInstance Win32_Processor|Select-Object -First 1
if($cpu.Architecture -ne 9){throw 'This setup installs the x64 runner and requires x64 Windows.'}
if($Repository -notmatch '^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$'){throw 'Use an owner/repository name.'}
$label=if([int]$os.BuildNumber -ge 22000){'vps-windows-11-x64'}else{'vps-windows-10-x64'}
[IO.Directory]::CreateDirectory($Directory)|Out-Null
$Directory=(Resolve-Path -LiteralPath $Directory).Path
function Download-VerifiedAsset($asset,[string]$destination){
    if(!$asset.digest -or $asset.digest -notmatch '^sha256:([a-fA-F0-9]{64})$'){throw "GitHub did not supply a SHA-256 digest for $($asset.name). Do not run an unverified runner download."}
    $expected=$Matches[1]
    Invoke-WebRequest -UseBasicParsing -Uri $asset.browser_download_url -OutFile $destination
    if((Get-FileHash -LiteralPath $destination -Algorithm SHA256).Hash -ne $expected){throw "SHA-256 mismatch for $($asset.name)"}
}
$headers=@{'User-Agent'='VirtualPrinterStudio-QA';'Accept'='application/vnd.github+json'}
$release=Invoke-RestMethod -Headers $headers -Uri 'https://api.github.com/repos/actions/runner/releases/latest'
$asset=$release.assets|Where-Object name -Match '^actions-runner-win-x64-.*\.zip$'|Select-Object -First 1
if(!$asset){throw 'The official Windows x64 Actions runner asset was not found.'}
$runner=Join-Path $Directory ('runner-'+[guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($runner)|Out-Null
$archive=Join-Path $Directory 'actions-runner.zip'
Download-VerifiedAsset $asset $archive
Expand-Archive -LiteralPath $archive -DestinationPath $runner
# The acceptance scripts use PowerShell 7. Install a verified portable copy only
# in the QA folder if this VM does not already have it; do not change OS policy.
if(!(Get-Command pwsh.exe -ErrorAction SilentlyContinue)){
    $ps=Invoke-RestMethod -Headers $headers -Uri 'https://api.github.com/repos/PowerShell/PowerShell/releases/latest'
    $psAsset=$ps.assets|Where-Object name -Match '^PowerShell-[0-9.]+-win-x64\.zip$'|Select-Object -First 1
    if(!$psAsset){throw 'The official portable PowerShell x64 asset was not found.'}
    $psArchive=Join-Path $Directory 'powershell.zip';Download-VerifiedAsset $psAsset $psArchive
    $psDirectory=Join-Path $Directory 'pwsh';Expand-Archive -LiteralPath $psArchive -DestinationPath $psDirectory -Force
    $env:PATH=$psDirectory+';'+$env:PATH
}
if(!$RegistrationToken){
    Write-Host "Get a one-hour registration token at https://github.com/$Repository/settings/actions/runners/new"
    $secure=Read-Host 'Runner registration token' -AsSecureString
    $pointer=[Runtime.InteropServices.Marshal]::SecureStringToBSTR($secure)
    try{$RegistrationToken=[Runtime.InteropServices.Marshal]::PtrToStringBSTR($pointer)}finally{[Runtime.InteropServices.Marshal]::ZeroFreeBSTR($pointer)}
}
Push-Location $runner
try{
    & .\config.cmd --unattended --ephemeral --url "https://github.com/$Repository" --token $RegistrationToken --name ($env:COMPUTERNAME+'-VPS-'+[guid]::NewGuid().ToString('N').Substring(0,6)) --labels ($label+',vps-interactive') --work _work
    if($LASTEXITCODE -ne 0){throw 'Runner registration failed.'}
    $RegistrationToken=$null
    Write-Host "Ready: $($os.Caption), build $($os.BuildNumber), label $label."
    Write-Host 'Keep this elevated, interactive terminal open for one acceptance job. No Windows service is installed.'
    & .\run.cmd
    if($LASTEXITCODE -ne 0){throw 'The runner exited with an error.'}
}finally{Pop-Location}
