# Renders the site's share picture and icons from the repository's own images:
#   site/media/og.png (1200x630, from scripts/site-og.html), icon.png (64 px), apple-touch-icon.png (180 px)
#   powershell -ExecutionPolicy Bypass -File scripts\site-og.ps1
# Needs Microsoft Edge (or set $env:OG_BROWSER to a Chromium browser).
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
$root = Resolve-Path (Join-Path $PSScriptRoot '..')
$media = Join-Path $root 'site\media'
New-Item -ItemType Directory -Force $media | Out-Null

$browser = $env:OG_BROWSER
if (-not $browser) {
  $browser = @("${env:ProgramFiles(x86)}\Microsoft\Edge\Application\msedge.exe", "$env:ProgramFiles\Microsoft\Edge\Application\msedge.exe") | Where-Object { Test-Path $_ } | Select-Object -First 1
}
if (-not $browser) { throw 'Microsoft Edge not found; set OG_BROWSER' }

$og = Join-Path $media 'og.png'
$page = 'file:///' + ((Join-Path $root 'scripts\site-og.html') -replace '\\', '/')
$profile = Join-Path ([IO.Path]::GetTempPath()) "ownvoice-og-$(Get-Random)"
& $browser --headless=new --disable-gpu --hide-scrollbars --force-device-scale-factor=1 --user-data-dir="$profile" --window-size=1200,630 --screenshot="$og" $page | Out-Null
Start-Sleep -Seconds 2
Remove-Item -Recurse -Force $profile -ErrorAction SilentlyContinue
$img = [System.Drawing.Image]::FromFile($og); "og.png: $($img.Width)x$($img.Height), $((Get-Item $og).Length) bytes"; $img.Dispose()

function Resize($src, $size, $dest) {
  $in = [System.Drawing.Image]::FromFile($src)
  $out = New-Object System.Drawing.Bitmap $size, $size
  $g = [System.Drawing.Graphics]::FromImage($out)
  $g.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
  $g.DrawImage($in, 0, 0, $size, $size); $g.Dispose(); $in.Dispose()
  $out.Save($dest, [System.Drawing.Imaging.ImageFormat]::Png); $out.Dispose()
  "$(Split-Path $dest -Leaf): ${size}x${size}, $((Get-Item $dest).Length) bytes"
}
$icon = Join-Path $root 'src-tauri\icons\icon.png'
Resize $icon 64 (Join-Path $media 'icon.png')
Resize $icon 180 (Join-Path $media 'apple-touch-icon.png')
