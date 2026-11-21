# End-to-end check of saving (archive.rs, recorder.rs): listens to the computer's own audio with
# saving set to "recording and transcript", speaks two English sentences with Windows TTS, starts a
# second segment between them with the "新段落" button, stops, then reports the session folder:
# each segment's WAV (size and length from its header) and the transcripts.
#   powershell -ExecutionPolicy Bypass -File scripts\e2e-archive.ps1 -Exe src-tauri\target\debug\ownvoice-meet.exe
# Debug builds read keys from .env.local. Uses Gemini Live and Claude Haiku 5.5 unless -Stt / -Llm say
# otherwise; -Stt record-only checks the recording without speech recognition.
param([string]$Exe, [string]$Stt = 'gemini-live', [string]$Llm = 'claude-haiku-5-5')
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes, System.Speech
$UIA = [System.Windows.Automation.AutomationElement]; $Scope = [System.Windows.Automation.TreeScope]; $CT = [System.Windows.Automation.ControlType]

$cfg = Join-Path ([IO.Path]::GetTempPath()) "ownvoice-archive-$(Get-Random)"
New-Item -ItemType Directory -Force $cfg | Out-Null
$settings = [ordered]@{ stt_id = $Stt; stt_language = 'en-US'; llm_id = $Llm; audio_source = 'loopback'; save_mode = 'audio'; agenda = "Keynote`nPanel discussion"; view = @{ reply_open = $false } }
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
      if ($b.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern, [ref]$pat)) { $pat.Invoke() }
      else { $b.GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle() }
      return
    }
    Start-Sleep -Milliseconds 300
  }
  throw "button '$prefix' not found"
}
function Texts($w) {
  ($w.FindAll($Scope::Descendants, (New-Object System.Windows.Automation.PropertyCondition($UIA::ControlTypeProperty, $CT::Text))) | ForEach-Object { $_.Current.Name } | Where-Object { $_ }) -join ' | '
}
function Say($text) { $tts = New-Object System.Speech.Synthesis.SpeechSynthesizer; $tts.Rate = 0; $tts.Speak($text); $tts.Dispose() }
function U32($bytes, $at) { [BitConverter]::ToUInt32($bytes, $at) }

try {
  $main = Win 'OwnVoice Meet'; if (-not $main) { throw 'main window not found' }
  Press $main '▶'
  Start-Sleep -Seconds 6
  "status after listen: " + (Texts $main)
  Say 'Welcome to the keynote. Today we talk about digital libraries.'
  Start-Sleep -Seconds 6
  Press $main '新段落'
  Start-Sleep -Seconds 2
  Say 'Now the panel begins. Please hold your questions until the end.'
  Start-Sleep -Seconds 8
  Press $main '■'
  Start-Sleep -Seconds 3
  "status after stop: " + (Texts $main)
} finally {
  Get-Process -Id $p.Id -ErrorAction SilentlyContinue | Stop-Process -Force -Confirm:$false
}

$session = Get-ChildItem (Join-Path $cfg 'sessions') -Directory | Select-Object -First 1
if (-not $session) { throw 'no session folder was written' }
"session folder: $($session.Name)"
foreach ($f in Get-ChildItem $session.FullName -File | Sort-Object Name) {
  if ($f.Extension -eq '.wav') {
    $b = [IO.File]::ReadAllBytes($f.FullName)
    $rate = U32 $b 24; $data = U32 $b 40
    "  $($f.Name): $($f.Length) bytes, header data $data bytes = $([math]::Round($data / 2 / $rate, 1)) s at $rate Hz, header matches file: $($data + 44 -eq $f.Length)"
  } else {
    "  $($f.Name): $($f.Length) bytes"
  }
}
"--- transcript.md ---"
Get-Content (Join-Path $session.FullName 'transcript.md') -Encoding UTF8
Remove-Item -Recurse -Force $cfg
