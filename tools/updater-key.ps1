# The signing key for OwnVoice Meet's in-app updates (the Tauri updater), made on this PC.
#
#   powershell -ExecutionPolicy Bypass -File tools\updater-key.ps1
#
# 1. Makes a key pair in %USERPROFILE%\.ownvoice-meet-signing. Tauri asks for a password twice (hidden).
# 2. Puts the private key into the GitHub repository's secrets (TAURI_SIGNING_PRIVATE_KEY), straight
#    from the file, and asks for the same password once more (hidden) for
#    TAURI_SIGNING_PRIVATE_KEY_PASSWORD.
# 3. Writes the public key (not secret) into src-tauri/tauri.updater.conf.json, for you to commit.
#
# Run it yourself. It handles your private key and password, and prints neither.
# Back up updater.key and the password together, somewhere other than this PC. Without them no
# installed copy can be updated again: everyone would have to download and install by hand.
# Needs: Node.js (npx tauri), and the GitHub CLI signed in (gh auth login) with access to the repo.

$ErrorActionPreference = 'Stop'
$repo = 'Zaious/ownvoice-meet'
$dir = Join-Path $env:USERPROFILE '.ownvoice-meet-signing'
$key = Join-Path $dir 'updater.key'
$root = Resolve-Path (Join-Path $PSScriptRoot '..')
$conf = Join-Path $root 'src-tauri\tauri.updater.conf.json'
New-Item -ItemType Directory -Force $dir | Out-Null
if (Test-Path -LiteralPath $key) { throw "An updater key is already in $dir. Using it again is fine; to start over, move it away first." }

Push-Location $root
try {
  Write-Host 'Choose a password for the update key (asked twice, hidden):'
  npx tauri signer generate -w $key
  if ($LASTEXITCODE -ne 0 -or -not (Test-Path -LiteralPath "$key.pub")) { throw 'tauri signer generate failed' }
} finally { Pop-Location }

# The key file is plain base64 text; gh reads it from the pipe and strips the final newline.
Get-Content -Raw -LiteralPath $key | gh secret set TAURI_SIGNING_PRIVATE_KEY --repo $repo
if ($LASTEXITCODE -ne 0) { throw 'gh secret set TAURI_SIGNING_PRIVATE_KEY failed' }
Write-Host ''
Write-Host 'Now the same password once more, for the release workflow (hidden):'
gh secret set TAURI_SIGNING_PRIVATE_KEY_PASSWORD --repo $repo
if ($LASTEXITCODE -ne 0) { throw 'gh secret set TAURI_SIGNING_PRIVATE_KEY_PASSWORD failed' }

# The public key goes where the release build reads it; the release workflow refuses to sign while
# the placeholder is still there.
$pub = (Get-Content -Raw -LiteralPath "$key.pub").Trim()
$text = [IO.File]::ReadAllText($conf)
if (-not $text.Contains('REPLACE_WITH_UPDATER_PUBLIC_KEY')) { throw "$conf has no placeholder; put the public key in by hand: $key.pub" }
[IO.File]::WriteAllText($conf, $text.Replace('REPLACE_WITH_UPDATER_PUBLIC_KEY', $pub), (New-Object Text.UTF8Encoding $false))

Write-Host ''
Write-Host "Done. Private key: $key (back it up with the password)."
Write-Host "Public key written into src-tauri\tauri.updater.conf.json: commit that file."
gh secret list --repo $repo
