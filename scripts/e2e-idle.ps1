# End-to-end check of the idle pause and end (idle.rs) with the times shortened to seconds: listens to
# the computer's own audio with Gemini Live, speaks a sentence, waits for the pause, speaks again to
# check that it wakes up without losing the first words, then waits for the automatic end. Reports the
# status line at each point and the audio seconds billed against the time spent listening.
#   powershell -ExecutionPolicy Bypass -File scripts\e2e-idle.ps1 -Exe src-tauri\target\debug\ownvoice-meet.exe
# Debug builds read keys from .env.local.
param([string]$Exe, [string]$Stt = 'gemini-live', [double]$PauseMin = 0.1, [double]$EndMin = 0.6)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes, System.Speech
$UIA = [System.Windows.Automation.AutomationElement]; $Scope = [System.Windows.Automation.TreeScope]; $CT = [System.Windows.Automation.ControlType]

$cfg = Join-Path ([IO.Path]::GetTempPath()) "ownvoice-idle-$(Get-Random)"
New-Item -ItemType Directory -Force $cfg | Out-Null
$settings = [ordered]@{ stt_id = $Stt; stt_language = 'en-US'; llm_id = 'claude-haiku-5-5'; audio_source = 'loopback'; save_mode = 'off'; idle_pause_min = $PauseMin; idle_end_min = $EndMin; view = @{ reply_open = $false } }
[IO.File]::WriteAllText((Join-Path $cfg 'settings.json'), ($settings | ConvertTo-Json), (New-Object Text.UTF8Encoding($false)))
$env:OWNVOICE_CONFIG_DIR = $cfg
$p = Start-Process $Exe -PassThru
$t0 = Get-Date

function Win($title, $timeout = 15) {
  $c = New-Object System.Windows.Automation.AndCondition(
    (New-Object System.Windows.Automation.PropertyCondition($UIA::ProcessIdProperty, $p.Id)),
    (New-Object System.Windows.Automation.PropertyCondition($UIA::NameProperty, $title)))
  $end = (Get-Date).AddSeconds($timeout)
  while ((Get-Date) -lt $end) { $w = $UIA::RootElement.FindFirst($Scope::Children, $c); if ($w) { return $w }; Start-Sleep -Milliseconds 300 }
}
function Press($w, $prefix) {
  $end = (Get-Date).AddSeconds(10)
  while ((Get-Date) -lt $end) {
    foreach ($b in $w.FindAll($Scope::Descendants, (New-Object System.Windows.Automation.PropertyCondition($UIA::ControlTypeProperty, $CT::Button)))) {
      if (-not $b.Current.Name.StartsWith($prefix) -or $b.Current.IsOffscreen) { continue }
      $pat = $null
      if ($b.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern, [ref]$pat)) { $pat.Invoke() } else { $b.GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle() }
      return
    }
    Start-Sleep -Milliseconds 300
  }
  throw "button '$prefix' not found"
}
function Status($w) {
  # The status line is the text that names the listening state; the web view gives it no id.
  $texts = $w.FindAll($Scope::Descendants, (New-Object System.Windows.Automation.PropertyCondition($UIA::ControlTypeProperty, $CT::Text))) | ForEach-Object { $_.Current.Name }
  $s = $texts | Where-Object { $_ -match '聆聽中|暫停辨識|自動結束|連線中|重新連線|已停止' } | Select-Object -First 1
  $btn = $w.FindAll($Scope::Descendants, (New-Object System.Windows.Automation.PropertyCondition($UIA::ControlTypeProperty, $CT::Button))) | Where-Object { $_.Current.Name -match '^(▶|■)' } | Select-Object -First 1
  $state = if ($btn -and $btn.Current.Name.StartsWith('▶')) { 'button ▶ (not listening)' } else { 'button ■ (listening)' }
  if ($s) { "$s  [$state]" } else { "(no status)  [$state]" }
}
function Captions($w) {
  $log = $w.FindFirst($Scope::Descendants, (New-Object System.Windows.Automation.PropertyCondition($UIA::NameProperty, '字幕')))
  ($log.FindAll($Scope::Descendants, (New-Object System.Windows.Automation.PropertyCondition($UIA::ControlTypeProperty, $CT::Text))) | ForEach-Object { $_.Current.Name } | Where-Object { $_ -and $_ -notmatch '^[\p{IsCJKUnifiedIdeographs}，。？！、「」：；（）\s\d]+$' }) -join ' | '
}
function Say($text) { $tts = New-Object System.Speech.Synthesis.SpeechSynthesizer; $tts.Speak($text); $tts.Dispose() }
function Stamp($label, $w) { "[{0,5:N1}s] {1}: {2}" -f ((Get-Date) - $t0).TotalSeconds, $label, (Status $w) }

try {
  $main = Win 'OwnVoice Meet'; if (-not $main) { throw 'main window not found' }
  Press $main '▶'
  Start-Sleep -Seconds 5
  Stamp 'listening' $main
  Say 'Good morning everyone, and welcome to the first session.'
  $pauseLimit = $PauseMin * 60 + 15
  $end = (Get-Date).AddSeconds($pauseLimit)
  while ((Get-Date) -lt $end -and (Status $main) -notmatch '暫停') { Start-Sleep -Milliseconds 500 }
  Stamp 'after quiet' $main
  Start-Sleep -Seconds 4
  Say 'Second speaker here. Thank you for having me today.'
  Start-Sleep -Seconds 8
  Stamp 'after speaking again' $main
  "captions: " + (Captions $main)
  $end = (Get-Date).AddSeconds($EndMin * 60 + 30)
  while ((Get-Date) -lt $end -and (Status $main) -notmatch '自動結束') { Start-Sleep -Milliseconds 500 }
  Stamp 'at the end' $main
  $listening = (Get-Date) - $t0
} finally {
  Get-Process -Id $p.Id -ErrorAction SilentlyContinue | Stop-Process -Force -Confirm:$false
}
$usage = Get-Content (Join-Path $cfg 'usage.json') -Raw | ConvertFrom-Json
$day = $usage.PSObject.Properties | Select-Object -First 1
$billed = $day.Value.$Stt.audio_secs
"audio seconds billed: {0:N1} of {1:N1} s with the app open" -f $billed, $listening.TotalSeconds
Remove-Item -Recurse -Force $cfg
