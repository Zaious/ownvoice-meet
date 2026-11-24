# End-to-end check of the speaker models' first download: a fresh config folder, Settings opened,
# 「標出講者」 ticked; waits for the download (checked against its digests in speakers.rs) and lists
# the files that arrived.
#   powershell -ExecutionPolicy Bypass -File scripts\e2e-speaker-models.ps1 -Exe src-tauri\target\debug\ownvoice-meet.exe
# -Local: the local recognition models instead (about 910 MB): picks 本機辨識 and presses its download button.
param([string]$Exe, [switch]$Local)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
$UIA = [System.Windows.Automation.AutomationElement]; $Scope = [System.Windows.Automation.TreeScope]; $CT = [System.Windows.Automation.ControlType]

$cfg = Join-Path ([IO.Path]::GetTempPath()) "ownvoice-spkmodels-$(Get-Random)"
New-Item -ItemType Directory -Force $cfg | Out-Null
# With -Local, 本機辨識 is already chosen, so Settings opens with its download button showing.
$json = if ($Local) { '{"stt_id":"sherpa-local","update_check":false,"view":{"reply_open":false}}' } else { '{"update_check":false,"view":{"reply_open":false}}' }
[IO.File]::WriteAllText((Join-Path $cfg 'settings.json'), $json, (New-Object Text.UTF8Encoding($false)))
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
  if ($Local) {
    $btn = $set.FindAll($Scope::Descendants, (New-Object System.Windows.Automation.PropertyCondition($UIA::ControlTypeProperty, $CT::Button))) | Where-Object { $_.Current.Name -like '下載本機辨識模型*' } | Select-Object -First 1
    $btn.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
    $want = '模型已在這台電腦上|沒有下載完成'; $limit = 900
  } else {
    $box = $set.FindAll($Scope::Descendants, (New-Object System.Windows.Automation.PropertyCondition($UIA::ControlTypeProperty, $CT::CheckBox))) | Where-Object { $_.Current.Name -like '*標出講者*' } | Select-Object -First 1
    $box.GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()
    $want = '講者模型已下載|沒有下載完成'; $limit = 120
  }
  $t0 = Get-Date; $state = ''
  while (((Get-Date) - $t0).TotalSeconds -lt $limit) {
    $state = (Texts $set | Where-Object { $_ -match "$want|下載中" }) -join ' | '
    if ($state -match $want) { break }
    Start-Sleep -Seconds 2
  }
  "state after $([int]((Get-Date) - $t0).TotalSeconds) s: $state"
  if (-not $Local) { "box ticked: $($box.GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Current.ToggleState)" }
} finally {
  Get-Process -Id $p.Id -ErrorAction SilentlyContinue | Stop-Process -Force -Confirm:$false
}
Get-ChildItem -Recurse -File (Join-Path $cfg 'models') -ErrorAction SilentlyContinue | ForEach-Object { "$($_.Directory.Name)/$($_.Name) $($_.Length)" }
"settings.json speaker_labels: " + ((Get-Content (Join-Path $cfg 'settings.json') -Raw | ConvertFrom-Json).speaker_labels)
Remove-Item -Recurse -Force $cfg
