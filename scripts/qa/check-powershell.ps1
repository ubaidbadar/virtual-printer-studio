$ErrorActionPreference='Stop'
$root=Join-Path $PSScriptRoot '..\..'
$files=@(Get-ChildItem -LiteralPath (Join-Path $root 'scripts') -Recurse -Filter '*.ps1')+@(Get-ChildItem -LiteralPath (Join-Path $root 'src\platform') -Filter '*.ps1')
foreach($file in $files){
    $tokens=$null;$errors=$null
    [Management.Automation.Language.Parser]::ParseFile($file.FullName,[ref]$tokens,[ref]$errors)|Out-Null
    if($errors.Count){$errors|ForEach-Object {Write-Host ($file.FullName+': '+$_.Message)};throw 'PowerShell syntax check failed.'}
}
Write-Host "PASS: $($files.Count) PowerShell scripts parsed by Windows PowerShell."
