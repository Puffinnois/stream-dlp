# Generate the app's original blue download mark at Windows icon sizes.
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
$assets = Join-Path (Split-Path $PSScriptRoot) 'assets'
New-Item -ItemType Directory -Path $assets -Force | Out-Null
$images = @()
foreach ($size in @(16, 24, 32, 48, 64, 128, 256)) {
    $bitmap = [Drawing.Bitmap]::new(256, 256)
    $graphics = [Drawing.Graphics]::FromImage($bitmap)
    $graphics.SmoothingMode = [Drawing.Drawing2D.SmoothingMode]::AntiAlias
    $graphics.ScaleTransform(4, 4)
    $shape = [Drawing.Drawing2D.GraphicsPath]::new()
    foreach ($corner in @(@(0,0,180), @(39,0,270), @(39,39,0), @(0,39,90))) {
        $shape.AddArc($corner[0], $corner[1], 24, 24, $corner[2], 90)
    }
    $shape.CloseFigure()
    $blue = [Drawing.SolidBrush]::new([Drawing.Color]::FromArgb(34,105,232))
    $graphics.FillPath($blue, $shape)
    $points = [Drawing.PointF[]]@([Drawing.PointF]::new(29,14), [Drawing.PointF]::new(35,14), [Drawing.PointF]::new(35,30), [Drawing.PointF]::new(46,30), [Drawing.PointF]::new(32,46), [Drawing.PointF]::new(18,30), [Drawing.PointF]::new(29,30))
    $graphics.FillPolygon([Drawing.Brushes]::White, $points)
    $graphics.FillRectangle([Drawing.Brushes]::White, 17,50,31,4)
    $graphics.FillRectangle([Drawing.Brushes]::White, 17,44,4,10)
    $graphics.FillRectangle([Drawing.Brushes]::White, 44,44,4,10)
    $resized = [Drawing.Bitmap]::new($size, $size)
    $scaled = [Drawing.Graphics]::FromImage($resized)
    $scaled.InterpolationMode = [Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
    $scaled.DrawImage($bitmap, 0,0,$size,$size)
    $stream = [IO.MemoryStream]::new()
    $resized.Save($stream, [Drawing.Imaging.ImageFormat]::Png)
    $images += [pscustomobject]@{ Size = $size; Bytes = $stream.ToArray() }
    if ($size -eq 256) { $resized.Save((Join-Path $assets 'app.png'), [Drawing.Imaging.ImageFormat]::Png) }
    $stream.Dispose(); $scaled.Dispose(); $resized.Dispose()
    $blue.Dispose(); $shape.Dispose(); $graphics.Dispose(); $bitmap.Dispose()
}
$output = [IO.File]::Create((Join-Path $assets 'app.ico'))
$writer = [IO.BinaryWriter]::new($output)
try {
    $writer.Write([uint16]0); $writer.Write([uint16]1); $writer.Write([uint16]$images.Count)
    $offset = 6 + 16 * $images.Count
    foreach ($entry in $images) {
        $dimension = if ($entry.Size -eq 256) { 0 } else { $entry.Size }
        $writer.Write([byte]$dimension); $writer.Write([byte]$dimension)
        $writer.Write([uint16]0); $writer.Write([uint16]1); $writer.Write([uint16]32)
        $writer.Write([uint32]$entry.Bytes.Length); $writer.Write([uint32]$offset)
        $offset += $entry.Bytes.Length
    }
    foreach ($entry in $images) { $writer.Write([byte[]]$entry.Bytes) }
} finally { $writer.Dispose(); $output.Dispose() }
