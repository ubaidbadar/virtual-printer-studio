param([Parameter(Mandatory=$true)][string]$OutputDirectory,[string]$ExpectedMachine)
$ErrorActionPreference='Stop'
[IO.Directory]::CreateDirectory($OutputDirectory)|Out-Null
$os=Get-CimInstance Win32_OperatingSystem
$cpu=Get-CimInstance Win32_Processor|Select-Object -First 1
$kind=if($os.ProductType -ne 1){'Windows Server'}elseif([int]$os.BuildNumber -ge 22000){'Windows 11'}else{'Windows 10'}
$metadata=@{caption=$os.Caption;version=$os.Version;build=$os.BuildNumber;product_type=$os.ProductType;client_os=$kind;cpu_architecture=$cpu.Architecture;processor_architecture=$env:PROCESSOR_ARCHITECTURE;interactive=[Environment]::UserInteractive;session_id=[Diagnostics.Process]::GetCurrentProcess().SessionId;expected=$ExpectedMachine;source_commit=$env:GITHUB_SHA;utc=(Get-Date).ToUniversalTime().ToString('o')}
$metadata|ConvertTo-Json -Depth 6|Set-Content -LiteralPath (Join-Path $OutputDirectory 'platform.json') -Encoding utf8NoBOM
$metadata|ConvertTo-Json|Write-Host
if($ExpectedMachine){
    if($os.ProductType -ne 1){throw 'Client acceptance cannot pass on Windows Server.'}
    if($ExpectedMachine -like 'windows-10-*' -and $kind -ne 'Windows 10'){throw 'This is not Windows 10.'}
    if($ExpectedMachine -like 'windows-11-*' -and $kind -ne 'Windows 11'){throw 'This is not Windows 11.'}
    if($ExpectedMachine -like '*x64-self-hosted' -and $cpu.Architecture -ne 9){throw 'Native x64 evidence requires x64 hardware/OS.'}
    if($ExpectedMachine -like '*arm-x64-emulated' -and $cpu.Architecture -ne 12){throw 'Expected an ARM64 Windows 11 host.'}
}
