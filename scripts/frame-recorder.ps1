# Helper for record-demo.ps1: every $IntervalMs, asks each window to draw itself (PrintWindow) and
# writes one composed frame (captions over reply on the left, script on the right) until $StopFile
# appears. Only the app's own windows are ever drawn: nothing else on the screen can get in.
param([long]$Captions, [long]$Reply, [long]$Script, [string]$OutDir, [string]$StopFile, [int]$IntervalMs = 200)
Add-Type -AssemblyName System.Drawing
Add-Type @"
using System; using System.Runtime.InteropServices;
public class W8 { [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
[DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
[DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr hdc, uint flags);
[DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(IntPtr ctx); }
"@
[void][W8]::SetProcessDpiAwarenessContext([IntPtr](-4))   # per-monitor: true window sizes on any screen
New-Item -ItemType Directory -Force $OutDir | Out-Null

function Capture([long]$h) {
  $r = New-Object W8+RECT; [void][W8]::GetWindowRect([IntPtr]$h, [ref]$r)
  $bmp = New-Object System.Drawing.Bitmap ($r.R - $r.L), ($r.B - $r.T)
  $g = [System.Drawing.Graphics]::FromImage($bmp); $hdc = $g.GetHdc()
  [void][W8]::PrintWindow([IntPtr]$h, $hdc, 2)
  $g.ReleaseHdc($hdc); $g.Dispose()
  $bmp
}

$i = 0
while (-not (Test-Path $StopFile)) {
  $t0 = Get-Date
  $c = Capture $Captions; $p = Capture $Reply; $s = Capture $Script
  $gap = 16
  $left = [Math]::Max($c.Width, $p.Width)
  $w = $left + $gap + $s.Width + 2 * $gap
  $h = [Math]::Max($c.Height + $gap + $p.Height, $s.Height) + 2 * $gap
  # even dimensions for the video encoder
  $w += $w % 2; $h += $h % 2
  $frame = New-Object System.Drawing.Bitmap $w, $h
  $g = [System.Drawing.Graphics]::FromImage($frame); $g.Clear([System.Drawing.Color]::FromArgb(11, 13, 16))
  $g.DrawImage($c, $gap, $gap); $g.DrawImage($p, $gap, $gap + $c.Height + $gap); $g.DrawImage($s, $gap + $left + $gap, $gap)
  $g.Dispose(); $c.Dispose(); $p.Dispose(); $s.Dispose()
  $frame.Save((Join-Path $OutDir ('f{0:D5}.png' -f $i)), [System.Drawing.Imaging.ImageFormat]::Png); $frame.Dispose()
  $i++
  $wait = $IntervalMs - ((Get-Date) - $t0).TotalMilliseconds
  if ($wait -gt 0) { Start-Sleep -Milliseconds ([int]$wait) }
}
"frames=$i"
