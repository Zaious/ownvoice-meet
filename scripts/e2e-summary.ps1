# End-to-end check of the live summary (batch.rs, summary.rs): listens to the computer's own audio with
# Gemini Live and Claude Haiku 5.5, saving the transcript, with the summary interval shortened to
# seconds; speaks a few sentences with Windows TTS, opens the 摘要 window and reads its points, then
# stops and shows the session folder's summary.md.
#   powershell -ExecutionPolicy Bypass -File scripts\e2e-summary.ps1 -Exe src-tauri\target\debug\ownvoice-meet.exe
# Debug builds read keys from .env.local.
# Another language: -Language ja-JP -AudioDir <folder of .wav files>, played in name order instead
# of the English TTS sentences.
param([string]$Exe, [double]$EveryMin = 0.25, [string]$Language = 'en-US', [string]$AudioDir)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes, System.Speech
$UIA = [System.Windows.Automation.AutomationElement]; $Scope = [System.Windows.Automation.TreeScope]; $CT = [System.Windows.Automation.ControlType]

$cfg = Join-Path ([IO.Path]::GetTempPath()) "ownvoice-summary-$(Get-Random)"
New-Item -ItemType Directory -Force $cfg | Out-Null
$settings = [ordered]@{ stt_id = 'gemini-live'; stt_language = $Language; llm_id = 'claude-haiku-5-5'; audio_source = 'loopback'; save_mode = 'text'; summary_every_min = $EveryMin; update_check = $false; view = @{ reply_open = $false } }
[IO.File]::WriteAllText((Join-Path $cfg 'settings.json'), ($settings | ConvertTo-Json), (New-Object Text.UTF8Encoding($false)))
$env:OWNVOICE_CONFIG_DIR = $cfg
$p = Start-Process $Exe -PassThru

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
function Texts($w) { ($w.FindAll($Scope::Descendants, (New-Object System.Windows.Automation.PropertyCondition($UIA::ControlTypeProperty, $CT::Text))) | ForEach-Object { $_.Current.Name } | Where-Object { $_ }) }
function Say($text) { $tts = New-Object System.Speech.Synthesis.SpeechSynthesizer; $tts.Speak($text); $tts.Dispose() }

try {
  $main = Win 'OwnVoice Meet'; if (-not $main) { throw 'main window not found' }
  Press $main '▶'
  Start-Sleep -Seconds 5
  # A pause after each sentence, so each one ends as its own caption line.
  if ($AudioDir) {
    foreach ($wav in (Get-ChildItem $AudioDir -Filter *.wav | Sort-Object Name)) {
      (New-Object System.Media.SoundPlayer $wav.FullName).PlaySync()
      Start-Sleep -Milliseconds 1800
    }
  } else { foreach ($line in @(
      'Welcome to the panel on digital libraries. I am the moderator.',
      'Our first panelist will talk about preserving web archives for twenty years.',
      'The library decided to move the whole collection to open formats by March next year.',
      'One open question is who pays for the storage after the grant ends.')) {
    Say $line
    Start-Sleep -Milliseconds 1800
  } }
  Press $main '摘要'
  $sum = Win 'OwnVoice Meet · Summary'
  $end = (Get-Date).AddSeconds(90)
  while ((Get-Date) -lt $end -and -not ((Texts $sum) -match '更新，共')) { Start-Sleep -Seconds 2 }
  "summary window state: " + (((Texts $sum) | Where-Object { $_ -match '更新|整理中|失敗' }) -join ' | ')
  "summary points:"
  $sum.FindAll($Scope::Descendants, (New-Object System.Windows.Automation.PropertyCondition($UIA::ControlTypeProperty, $CT::ListItem))) | ForEach-Object { "  - " + $_.Current.Name }
  Press $main '■'
  Start-Sleep -Seconds 12
} finally {
  Get-Process -Id $p.Id -ErrorAction SilentlyContinue | Stop-Process -Force -Confirm:$false
}
$session = Get-ChildItem (Join-Path $cfg 'sessions') -Directory | Select-Object -First 1
"session folder files: " + ((Get-ChildItem $session.FullName -File | ForEach-Object Name) -join ', ')
"transcript lines: " + @(Get-Content (Join-Path $session.FullName 'transcript.md') -Encoding UTF8 | Where-Object { $_ -match '^\[\d' }).Count
"--- transcript.md (lines) ---"
Get-Content (Join-Path $session.FullName 'transcript.md') -Encoding UTF8 | Where-Object { $_ -match '^\[\d' -or $_ -match '^　' }
"--- summary.md ---"
Get-Content (Join-Path $session.FullName 'summary.md') -Encoding UTF8
Remove-Item -Recurse -Force $cfg
