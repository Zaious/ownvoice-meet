# README screenshots: runs the app in Demo mode (captions, the example script and a sample reply; no
# keys, no audio), opens the reply, script and settings windows and saves each window, plus the three
# side by side, to docs/images. Each window is asked to draw itself (PrintWindow), so nothing else on
# the screen, and no other app, can end up in a picture. Look at every image before committing it.
#   powershell -ExecutionPolicy Bypass -File scripts\readme-screenshots.ps1 -Exe src-tauri\target\debug\ownvoice-meet.exe
# It moves the mouse to press buttons: leave the computer alone while it runs (about 30 s).
param([string]$Exe, [string]$OutDir = (Join-Path $PSScriptRoot '..\docs\images'))
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes, System.Drawing
Add-Type @"
using System; using System.Runtime.InteropServices;
public class W6 { [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
[DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
[DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int w, int hh, uint f);
[DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr hdc, uint flags);
[DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
[DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
[DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
[DllImport("user32.dll")] public static extern void mouse_event(uint f, uint x, uint y, uint d, UIntPtr e); }
"@
[void][W6]::SetProcessDPIAware()
$UIA = [System.Windows.Automation.AutomationElement]; $Scope = [System.Windows.Automation.TreeScope]; $CT = [System.Windows.Automation.ControlType]
New-Item -ItemType Directory -Force $OutDir | Out-Null

# A fresh settings folder with capture exclusion off, so the reply and script windows can be shot.
$cfg = Join-Path ([IO.Path]::GetTempPath()) "ownvoice-shots-$(Get-Random)"
New-Item -ItemType Directory -Force $cfg | Out-Null
'{"view":{"hide_from_capture":false}}' | Set-Content -Encoding ascii (Join-Path $cfg 'settings.json')
$env:OWNVOICE_CONFIG_DIR = $cfg; $env:OWNVOICE_DEMO = '1'; $env:OWNVOICE_NO_DEV_KEYS = '1'
$p = Start-Process $Exe -PassThru

function Win($title, $timeout = 15) {
  $c = New-Object System.Windows.Automation.AndCondition(
    (New-Object System.Windows.Automation.PropertyCondition($UIA::ProcessIdProperty, $p.Id)),
    (New-Object System.Windows.Automation.PropertyCondition($UIA::NameProperty, $title)))
  $end = (Get-Date).AddSeconds($timeout)
  while ((Get-Date) -lt $end) { $w = $UIA::RootElement.FindFirst($Scope::Children, $c); if ($w) { return $w }; Start-Sleep -Milliseconds 300 }
}
function Hwnd($w) { [IntPtr]$w.Current.NativeWindowHandle }
function Press($w, $prefix) {
  # The web view's accessibility tree fills in after the window appears: look for up to 8 s.
  $end = (Get-Date).AddSeconds(8)
  while ((Get-Date) -lt $end) {
    foreach ($b in $w.FindAll($Scope::Descendants, (New-Object System.Windows.Automation.PropertyCondition($UIA::ControlTypeProperty, $CT::Button)))) {
      if (-not $b.Current.Name.StartsWith($prefix)) { continue }
      $r = $b.Current.BoundingRectangle
      [void][W6]::SetForegroundWindow((Hwnd $w)); Start-Sleep -Milliseconds 400
      [void][W6]::SetCursorPos([int]($r.X + $r.Width / 2), [int]($r.Y + $r.Height / 2))
      [W6]::mouse_event(2, 0, 0, 0, [UIntPtr]::Zero); [W6]::mouse_event(4, 0, 0, 0, [UIntPtr]::Zero)
      return
    }
    Start-Sleep -Milliseconds 300
  }
  throw "button '$prefix' not found"
}
function Place($w, $x, $y, $width, $height) { [void][W6]::SetWindowPos((Hwnd $w), [IntPtr]::Zero, $x, $y, $width, $height, 0x0040) }
# The window draws itself into a bitmap (PW_RENDERFULLCONTENT = 2, needed for web views).
function Capture($w) {
  $r = New-Object W6+RECT; [void][W6]::GetWindowRect((Hwnd $w), [ref]$r)
  $bmp = New-Object System.Drawing.Bitmap ($r.R - $r.L), ($r.B - $r.T)
  $g = [System.Drawing.Graphics]::FromImage($bmp); $hdc = $g.GetHdc()
  $ok = [W6]::PrintWindow((Hwnd $w), $hdc, 2)
  $g.ReleaseHdc($hdc); $g.Dispose()
  if (-not $ok) { $bmp.Dispose(); throw "PrintWindow failed for $($w.Current.Name)" }
  $bmp
}
function Save($bmp, $name) { $f = Join-Path $OutDir $name; $bmp.Save($f, [System.Drawing.Imaging.ImageFormat]::Png); "saved $f ($($bmp.Width)x$($bmp.Height))" }

try {
  $main = Win 'OwnVoice Meet'; if (-not $main) { throw 'main window not found' }
  Place $main 20 20 980 560
  Press $main '講稿'; $script = Win 'OwnVoice Meet · Script'
  Press $main '回覆'; $reply = Win 'OwnVoice Meet · Reply'
  if (-not ($script -and $reply)) { throw 'reply or script window did not open' }
  Place $reply 20 600 980 560
  Place $script 1020 20 760 1000
  Start-Sleep -Seconds 16   # let the demo play all four lines
  $cap = Capture $main; $rep = Capture $reply; $scr = Capture $script
  Save $cap 'captions.png'; Save $rep 'reply.png'; Save $scr 'script.png'
  # Overview: captions above reply on the left, script on the right, on the app's background colour.
  $gap = 16
  $w = [Math]::Max($cap.Width, $rep.Width) + $gap + $scr.Width + 2 * $gap
  $h = [Math]::Max($cap.Height + $gap + $rep.Height, $scr.Height) + 2 * $gap
  $all = New-Object System.Drawing.Bitmap $w, $h
  $g = [System.Drawing.Graphics]::FromImage($all); $g.Clear([System.Drawing.Color]::FromArgb(11, 13, 16))
  $g.DrawImage($cap, $gap, $gap); $g.DrawImage($rep, $gap, $gap + $cap.Height + $gap)
  $g.DrawImage($scr, $gap + [Math]::Max($cap.Width, $rep.Width) + $gap, $gap); $g.Dispose()
  Save $all 'overview.png'
  Press $main '⚙'; $settings = Win 'OwnVoice Meet · Settings'
  if ($settings) { Place $settings 1020 20 760 1000; Start-Sleep -Milliseconds 1500; Save (Capture $settings) 'settings.png' }
} finally {
  if (-not $p.HasExited) { Stop-Process -Id $p.Id -Force }
  Remove-Item -Recurse -Force $cfg -ErrorAction SilentlyContinue
}
