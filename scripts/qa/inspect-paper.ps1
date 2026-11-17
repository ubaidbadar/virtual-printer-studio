param(
    [Parameter(Mandatory=$true)][string]$PrinterName,
    [Parameter(Mandatory=$true)][string]$OutputPath
)
$ErrorActionPreference='Stop'
Add-Type -AssemblyName ReachFramework
Add-Type -AssemblyName System.Printing
$server=New-Object System.Printing.LocalPrintServer
$printer=$server.GetPrintQueue($PrinterName)
try {
    $papers=@($printer.GetPrintCapabilities().PageMediaSizeCapability|ForEach-Object {
        @{name=[string]$_.PageMediaSizeName;width_mm=[Math]::Round($_.Width*25.4/96,2);height_mm=[Math]::Round($_.Height*25.4/96,2)}
    })
    $request=New-Object System.Printing.PrintTicket
    $request.PageMediaSize=New-Object System.Printing.PageMediaSize((80*96/25.4),(150*96/25.4))
    $result=$printer.MergeAndValidatePrintTicket($printer.DefaultPrintTicket,$request)
    $validated=$result.ValidatedPrintTicket.PageMediaSize
    @{supported_papers=$papers;custom_request_mm=@(80,150);validated_width_mm=[Math]::Round($validated.Width*25.4/96,2);validated_height_mm=[Math]::Round($validated.Height*25.4/96,2);conflict_status=[string]$result.ConflictStatus}|ConvertTo-Json -Depth 5|Set-Content -LiteralPath $OutputPath -Encoding UTF8
} finally { $printer.Dispose();$server.Dispose() }
