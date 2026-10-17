# End-to-end listening check on Windows: launches OwnVoice Meet with a given config folder, presses
# Listen, speaks English through the default speakers with Windows TTS, and reports the captions.
#   powershell -ExecutionPolicy Bypass -File scripts\e2e-listen.ps1 -Exe src-tauri\target\debug\ownvoice-meet.exe -ConfigDir <folder>
# The config folder holds settings.json (which speech and translation options to use).
#   -Stt <catalog id>  writes that speech option into the folder's settings.json first, e.g.
#                      -Stt windows-live-captions (Windows Live Captions, no key needed)
#   -LongSentence      first speaks one long sentence without a pause and reports whether Chinese
#                      appeared while it was still being spoken (early translation), then the
#                      usage counts (`early` = translations of an unfinished sentence)
param([string]$Exe, [string]$ConfigDir, [int]$WaitSeconds = 8, [string]$Stt = '', [switch]$LongSentence)
$ErrorActionPreference = 'Stop'
if ($Stt) {
  New-Item -ItemType Directory -Force $ConfigDir | Out-Null
  $file = Join-Path $ConfigDir 'settings.json'
  $cfg = if (Test-Path $file) { Get-Content $file -Raw -Encoding UTF8 | ConvertFrom-Json } else { [pscustomobject]@{} }
  $cfg | Add-Member -NotePropertyName stt_id -NotePropertyValue $Stt -Force
  [IO.File]::WriteAllText($file, ($cfg | ConvertTo-Json), (New-Object Text.UTF8Encoding($false)))
  "settings=" + (Get-Content $file -Raw -Encoding UTF8).Replace("`r`n", ' ')
}
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
# Only the caption rows (the list labelled 字幕), without its placeholder: the status line is Chinese too.
function Caption-Texts($w) {
  $log = $w.FindFirst($Scope::Descendants, (New-Object System.Windows.Automation.PropertyCondition($UIA::NameProperty, '字幕')))
  if (-not $log) { return '' }
  $all = $log.FindAll($Scope::Descendants, (New-Object System.Windows.Automation.PropertyCondition($UIA::ControlTypeProperty, $CT::Text)))
  ($all | ForEach-Object { $_.Current.Name } | Where-Object { $_ -and -not $_.StartsWith('按「聆聽」') }) -join ' | '
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
  if ($LongSentence) {
    $long = 'So the plan for the next two quarters is to move the whole deployment pipeline onto the new cluster and then retire the old build servers once every single team has signed off on the migration checklist that we shared last week'
    $t0 = Get-Date
    [void]$tts.SpeakAsync($long)
    Start-Sleep -Milliseconds 200
    $seen = $false
    while ($tts.State -eq 'Speaking') {
      $now = Caption-Texts $main
      if (-not $seen -and $now -match '[一-鿿]') {
        $seen = $true
        "early_zh_while_speaking_at_s=" + [math]::Round(((Get-Date) - $t0).TotalSeconds, 1) + " :: " + $now
      }
      Start-Sleep -Milliseconds 250
    }
    "long_sentence_spoken_s=" + [math]::Round(((Get-Date) - $t0).TotalSeconds, 1)
    if (-not $seen) { 'early_zh_while_speaking=none' }
    Start-Sleep -Milliseconds 1500
  }
  foreach ($s in 'Thanks everyone for joining the call today.', 'Could you share your screen and walk us through the deployment timeline?') {
    $tts.Speak($s); Start-Sleep -Milliseconds 1500
  }
  Start-Sleep -Seconds $WaitSeconds
  "captions=" + (Texts $main)
  "click_stop=" + (Click-Button $main '■')
  Start-Sleep -Seconds 2
  $usage = Join-Path $ConfigDir 'usage.json'
  if (Test-Path $usage) { "usage=" + (Get-Content $usage -Raw -Encoding UTF8) }
  # With OWNVOICE_DEBUG=1: when each sentence ended, and each translation (early or final) shown.
  $log = Join-Path $ConfigDir 'captions-debug.log'
  if (Test-Path $log) { 'captions_log:'; Get-Content $log -Encoding UTF8 }
} finally {
  if (-not $p.HasExited) { Stop-Process -Id $p.Id -Force }
}
