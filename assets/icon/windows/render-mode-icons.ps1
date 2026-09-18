<#
.SYNOPSIS
    把「注」（注音模式）图标的字形轮廓化，生成 svg 源文件与四档 DPI 的 8 位 alpha 蒙版。

.DESCRIPTION
    mode-zh.svg / mode-en.svg / mode-caps.svg 是设计稿里手画的轮廓，直接用 render-mode-icons.sh 栅格化即可。
    「注」没有设计稿，这里用系统字体（默认 Noto Sans SC Black，与设计稿笔画重量最接近）取轮廓：

      1. System.Drawing 的 GraphicsPath.AddString 拿字形轮廓，归一到 16×16 viewBox：
         ink 高度 = 13.5 单位（设计稿「中」14、「英」13），ink 中心落在 (8,8)；
      2. 轮廓写成 assets/icon/windows/mode-zhuyin.svg（纯 path，不依赖字体，可交给 rsvg-convert）；
      3. 同一份轮廓用 GDI+ 抗锯齿填充，取 alpha（按字号² 字节，与 render-mode-icons.sh 的产物同格式），
         写到 apps/windows/tsf/resources/mode/zhuyin-{16,20,24,32}.alpha。

    改了字或字号（-Char / -Ink / -Family）重跑本脚本，产物随仓库提交。注意本文件在 Windows PowerShell 5.1 下
    必须带 UTF-8 BOM（否则中文注释被按 GBK 解码，注释尾字节会吃掉换行）。

.EXAMPLE
    powershell -NoProfile -ExecutionPolicy Bypass -File assets\icon\windows\render-mode-icons.ps1
#>
param(
    [string]$Family = 'Noto Sans SC Black',
    [int]$Char = 0x6CE8,
    [string]$Name = 'zhuyin',
    [double]$Ink = 13.5,
    [string]$Tmp
)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing

$Root = (Resolve-Path (Join-Path $PSScriptRoot '..\..\..')).Path
$SvgDir = Join-Path $Root 'assets\icon\windows'
$MaskDir = Join-Path $Root 'apps\windows\tsf\resources\mode'
if (-not $Tmp) { $Tmp = Join-Path ([System.IO.Path]::GetTempPath()) 'qingjian-mode-icons' }
New-Item -ItemType Directory -Force -Path $Tmp | Out-Null

$SIZES = 16, 20, 24, 32
$VIEW = 16.0

function Get-Bbox($pts) {
    $minx = [single]::MaxValue; $miny = [single]::MaxValue; $maxx = [single]::MinValue; $maxy = [single]::MinValue
    foreach ($pt in $pts) {
        if ($pt.X -lt $minx) { $minx = $pt.X }
        if ($pt.Y -lt $miny) { $miny = $pt.Y }
        if ($pt.X -gt $maxx) { $maxx = $pt.X }
        if ($pt.Y -gt $maxy) { $maxy = $pt.Y }
    }
    return @{ X = $minx; Y = $miny; W = $maxx - $minx; H = $maxy - $miny }
}

# 取字形轮廓，缩放到 ink 高度 = $InkHeight、ink 中心 = (8,8)
function Get-NormalizedPath([string]$family, [int]$code, [single]$inkHeight) {
    $fam = New-Object System.Drawing.FontFamily($family)
    $raw = New-Object System.Drawing.Drawing2D.GraphicsPath
    $fmt = New-Object System.Drawing.StringFormat
    $raw.AddString([string][char]$code, $fam, 0, 1000, (New-Object System.Drawing.PointF(0, 0)), $fmt)
    $data = $raw.PathData
    $pts = $data.Points
    $box = Get-Bbox $pts
    $scale = $inkHeight / $box.H
    $cx = $box.X + $box.W / 2.0
    $cy = $box.Y + $box.H / 2.0
    $out = New-Object 'System.Drawing.PointF[]' $pts.Length
    for ($i = 0; $i -lt $pts.Length; $i++) {
        $out[$i] = New-Object System.Drawing.PointF((($pts[$i].X - $cx) * $scale + ($VIEW / 2.0)), (($pts[$i].Y - $cy) * $scale + ($VIEW / 2.0)))
    }
    $raw.Dispose()
    return @{ Pts = $out; Types = $data.Types }
}

function Format-Num([single]$v) {
    return $v.ToString('0.###', [System.Globalization.CultureInfo]::InvariantCulture)
}

