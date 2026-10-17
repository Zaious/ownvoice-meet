param([string]$Exe, [string]$Out)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes, System.Drawing, System.Windows.Forms
Add-Type @"
using System; using System.Runtime.InteropServices;
public class Win { [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
[DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
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
function Texts($w) {
  $all = $w.FindAll([System.Windows.Automation.TreeScope]::Descendants, (New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::ControlTypeProperty, [System.Windows.Automation.ControlType]::Text)))
  ($all | ForEach-Object { $_.Current.Name } | Where-Object { $_ }) -join ' | '
}

# UIA reports physical pixels; without this, SetCursorPos is scaled on a 125% display and clicks drift.
[void][Win]::SetProcessDPIAware()
$env:OWNVOICE_DEMO = '1'
# See the app as a new user would: no development keys from .env.local.
$env:OWNVOICE_NO_DEV_KEYS = '1'
$p = Start-Process $Exe -PassThru
try {
  $main = Find-Window 'OwnVoice Meet'
  if (-not $main) { throw 'captions window not found' }
  Start-Sleep -Seconds 9   # let the demo play a few lines
  Shot $main "$Out\main.png"
  $t = Texts $main
  "captions_has_zh=" + ($t -match '謝謝大家參加')
  "captions_has_en=" + ($t -match 'project update')

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
  if ($script) { Shot $script "$Out\script.png" }

  # Settings opens; listening without a Gemini key points the user to it instead of failing quietly.
  "click_settings=" + (Click $main '⚙')
  $settings = Find-Window 'OwnVoice Meet · Settings' 10
  "settings_window=" + [bool]$settings
  if ($settings) { Start-Sleep -Milliseconds 800; Shot $settings "$Out\settings.png"; "settings_shows_key_state=" + ((Texts $settings) -match '已設定|未設定') }
  "click_listen=" + (Click $main '▶')
  Start-Sleep -Milliseconds 800
  "listen_without_key_hint=" + ((Texts $main) -match '設定.*API key')

  # closing the reply window by itself must flip the main button back
  if ($reply) { $reply.GetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern).Close(); Start-Sleep -Milliseconds 600 }
  "reply_closed=" + (-not (Find-Window 'OwnVoice Meet · Reply' 1))
  "app_alive_after_child_close=" + (-not $p.HasExited)
  # closing the captions window quits the whole app
  $main.GetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern).Close()
  [void]$p.WaitForExit(8000)
  "app_exited_with_main=" + $p.HasExited
  "script_closed_with_main=" + (-not (Find-Window 'OwnVoice Meet · Script' 1))
} finally {
  if (-not $p.HasExited) { Stop-Process -Id $p.Id -Force }
}
