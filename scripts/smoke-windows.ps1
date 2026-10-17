param([string]$Exe, [string]$Out)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes, System.Drawing, System.Windows.Forms
Add-Type @"
using System; using System.Runtime.InteropServices;
public class Win { [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
[StructLayout(LayoutKind.Sequential)] public struct POINT { public int X, Y; }
[DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
[DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h, out RECT r);
[DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr h, ref POINT p);
[DllImport("user32.dll")] public static extern int GetWindowLong(IntPtr h, int i);
[DllImport("user32.dll")] public static extern bool GetWindowDisplayAffinity(IntPtr h, out uint a);
[DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int cx, int cy, uint f);
[DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
[DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
[DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
[DllImport("user32.dll")] public static extern void mouse_event(uint f, uint x, uint y, uint d, UIntPtr e); }
"@
$A = [System.Windows.Automation.AutomationElement]
$T = [System.Windows.Automation.TreeScope]

function Find-Window($title, $timeout = 15) {
  # Match the process too: another running app may also have a window titled "OwnVoice Meet".
  $byName = New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::NameProperty, $title)
  $byPid = New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::ProcessIdProperty, $p.Id)
  $cond = New-Object System.Windows.Automation.AndCondition($byName, $byPid)
  $end = (Get-Date).AddSeconds($timeout)
  while ((Get-Date) -lt $end) {
    $w = [System.Windows.Automation.AutomationElement]::RootElement.FindFirst([System.Windows.Automation.TreeScope]::Children, $cond)
    if ($w) { return $w }
    Start-Sleep -Milliseconds 300
  }
  return $null
}
function Shot($w, $file) {
  $h = [IntPtr]$w.Current.NativeWindowHandle; $r = New-Object Win+RECT
  [void][Win]::GetWindowRect($h, [ref]$r)
  $bmp = New-Object System.Drawing.Bitmap ($r.R - $r.L), ($r.B - $r.T)
  $g = [System.Drawing.Graphics]::FromImage($bmp); $g.CopyFromScreen($r.L, $r.T, 0, 0, $bmp.Size)
  $bmp.Save($file); $g.Dispose(); $bmp.Dispose()
}
function Click($w, $namePrefix) {
  $all = $w.FindAll([System.Windows.Automation.TreeScope]::Descendants, (New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::ControlTypeProperty, [System.Windows.Automation.ControlType]::Button)))
  foreach ($b in $all) {
    if (-not $b.Current.Name.StartsWith($namePrefix)) { continue }
    # WebView2 buttons do not always expose Invoke; click the middle of the button instead.
    $r = $b.Current.BoundingRectangle
    [void][Win]::SetForegroundWindow([IntPtr]$w.Current.NativeWindowHandle)
    Start-Sleep -Milliseconds 400   # let activation settle so the click is not spent on it
    [void][Win]::SetCursorPos([int]($r.X + $r.Width / 2), [int]($r.Y + $r.Height / 2))
    [Win]::mouse_event(2, 0, 0, 0, [UIntPtr]::Zero); [Win]::mouse_event(4, 0, 0, 0, [UIntPtr]::Zero)
    return $true
  }
  return $false
}
function ClickAt($x, $y) {
  [void][Win]::SetCursorPos([int]$x, [int]$y)
  [Win]::mouse_event(2, 0, 0, 0, [UIntPtr]::Zero); [Win]::mouse_event(4, 0, 0, 0, [UIntPtr]::Zero)
}
function Hwnd($w) { [IntPtr]$w.Current.NativeWindowHandle }
function Client($w) {
  $r = New-Object Win+RECT; [void][Win]::GetClientRect((Hwnd $w), [ref]$r)
  $pt = New-Object Win+POINT; [void][Win]::ClientToScreen((Hwnd $w), [ref]$pt)
  @{ X = $pt.X; Y = $pt.Y; W = $r.R; H = $r.B }
}
function Find-Desc($w, $type, $timeout = 8) {
  $cond = New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::ControlTypeProperty, $type)
  $end = (Get-Date).AddSeconds($timeout)
  while ((Get-Date) -lt $end) {
    $e = $w.FindFirst([System.Windows.Automation.TreeScope]::Descendants, $cond)
    if ($e) { return $e }
    Start-Sleep -Milliseconds 250
  }
  return $null
}
function Text-Height($w, $pattern) {
  $all = $w.FindAll([System.Windows.Automation.TreeScope]::Descendants, (New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::ControlTypeProperty, [System.Windows.Automation.ControlType]::Text)))
  foreach ($e in $all) { if ($e.Current.Name -match $pattern) { return $e.Current.BoundingRectangle.Height } }
  return 0
}
function Grab($x, $y, $w, $h) {
  $bmp = New-Object System.Drawing.Bitmap ([int]$w), ([int]$h)
  $g = [System.Drawing.Graphics]::FromImage($bmp); $g.CopyFromScreen([int]$x, [int]$y, 0, 0, $bmp.Size); $g.Dispose()
  return $bmp
}
# Share of the region that is the magenta backdrop, i.e. where the screen capture saw no window.
function Magenta-Share($bmp) {
  $hit = 0; $n = 0
  for ($y = 0; $y -lt $bmp.Height; $y += 6) { for ($x = 0; $x -lt $bmp.Width; $x += 6) {
    $c = $bmp.GetPixel($x, $y); $n++
    if ($c.R -gt 200 -and $c.G -lt 60 -and $c.B -gt 200) { $hit++ }
  } }
  [math]::Round($hit / [math]::Max($n, 1), 3)
}
function Pump { for ($i = 0; $i -lt 10; $i++) { [System.Windows.Forms.Application]::DoEvents(); Start-Sleep -Milliseconds 30 } }
# A solid magenta window of our own placed right under one of the app's windows: what a capture
# shows through an excluded or faded window, and where a click-through click lands.
$script:backdropClicks = 0
function Backdrop-Under($w) {
  $r = New-Object Win+RECT; [void][Win]::GetWindowRect((Hwnd $w), [ref]$r)
  $script:bd.Bounds = New-Object System.Drawing.Rectangle $r.L, $r.T, ($r.R - $r.L), ($r.B - $r.T)
  $script:bd.Show(); Pump
  # Both are topmost; put the app's window back on top of the backdrop (no move, size or activation).
  [void][Win]::SetWindowPos((Hwnd $w), [IntPtr](-1), 0, 0, 0, 0, 0x13)
  Pump; Start-Sleep -Milliseconds 300
}
function View-Setting { (Get-Content -Raw -Encoding UTF8 (Join-Path $env:OWNVOICE_CONFIG_DIR 'settings.json') | ConvertFrom-Json).view }
function Texts($w) {
  $all = $w.FindAll([System.Windows.Automation.TreeScope]::Descendants, (New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::ControlTypeProperty, [System.Windows.Automation.ControlType]::Text)))
  ($all | ForEach-Object { $_.Current.Name } | Where-Object { $_ }) -join ' | '
}

