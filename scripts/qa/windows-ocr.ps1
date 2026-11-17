param(
    [Parameter(Mandatory=$true)][string]$Image,
    [Parameter(Mandatory=$true)][string]$TextOutput
)
$ErrorActionPreference='Stop'
try {
    Add-Type -AssemblyName System.Runtime.WindowsRuntime
    [Windows.Storage.StorageFile,Windows.Storage,ContentType=WindowsRuntime]|Out-Null
    [Windows.Graphics.Imaging.BitmapDecoder,Windows.Foundation,ContentType=WindowsRuntime]|Out-Null
    [Windows.Media.Ocr.OcrEngine,Windows.Foundation,ContentType=WindowsRuntime]|Out-Null
    $asTask=[System.WindowsRuntimeSystemExtensions].GetMethods()|Where-Object {
        $_.Name -eq 'AsTask' -and $_.IsGenericMethod -and
        $_.GetGenericArguments().Count -eq 1 -and $_.GetParameters().Count -eq 1
    }|Select-Object -First 1
    function Await-WinRt($operation,[Type]$resultType) {
        $task=$asTask.MakeGenericMethod($resultType).Invoke($null,@($operation))
        $task.GetAwaiter().GetResult()
    }
    $file=Await-WinRt ([Windows.Storage.StorageFile]::GetFileFromPathAsync($Image)) ([Windows.Storage.StorageFile])
    $stream=Await-WinRt ($file.OpenReadAsync()) ([Windows.Storage.Streams.IRandomAccessStreamWithContentType])
    try {
        $decoder=Await-WinRt ([Windows.Graphics.Imaging.BitmapDecoder]::CreateAsync($stream)) ([Windows.Graphics.Imaging.BitmapDecoder])
        $bitmap=Await-WinRt ($decoder.GetSoftwareBitmapAsync()) ([Windows.Graphics.Imaging.SoftwareBitmap])
        try {
            $gray=[Windows.Graphics.Imaging.SoftwareBitmap]::Convert($bitmap,[Windows.Graphics.Imaging.BitmapPixelFormat]::Gray8)
            try {
                $engine=[Windows.Media.Ocr.OcrEngine]::TryCreateFromUserProfileLanguages()
                if (!$engine) { throw 'No local OCR language is available.' }
                $result=Await-WinRt ($engine.RecognizeAsync($gray)) ([Windows.Media.Ocr.OcrResult])
                [IO.File]::WriteAllText($TextOutput,$result.Text,(New-Object Text.UTF8Encoding($false)))
            } finally { $gray.Dispose() }
        } finally { $bitmap.Dispose() }
    } finally { $stream.Dispose() }
} catch {
    # The caller captures this output; never include recognized account data.
    Write-Error 'Local Windows OCR failed.'
    exit 1
}