function ConvertTo-SvgPath($norm) {
    $pts = $norm.Pts; $types = $norm.Types
    $sb = New-Object System.Text.StringBuilder
    $i = 0
    $open = $false
    while ($i -lt $types.Length) {
        $t = $types[$i]
        $kind = $t -band 0x07
        $close = ($t -band 0x80) -ne 0
        if ($kind -eq 0 -and $open) {
            [void]$sb.Append('Z')
            $open = $false
        }
        switch ($kind) {
            0 { [void]$sb.Append("M$(Format-Num $pts[$i].X),$(Format-Num $pts[$i].Y)"); $i++; $open = $true }
            1 { [void]$sb.Append("L$(Format-Num $pts[$i].X),$(Format-Num $pts[$i].Y)"); $i++ }
            3 {
                [void]$sb.Append("C$(Format-Num $pts[$i].X),$(Format-Num $pts[$i].Y) $(Format-Num $pts[$i + 1].X),$(Format-Num $pts[$i + 1].Y) $(Format-Num $pts[$i + 2].X),$(Format-Num $pts[$i + 2].Y)")
                $i += 3
            }
            default { throw "未知的轮廓点类型 $t" }
        }
        if ($close) { [void]$sb.Append('Z'); $open = $false }
    }
    if ($open) { [void]$sb.Append('Z') }
    return $sb.ToString()
}

function Write-Svg([string]$svgPath, [string]$d, [string]$family) {
    $lines = @(
        '<?xml version="1.0" encoding="utf-8"?>',
        "<!-- 由 assets/icon/windows/render-mode-icons.ps1 从 $family 轮廓化生成，改字改脚本重跑，勿手改 -->",
        '<svg version="1.1" xmlns="http://www.w3.org/2000/svg" x="0px" y="0px" viewBox="0 0 16 16" xml:space="preserve">',
        '<style type="text/css">',
        '	.st0{fill:#070707;}',
        '</style>',
        "<path class=""st0"" d=""$d""/>",
        '</svg>',
        ''
    )
    [System.IO.File]::WriteAllLines($svgPath, $lines, (New-Object System.Text.UTF8Encoding($false)))
}

function Write-Mask($norm, [int]$size, [string]$outPath) {
    $pts = New-Object 'System.Drawing.PointF[]' $norm.Pts.Length
    $k = $size / $VIEW
    for ($i = 0; $i -lt $norm.Pts.Length; $i++) {
        $pts[$i] = New-Object System.Drawing.PointF(($norm.Pts[$i].X * $k), ($norm.Pts[$i].Y * $k))
    }
    $gp = New-Object System.Drawing.Drawing2D.GraphicsPath($pts, $norm.Types)
    # SVG 默认 fill-rule="nonzero"，GDI+ 默认 Alternate；字形轮廓不重叠时两者等价，这里显式对齐语义
    $gp.FillMode = [System.Drawing.Drawing2D.FillMode]::Winding
    $bmp = New-Object System.Drawing.Bitmap($size, $size, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.Clear([System.Drawing.Color]::Transparent)
    $g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
    $g.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
    $g.FillPath([System.Drawing.Brushes]::Black, $gp)
    $g.Dispose()
    $bytes = New-Object 'byte[]' ($size * $size)
    for ($y = 0; $y -lt $size; $y++) {
        for ($x = 0; $x -lt $size; $x++) {
            $bytes[$y * $size + $x] = $bmp.GetPixel($x, $y).A
        }
    }
    [System.IO.File]::WriteAllBytes($outPath, $bytes)
    $prev = New-Object System.Drawing.Bitmap -ArgumentList ($size * 8), ($size * 8)
    $pg = [System.Drawing.Graphics]::FromImage($prev)
    $pg.Clear([System.Drawing.Color]::White)
    $pg.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::NearestNeighbor
    $pg.DrawImage($bmp, 0, 0, $size * 8, $size * 8)
    $pg.Dispose()
    $prev.Save((Join-Path $Tmp "$Name-$size.png"), [System.Drawing.Imaging.ImageFormat]::Png)
    $prev.Dispose(); $bmp.Dispose(); $gp.Dispose()
}

$norm = Get-NormalizedPath $Family $Char $Ink
Write-Svg (Join-Path $SvgDir "mode-$Name.svg") (ConvertTo-SvgPath $norm) $Family
foreach ($s in $SIZES) {
    Write-Mask $norm $s (Join-Path $MaskDir "$Name-$s.alpha")
}
Write-Host "已写 assets/icon/windows/mode-$Name.svg 与 apps/windows/tsf/resources/mode/$Name-{16,20,24,32}.alpha（字体 $Family，ink $Ink/16）"
Write-Host "预览图（8 倍）在 $Tmp"
