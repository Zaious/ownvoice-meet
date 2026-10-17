# Records the README demo with the real pipeline: Gemini Live hears two questions played through the
# speakers by Windows text-to-speech, Claude Haiku 5.5 translates them, the Q&A tab of the JCDL 2026
# example script is open, and for the question that is not in the script a Chinese reply is typed and
# turned into English to read aloud. Needs a debug build and GEMINI_API_KEY + ANTHROPIC_API_KEY in
# .env.local. Windows are opened and listening is started through OWNVOICE_AUTOMATION (debug builds
# only), so the mouse is not used; only the reply is typed. It plays sound for about 40 s.
#   powershell -ExecutionPolicy Bypass -File scripts\record-demo.ps1 -Exe src-tauri\target\debug\ownvoice-meet.exe
# Writes docs/media/demo.gif and demo.mp4 (ffmpeg on PATH). Look at them before committing.
param([string]$Exe, [string]$OutDir = (Join-Path $PSScriptRoot '..\docs\media'))
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes, System.Speech, System.Windows.Forms
Add-Type @"
using System; using System.Runtime.InteropServices;
public class W9 {
[DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
[DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int cmd);
[DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int w, int hh, uint f);
[DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(IntPtr ctx); }
"@
[void][W9]::SetProcessDpiAwarenessContext([IntPtr](-4))
$UIA = [System.Windows.Automation.AutomationElement]; $Scope = [System.Windows.Automation.TreeScope]; $CT = [System.Windows.Automation.ControlType]
$root = Resolve-Path (Join-Path $PSScriptRoot '..')
New-Item -ItemType Directory -Force $OutDir | Out-Null

# Fresh settings: Gemini Live + Claude Haiku 5.5, windows capturable, the JCDL example as the open script.
$cfg = Join-Path ([IO.Path]::GetTempPath()) "ownvoice-demo-$(Get-Random)"
$frames = Join-Path $cfg 'frames'; $stop = Join-Path $cfg 'stop'
New-Item -ItemType Directory -Force $frames | Out-Null
'{"stt_id":"gemini-live","stt_language":"en-US","llm_id":"claude-haiku-5-5","view":{"hide_from_capture":false}}' | Set-Content -Encoding ascii (Join-Path $cfg 'settings.json')
Copy-Item (Join-Path $root 'examples\scripts\journal-atlas-jcdl2026.md') (Join-Path $cfg 'script.md')
$env:OWNVOICE_CONFIG_DIR = $cfg
# The reply is typed by the reply window itself (no keyboard races): about when the second question
# has been heard, counted from when that window opens.
$env:OWNVOICE_AUTOMATION = '{"open":["reply","script"],"listen":true,"script_tab":"Q&A","reply_after_ms":21000,"reply_zh":"目前只收期刊，研討會還沒有。不過資料格式可以擴充，歡迎大家貢獻研討會的資料。"}'
Remove-Item Env:OWNVOICE_NO_DEV_KEYS -ErrorAction SilentlyContinue
$p = Start-Process $Exe -PassThru

function Win($title, $timeout = 20) {
  $c = New-Object System.Windows.Automation.AndCondition(
    (New-Object System.Windows.Automation.PropertyCondition($UIA::ProcessIdProperty, $p.Id)),
    (New-Object System.Windows.Automation.PropertyCondition($UIA::NameProperty, $title)))
  $end = (Get-Date).AddSeconds($timeout)
  while ((Get-Date) -lt $end) { $w = $UIA::RootElement.FindFirst($Scope::Children, $c); if ($w) { return $w }; Start-Sleep -Milliseconds 300 }
  throw "window '$title' not found"
}
function Hwnd($w) { [IntPtr]$w.Current.NativeWindowHandle }
function Texts($w) { ($w.FindAll($Scope::Descendants, (New-Object System.Windows.Automation.PropertyCondition($UIA::ControlTypeProperty, $CT::Text))) | ForEach-Object { $_.Current.Name }) -join ' | ' }

$recorder = $null
try {
  $main = Win 'OwnVoice Meet'; $reply = Win 'OwnVoice Meet · Reply'; $script = Win 'OwnVoice Meet · Script'
  # Window positions are remembered in the real app data folder, not the test settings folder: a
  # window left minimized by an earlier run would come back minimized. Restore and lay them out.
  Start-Sleep -Seconds 1
  foreach ($pair in @(@($main, 20, 20, 900, 400), @($reply, 20, 440, 900, 600), @($script, 940, 20, 760, 1020))) {
    $h = Hwnd $pair[0]
    [void][W9]::ShowWindow($h, 9)   # SW_RESTORE
    [void][W9]::SetWindowPos($h, [IntPtr]::Zero, $pair[1], $pair[2], $pair[3], $pair[4], 0x0040)
  }
  Start-Sleep -Seconds 1
  # Wait until listening is live before recording.
  $end = (Get-Date).AddSeconds(20)
  while ((Get-Date) -lt $end -and -not ((Texts $main) -match '聆聽中')) { Start-Sleep -Milliseconds 300 }
  if (-not ((Texts $main) -match '聆聽中')) { throw "listening did not start: $(Texts $main)" }

  $recorder = Start-Process powershell -PassThru -WindowStyle Hidden -ArgumentList @(
    '-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', (Join-Path $PSScriptRoot 'frame-recorder.ps1'),
    '-Captions', (Hwnd $main).ToInt64(), '-Reply', (Hwnd $reply).ToInt64(), '-Script', (Hwnd $script).ToInt64(),
    '-OutDir', $frames, '-StopFile', $stop)
  Start-Sleep -Seconds 2

  $tts = New-Object System.Speech.Synthesis.SpeechSynthesizer
  # A question the script has an answer for.
  $tts.Speak('How is this different from tools like Jane, or Bison?'); Start-Sleep -Seconds 5
  # A question it does not: answer in Chinese, read the English.
  $tts.Speak('Can it recommend conference venues too, or only journals?')
  # The reply window types the answer and sends it (reply_after_ms); wait for the English to finish.
  $end = (Get-Date).AddSeconds(25)
  while ((Get-Date) -lt $end -and -not ((Texts $reply) -match 'journals\.')) { Start-Sleep -Milliseconds 500 }
  Start-Sleep -Seconds 4
  "reply window: $(Texts $reply)"
} finally {
  New-Item -ItemType File -Force $stop | Out-Null
  if ($recorder) { $recorder.WaitForExit(15000) | Out-Null }
  if (-not $p.HasExited) { Stop-Process -Id $p.Id -Force }
  Remove-Item Env:OWNVOICE_AUTOMATION -ErrorAction SilentlyContinue
}

# The real frame rate, from when the frames were written (each capture takes its own time).
$files = Get-ChildItem $frames -Filter 'f*.png' | Sort-Object Name
$n = $files.Count
$secs = ($files[-1].LastWriteTime - $files[0].LastWriteTime).TotalSeconds
$fps = [Math]::Round(($n - 1) / [Math]::Max($secs, 1), 2)
"frames: $n over $([Math]::Round($secs, 1)) s = $fps fps"
$in = Join-Path $frames 'f%05d.png'
& ffmpeg -y -loglevel error -framerate $fps -i $in -vf 'scale=1280:-2:flags=lanczos' -c:v libx264 -pix_fmt yuv420p -r 30 (Join-Path $OutDir 'demo.mp4')
# GIF for the README: 3 frames a second, 800 px wide, and only the changed rectangle stored per frame
# (most of the picture is still), with one palette for the whole clip and no dithering noise.
& ffmpeg -y -loglevel error -framerate $fps -i $in -vf 'fps=3,scale=800:-1:flags=lanczos,split[a][b];[a]palettegen=max_colors=64:stats_mode=diff[p];[b][p]paletteuse=dither=none:diff_mode=rectangle' -loop 0 (Join-Path $OutDir 'demo.gif')
Get-ChildItem $OutDir -Filter 'demo.*' | ForEach-Object { "{0} {1:N0} KB" -f $_.Name, ($_.Length / 1KB) }
Remove-Item -Recurse -Force $cfg -ErrorAction SilentlyContinue
