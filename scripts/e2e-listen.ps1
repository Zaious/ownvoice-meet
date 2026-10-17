# End-to-end listening check on Windows: launches OwnVoice Meet with a given config folder, presses
# Listen, speaks English through the default speakers with Windows TTS, and reports the captions.
#   powershell -ExecutionPolicy Bypass -File scripts\e2e-listen.ps1 -Exe src-tauri\target\debug\ownvoice-meet.exe -ConfigDir <folder>
# The config folder holds settings.json (which speech and translation options to use).
param([string]$Exe, [string]$ConfigDir, [int]$WaitSeconds = 8)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes, System.Speech
Add-Type @"
using System; using System.Runtime.InteropServices;
public class W3 {
[DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
[DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
[DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
[DllImport("user32.dll")] public static extern void mouse_event(uint f, uint x, uint y, uint d, UIntPtr e); }
"@
[void][W3]::SetProcessDPIAware()
$UIA = [System.Windows.Automation.AutomationElement]
$Scope = [System.Windows.Automation.TreeScope]
$CT = [System.Windows.Automation.ControlType]

$env:OWNVOICE_CONFIG_DIR = $ConfigDir
$p = Start-Process $Exe -PassThru
function Main-Window {
  $byPid = New-Object System.Windows.Automation.PropertyCondition($UIA::ProcessIdProperty, $p.Id)
  $byName = New-Object System.Windows.Automation.PropertyCondition($UIA::NameProperty, 'OwnVoice Meet')
  $UIA::RootElement.FindFirst($Scope::Children, (New-Object System.Windows.Automation.AndCondition($byPid, $byName)))
}
function Texts($w) {
  $all = $w.FindAll($Scope::Descendants, (New-Object System.Windows.Automation.PropertyCondition($UIA::ControlTypeProperty, $CT::Text)))
  ($all | ForEach-Object { $_.Current.Name } | Where-Object { $_ }) -join ' | '
}
function Click-Button($w, $prefix) {
  $all = $w.FindAll($Scope::Descendants, (New-Object System.Windows.Automation.PropertyCondition($UIA::ControlTypeProperty, $CT::Button)))
  foreach ($b in $all) {
    if (-not $b.Current.Name.StartsWith($prefix)) { continue }
    $r = $b.Current.BoundingRectangle
    [void][W3]::SetForegroundWindow([IntPtr]$w.Current.NativeWindowHandle); Start-Sleep -Milliseconds 400
    [void][W3]::SetCursorPos([int]($r.X + $r.Width / 2), [int]($r.Y + $r.Height / 2))
    [W3]::mouse_event(2, 0, 0, 0, [UIntPtr]::Zero); [W3]::mouse_event(4, 0, 0, 0, [UIntPtr]::Zero)
    return $true
  }
  $false
}
try {
  $main = $null; $end = (Get-Date).AddSeconds(15)
  while (-not $main -and (Get-Date) -lt $end) { $main = Main-Window; Start-Sleep -Milliseconds 300 }
  if (-not $main) { throw 'window not found' }
  Start-Sleep -Seconds 2
  "click_listen=" + (Click-Button $main '▶')
  Start-Sleep -Seconds 3
  "status_after_listen=" + (Texts $main)
  $tts = New-Object System.Speech.Synthesis.SpeechSynthesizer
  foreach ($s in 'Thanks everyone for joining the call today.', 'Could you share your screen and walk us through the deployment timeline?') {
    $tts.Speak($s); Start-Sleep -Milliseconds 1500
  }
  Start-Sleep -Seconds $WaitSeconds
  "captions=" + (Texts $main)
  "click_stop=" + (Click-Button $main '■')
  Start-Sleep -Seconds 2
} finally {
  if (-not $p.HasExited) { Stop-Process -Id $p.Id -Force }
}