# UIA reports physical pixels; without this, SetCursorPos is scaled on a 125% display and clicks drift.
[void][Win]::SetProcessDPIAware()
$env:OWNVOICE_DEMO = '1'
# See the app as a new user would: no development keys from .env.local.
$env:OWNVOICE_NO_DEV_KEYS = '1'
# A fresh settings folder: the view settings below start from their defaults and never touch the user's.
$env:OWNVOICE_CONFIG_DIR = Join-Path $Out 'config'
Remove-Item -Recurse -Force $env:OWNVOICE_CONFIG_DIR -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $env:OWNVOICE_CONFIG_DIR | Out-Null
$bd = New-Object System.Windows.Forms.Form
$bd.FormBorderStyle = 'None'; $bd.BackColor = [System.Drawing.Color]::Magenta; $bd.TopMost = $true
$bd.ShowInTaskbar = $false; $bd.StartPosition = 'Manual'
$bd.Add_MouseDown({ $script:backdropClicks++ })
$p = Start-Process $Exe -PassThru
try {
  $main = Find-Window 'OwnVoice Meet'
  if (-not $main) { throw 'captions window not found' }
  Start-Sleep -Seconds 9   # let the demo play a few lines
  Shot $main "$Out\main.png"
  $t = Texts $main
  "captions_has_zh=" + ($t -match '謝謝大家參加')
  "captions_has_en=" + ($t -match 'project update')
  # Both global keys (reply, click-through) registered: no failure message in the status line.
  "shortcuts_registered=" + (-not ($t -match '快捷鍵註冊失敗'))

  # Text size: A+ twice makes the caption text taller, live, and is stored.
  $h0 = Text-Height $main '謝謝大家參加'
  [void](Click $main 'A+'); Start-Sleep -Milliseconds 500; [void](Click $main 'A+'); Start-Sleep -Milliseconds 800
  $h1 = Text-Height $main '謝謝大家參加'
  "font_grows_live=" + ($h1 -gt $h0) + " (text height $h0 -> $h1)"
  "font_saved=" + ((View-Setting).captions.font_px -eq 18)

  # Translation only hides the English of translated lines, and comes back.
  [void](Click $main '只看中文'); Start-Sleep -Milliseconds 800
  $t = Texts $main
  "zh_only_hides_en=" + (($t -match '謝謝大家參加') -and -not ($t -match 'project update'))
  "zh_only_saved=" + ((View-Setting).zh_only -eq $true)
  [void](Click $main '只看中文'); Start-Sleep -Milliseconds 800
  "zh_only_off_shows_en=" + ((Texts $main) -match 'project update')

  # Background opacity: over the magenta backdrop, the padding left of the captions turns pink.
  Backdrop-Under $main
  $c = Client $main
  $px = { $b = Grab ($c.X + 5) ($c.Y + [int]($c.H * 0.6)) 1 1; $v = $b.GetPixel(0, 0); $b.Dispose(); $v }
  $before = & $px
  $slider = Find-Desc $main ([System.Windows.Automation.ControlType]::Slider)
  $sr = $slider.Current.BoundingRectangle
  [void][Win]::SetForegroundWindow((Hwnd $main)); Start-Sleep -Milliseconds 300
  # Set the value through UI Automation: it fires the page's input event like a drag does, and does
  # not depend on where the slider sits once the toolbar wraps (a click there could hit the backdrop).
  $slider.GetCurrentPattern([System.Windows.Automation.RangeValuePattern]::Pattern).SetValue(40); Start-Sleep -Milliseconds 800
  $after = & $px
  "opacity_fades_background=" + ($before.R -lt 40 -and $after.R -gt 100) + " (pixel $($before.R),$($before.G),$($before.B) -> $($after.R),$($after.G),$($after.B))"
  "opacity_saved=" + ((View-Setting).captions.opacity) + "%"
  "opacity_text_readable=" + ((Texts $main) -match '謝謝大家參加')
  Shot $main "$Out\main-faded.png"

  # Click-through: clicks land on the backdrop underneath; Ctrl+Shift+F8 brings the mouse back.
  [void](Click $main '穿透'); Start-Sleep -Milliseconds 800
  $ex = [Win]::GetWindowLong((Hwnd $main), -20)
  "click_through_on=" + (($ex -band 0x20) -ne 0)
  "click_through_note=" + ((Texts $main) -match '滑鼠穿透中')
  $clicks0 = $script:backdropClicks
  ClickAt ($c.X + $c.W / 2) ($c.Y + $c.H * 0.6); Pump
  "click_passes_through=" + ($script:backdropClicks -eq $clicks0 + 1)
  [System.Windows.Forms.SendKeys]::SendWait('^+{F8}'); Start-Sleep -Milliseconds 800
  $ex = [Win]::GetWindowLong((Hwnd $main), -20)
  "shortcut_turns_it_off=" + (($ex -band 0x20) -eq 0) + (" (exstyle 0x{0:X})" -f $ex)
  # The click raised the backdrop (it was activated); put the captions window back above it.
  [void][Win]::SetWindowPos((Hwnd $main), [IntPtr](-1), 0, 0, 0, 0, 0x13); Pump
  ClickAt ($c.X + $c.W / 2) ($c.Y + $c.H * 0.6); Pump
  "click_lands_on_captions_again=" + ($script:backdropClicks -eq $clicks0 + 1)
  "click_through_note_gone=" + (-not ((Texts $main) -match '滑鼠穿透中'))
  $bd.Hide(); Pump

  "click_script=" + (Click $main '講稿')
  $script = Find-Window 'OwnVoice Meet · Script' 10
  "script_window=" + [bool]$script
  "click_reply=" + (Click $main '回覆')
  $reply = Find-Window 'OwnVoice Meet · Reply' 10
  "reply_window=" + [bool]$reply
  if ($reply) {
    # The web view's accessibility tree fills in after the window appears: wait for the text box.
    $edit = $null; $end = (Get-Date).AddSeconds(8)
    while (-not $edit -and (Get-Date) -lt $end) {
      $edit = $reply.FindFirst([System.Windows.Automation.TreeScope]::Descendants, (New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::ControlTypeProperty, [System.Windows.Automation.ControlType]::Edit)))
      if (-not $edit) { Start-Sleep -Milliseconds 250 }
    }
    $edit.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue('我同意，下週再確認')
    # SendKeys goes to the foreground window: bring the reply window forward and click into the box first.
    $r = $edit.Current.BoundingRectangle
    [void][Win]::SetForegroundWindow([IntPtr]$reply.Current.NativeWindowHandle)
    [void][Win]::SetCursorPos([int]($r.X + $r.Width / 2), [int]($r.Y + $r.Height / 2))
    [Win]::mouse_event(2, 0, 0, 0, [UIntPtr]::Zero); [Win]::mouse_event(4, 0, 0, 0, [UIntPtr]::Zero)
    Start-Sleep -Milliseconds 300
    [System.Windows.Forms.SendKeys]::SendWait('{ENTER}')
    Start-Sleep -Milliseconds 800
    $rt = Texts $reply
    "reply_shows_input=" + ($rt -match '我同意')
    "reply_points_to_key=" + ($rt -match 'API key')
    Shot $reply "$Out\reply.png"
  }
  # Reply and script are left out of screen capture, so these two shots show what is behind them.
  if ($script) { Shot $script "$Out\script.png" }

  # Settings opens; listening without a Gemini key points the user to it instead of failing quietly.
  "click_settings=" + (Click $main '⚙')
  $settings = Find-Window 'OwnVoice Meet · Settings' 10
  "settings_window=" + [bool]$settings
  if ($settings) { Start-Sleep -Milliseconds 800; Shot $settings "$Out\settings.png"; "settings_shows_key_state=" + ((Texts $settings) -match '已設定|未設定') }

  # Screen capture: reply and script are excluded (WDA_EXCLUDEFROMCAPTURE = 0x11), captions is not.
  # A screen grab of the reply window's place shows the backdrop behind it, until the setting is off.
  $aff = { param($w) $a = [uint32]0; [void][Win]::GetWindowDisplayAffinity((Hwnd $w), [ref]$a); '0x{0:X}' -f $a }
  if ($reply) { "reply_affinity=" + (& $aff $reply) }
  if ($script) { "script_affinity=" + (& $aff $script) }
  "captions_affinity=" + (& $aff $main)
  if ($reply -and $settings) {
    $grabReply = { param($file)
      Backdrop-Under $reply
      $r = New-Object Win+RECT; [void][Win]::GetWindowRect((Hwnd $reply), [ref]$r)
      $b = Grab $r.L $r.T ($r.R - $r.L) ($r.B - $r.T); $b.Save($file); $share = Magenta-Share $b; $b.Dispose()
      $bd.Hide(); Pump
      $share
    }
    $hidden = & $grabReply "$Out\capture-reply-hidden.png"
    "capture_skips_reply=" + ($hidden -gt 0.95) + " (backdrop share $hidden)"
    "reply_says_hidden=" + ((Texts $reply) -match '分享螢幕時隱藏')
    # The box is far down the settings form: toggle it through UI Automation rather than a click.
    $box = Find-Desc $settings ([System.Windows.Automation.ControlType]::CheckBox)
    $toggle = $box.GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern)
    $toggle.Toggle(); Start-Sleep -Milliseconds 800
    "setting_off_affinity=" + (& $aff $reply) + " saved=" + ((View-Setting).hide_from_capture)
    $shown = & $grabReply "$Out\capture-reply-shown.png"
    "capture_sees_reply_when_off=" + ($shown -lt 0.2) + " (backdrop share $shown)"
    $toggle.Toggle(); Start-Sleep -Milliseconds 800
    "setting_back_on_affinity=" + (& $aff $reply)
  }
  "click_listen=" + (Click $main '▶')
  Start-Sleep -Milliseconds 800
  "listen_without_key_hint=" + ((Texts $main) -match '設定.*API key')

  # closing the reply window by itself must flip the main button back
  if ($reply) { $reply.GetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern).Close(); Start-Sleep -Milliseconds 600 }
  "reply_closed=" + (-not (Find-Window 'OwnVoice Meet · Reply' 1))
  # The global key opens it again from another window (here: the settings window has focus).
  if ($settings) { [void][Win]::SetForegroundWindow((Hwnd $settings)); Start-Sleep -Milliseconds 300 }
  [System.Windows.Forms.SendKeys]::SendWait('^+ ')
  "reply_shortcut_opens=" + [bool](Find-Window 'OwnVoice Meet · Reply' 5)
  "app_alive_after_child_close=" + (-not $p.HasExited)
  # closing the captions window quits the whole app
  $main.GetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern).Close()
  [void]$p.WaitForExit(8000)
  "app_exited_with_main=" + $p.HasExited
  "script_closed_with_main=" + (-not (Find-Window 'OwnVoice Meet · Script' 1))
} finally {
  $bd.Dispose()
  if (-not $p.HasExited) { Stop-Process -Id $p.Id -Force }
}
