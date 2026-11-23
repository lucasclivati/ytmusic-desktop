# Gera assets\icon.ico (varios tamanhos) e assets\icon.png (256x256).
# O desenho e o mesmo do logo da barra superior do app: circulo vermelho, anel branco e play.
# Uso: powershell -File tools\make-icon.ps1

Add-Type -AssemblyName System.Drawing
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$assets = Join-Path $root 'assets'
New-Item -ItemType Directory -Force $assets | Out-Null

function New-IconPng([int]$size) {
    $bmp = New-Object System.Drawing.Bitmap $size, $size, ([System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
    $g.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
    $g.Clear([System.Drawing.Color]::Transparent)

    $c = $size / 2.0
    $r = $size / 2.0 - ($size * 0.02)            # circulo vermelho
    $red = [System.Drawing.Color]::FromArgb(255, 255, 0, 0)
    $g.FillEllipse((New-Object System.Drawing.SolidBrush $red), $c - $r, $c - $r, 2 * $r, 2 * $r)

    $ringR = $r * 0.607                           # anel branco (proporcoes do logo do app)
    $pen = New-Object System.Drawing.Pen ([System.Drawing.Color]::White), ([single]($r * 0.114 * 1.25))
    $g.DrawEllipse($pen, $c - $ringR, $c - $ringR, 2 * $ringR, 2 * $ringR)

    $pts = @(
        (New-Object System.Drawing.PointF ([single]($c - $r * 0.186)), ([single]($c - $r * 0.30))),
        (New-Object System.Drawing.PointF ([single]($c - $r * 0.186)), ([single]($c + $r * 0.30))),
        (New-Object System.Drawing.PointF ([single]($c + $r * 0.314)), ([single]$c))
    )
    $g.FillPolygon([System.Drawing.Brushes]::White, [System.Drawing.PointF[]]$pts)
    $g.Dispose()

    $ms = New-Object System.IO.MemoryStream
    $bmp.Save($ms, [System.Drawing.Imaging.ImageFormat]::Png)
    $bmp.Dispose()
    return , $ms.ToArray()
}

$sizes = 16, 24, 32, 48, 64, 128, 256
$frames = foreach ($s in $sizes) { , (New-IconPng $s) }

# ICO com quadros PNG (suportado desde o Windows Vista).
$out = New-Object System.IO.MemoryStream
$w = New-Object System.IO.BinaryWriter $out
$w.Write([uint16]0); $w.Write([uint16]1); $w.Write([uint16]$sizes.Count)
$offset = 6 + 16 * $sizes.Count
for ($i = 0; $i -lt $sizes.Count; $i++) {
    $s = $sizes[$i]; $len = $frames[$i].Length
    $dim = if ($s -ge 256) { 0 } else { $s }
    $w.Write([byte]$dim); $w.Write([byte]$dim); $w.Write([byte]0); $w.Write([byte]0)
    $w.Write([uint16]1); $w.Write([uint16]32); $w.Write([uint32]$len); $w.Write([uint32]$offset)
    $offset += $len
}
foreach ($f in $frames) { $w.Write($f) }
$w.Flush()
[System.IO.File]::WriteAllBytes((Join-Path $assets 'icon.ico'), $out.ToArray())
[System.IO.File]::WriteAllBytes((Join-Path $assets 'icon.png'), $frames[$sizes.Count - 1])
"gerado: assets\icon.ico ({0:N1} KB) e assets\icon.png" -f ((Get-Item (Join-Path $assets 'icon.ico')).Length / 1KB)
