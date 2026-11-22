# End-to-end check that Settings keeps what was typed without pressing the save button: opens
# Settings, types a glossary line and a save folder, closes the window, opens it again and reads the
# fields back, then the saved settings.json. Leaves keys alone (they would go to the real keychain).
#   powershell -ExecutionPolicy Bypass -File scripts\e2e-settings-autosave.ps1 -Exe src-tauri\target\debug\ownvoice-meet.exe
param([string]$Exe)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes, System.Windows.Forms
$UIA = [System.Windows.Automation.AutomationElement]; $Scope = [System.Windows.Automation.TreeScope]; $CT = [System.Windows.Automation.ControlType]

$cfg = Join-Path ([IO.Path]::GetTempPath()) "ownvoice-autosave-$(Get-Random)"
New-Item -ItemType Directory -Force $cfg | Out-Null
[IO.File]::WriteAllText((Join-Path $cfg 'settings.json'), '{"update_check":false,"view":{"reply_open":false,"hide_from_capture":false}}', (New-Object Text.UTF8Encoding($false)))
$env:OWNVOICE_CONFIG_DIR = $cfg; $env:OWNVOICE_NO_DEV_KEYS = '1'
$p = Start-Process $Exe -PassThru

function Win($title, $timeout = 15) {
  $c = New-Object System.Windows.Automation.AndCondition(
    (New-Object System.Windows.Automation.PropertyCondition($UIA::ProcessIdProperty, $p.Id)),
    (New-Object System.Windows.Automation.PropertyCondition($UIA::NameProperty, $title)))
  $end = (Get-Date).AddSeconds($timeout)
  while ((Get-Date) -lt $end) { $w = $UIA::RootElement.FindFirst($Scope::Children, $c); if ($w) { return $w }; Start-Sleep -Milliseconds 300 }
}
function OpenSettings($main) {
  $end = (Get-Date).AddSeconds(10)
  while ((Get-Date) -lt $end) {
    foreach ($b in $main.FindAll($Scope::Descendants, (New-Object System.Windows.Automation.PropertyCondition($UIA::ControlTypeProperty, $CT::Button)))) {
      if ($b.Current.Name -like '*⚙*' -or $b.Current.Name -eq '設定') { $b.GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle(); return (Win 'OwnVoice Meet · Settings') }
    }
    Start-Sleep -Milliseconds 300
  }
  throw 'settings button not found'
}
function EditNamed($w, $like) {
  $end = (Get-Date).AddSeconds(10)
  while ((Get-Date) -lt $end) {
    foreach ($e in $w.FindAll($Scope::Descendants, (New-Object System.Windows.Automation.PropertyCondition($UIA::ControlTypeProperty, $CT::Edit)))) {
      if ($e.Current.Name -like $like) { return $e }
    }
    Start-Sleep -Milliseconds 300
  }
  throw "field '$like' not found"
}
function Enter-Text($e, $text) {
  # Through UI Automation, not SendKeys: keystrokes go to whichever window has the focus.
  $e.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue($text)
  Start-Sleep -Milliseconds 300
}
function Value($e) { $e.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).Current.Value }

try {
  $main = Win 'OwnVoice Meet'; if (-not $main) { throw 'main window not found' }
  $set = OpenSettings $main
  Start-Sleep -Seconds 2
  $gl = EditNamed $set '*中文 = English*'; $dir = EditNamed $set '存檔資料夾*'
  Enter-Text $gl 'autosave check = autosave check'
  Enter-Text $dir 'D:/autosave-check'
  "typed: glossary '$(Value $gl)', save folder '$(Value $dir)'"
  Start-Sleep -Seconds 2
  "save bar says: " + (($set.FindAll($Scope::Descendants, (New-Object System.Windows.Automation.PropertyCondition($UIA::ControlTypeProperty, $CT::Text))) | ForEach-Object { $_.Current.Name } | Where-Object { $_ -match '儲存' }) -join ' | ')
  $set.GetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern).Close()
  Start-Sleep -Seconds 2
  $set = OpenSettings $main
  Start-Sleep -Seconds 2
  # A filled field's name can take in its text, so look the fields up by what they hold.
  $edits = $set.FindAll($Scope::Descendants, (New-Object System.Windows.Automation.PropertyCondition($UIA::ControlTypeProperty, $CT::Edit)))
  "after reopening, fields holding the typed text: " + (($edits | ForEach-Object { Value $_ } | Where-Object { $_ -like '*autosave-check*' -or $_ -like 'autosave check*' }) -join ' | ')
} finally {
  Get-Process -Id $p.Id -ErrorAction SilentlyContinue | Stop-Process -Force -Confirm:$false
}
$json = Get-Content (Join-Path $cfg 'settings.json') -Raw -Encoding UTF8 | ConvertFrom-Json
"settings.json: glossary='$($json.glossary)' archive_dir='$($json.archive_dir)'"
Remove-Item -Recurse -Force $cfg
