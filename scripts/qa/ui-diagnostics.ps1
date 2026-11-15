function Save-VpsUiDiagnostics($Root,[string]$Directory,[string]$Name='window') {
    try {
        $elements=$Root.FindAll([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.Condition]::TrueCondition)
        $rows=@(foreach($element in $elements){@{name=$element.Current.Name;type=$element.Current.ControlType.ProgrammaticName;enabled=$element.Current.IsEnabled;offscreen=$element.Current.IsOffscreen}})
        $rows|ConvertTo-Json -Depth 4|Set-Content -LiteralPath (Join-Path $Directory "$Name-controls.json") -Encoding UTF8
        Add-Type -AssemblyName System.Drawing
        Add-Type -AssemblyName System.Windows.Forms
        $bounds=$Root.Current.BoundingRectangle
        $screen=[Windows.Forms.SystemInformation]::VirtualScreen
        $window=New-Object Drawing.Rectangle([int]$bounds.X,[int]$bounds.Y,[int]$bounds.Width,[int]$bounds.Height)
        $visible=[Drawing.Rectangle]::Intersect($window,$screen)
        if($visible.Width -gt 0 -and $visible.Height -gt 0){
            $bitmap=New-Object Drawing.Bitmap($visible.Width,$visible.Height)
            $graphics=[Drawing.Graphics]::FromImage($bitmap)
            try {
                $graphics.CopyFromScreen($visible.Location,[Drawing.Point]::Empty,$visible.Size)
                $bitmap.Save((Join-Path $Directory "$Name.png"),[Drawing.Imaging.ImageFormat]::Png)
                @{window=@{x=$bounds.X;y=$bounds.Y;width=$bounds.Width;height=$bounds.Height};visible=@{x=$visible.X;y=$visible.Y;width=$visible.Width;height=$visible.Height};note='Actual visible native window, including OS frame; no resizing.'}|ConvertTo-Json -Depth 4|Set-Content (Join-Path $Directory "$Name-bounds.json") -Encoding UTF8
            } finally {$graphics.Dispose();$bitmap.Dispose()}
        }
    } catch {Write-Host ('Optional UI diagnostic capture: '+$_.Exception.Message)}
}
