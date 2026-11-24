# End-to-end check of the speaker models' first download: a fresh config folder, Settings opened,
# 「標出講者」 ticked; waits for the download (checked against its digests in speakers.rs) and lists
# the files that arrived.
#   powershell -ExecutionPolicy Bypass -File scripts\e2e-speaker-models.ps1 -Exe src-tauri\target\debug\ownvoice-meet.exe
param([string]$Exe)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
$UIA = [System.Windows.Automation.AutomationElement]; $Scope = [System.Windows.Automation.TreeScope]; $CT = [System.Windows.Automation.ControlType]

$cfg = Join-Path ([IO.Path]::GetTempPath()) "ownvoice-spkmodels-$(Get-Random)"
New-Item -ItemType Directory -Force $cfg | Out-Null
[IO.File]::WriteAllText((Join-Path $cfg 'settings.json'), '{"update_check":false,"view":{"reply_open":false}}', (New-Object Text.UTF8Encoding($false)))
$env:OWNVOICE_CONFIG_DIR = $cfg; $env:OWNVOICE_NO_DEV_KEYS = '1'
$p = Start-Process $Exe -PassThru

function Win($title, $timeout = 15) {
  $c = New-Object System.Windows.Automation.AndCondition(
    (New-Object System.Windows.Automation.PropertyCondition($UIA::ProcessIdProperty, $p.Id)),
    (New-Object System.Windows.Automation.PropertyCondition($UIA::NameProperty, $title)))
  $end = (Get-Date).AddSeconds($timeout)
  while ((Get-Date) -lt $end) { $w = $UIA::RootElement.FindFirst($Scope::Children, $c); if ($w) { return $w }; Start-Sleep -Milliseconds 300 }
}
function Texts($w) { ($w.FindAll($Scope::Descendants, (New-Object System.Windows.Automation.PropertyCondition($UIA::ControlTypeProperty, $CT::Text))) | ForEach-Object { $_.Current.Name } | Where-Object { $_ }) }

try {
  $main = Win 'OwnVoice Meet'
  $set = $null; $end = (Get-Date).AddSeconds(15)
  while (-not $set -and (Get-Date) -lt $end) {
    foreach ($b in $main.FindAll($Scope::Descendants, (New-Object System.Windows.Automation.PropertyCondition($UIA::ControlTypeProperty, $CT::Button)))) {
      if ($b.Current.Name -eq '⚙') { $b.GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle(); break }
    }
    $set = Win 'OwnVoice Meet · Settings' 3
  }
  Start-Sleep -Seconds 2
  $box = $set.FindAll($Scope::Descendants, (New-Object System.Windows.Automation.PropertyCondition($UIA::ControlTypeProperty, $CT::CheckBox))) | Where-Object { $_.Current.Name -like '*標出講者*' } | Select-Object -First 1
  $box.GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()
  $t0 = Get-Date; $state = ''
  while (((Get-Date) - $t0).TotalSeconds -lt 120) {
    $state = (Texts $set | Where-Object { $_ -match '講者模型|沒有下載完成' }) -join ' | '
    if ($state -match '已下載|沒有下載完成') { break }
    Start-Sleep -Seconds 1
  }
  "state after $([int]((Get-Date) - $t0).TotalSeconds) s: $state"
  "box ticked: $($box.GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Current.ToggleState)"
} finally {
  Get-Process -Id $p.Id -ErrorAction SilentlyContinue | Stop-Process -Force -Confirm:$false
}
Get-ChildItem (Join-Path $cfg 'models') -ErrorAction SilentlyContinue | ForEach-Object { "$($_.Name) $($_.Length)" }
"settings.json speaker_labels: " + ((Get-Content (Join-Path $cfg 'settings.json') -Raw | ConvertFrom-Json).speaker_labels)
Remove-Item -Recurse -Force $cfg
