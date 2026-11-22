# macOS signing for OwnVoice Meet releases, done from this Windows PC (no Mac needed).
#
#   powershell -ExecutionPolicy Bypass -File tools\macos-signing.ps1
#
# A Developer ID Application certificate signs any app of its team, so this reuses the one made for
# Basesmall (its tools\macos-signing.ps1 keeps it in %USERPROFILE%\.basesmall-signing). It puts the
# certificate, its password, the signing identity and the team id into this repository's secrets,
# then asks for your Apple ID and an app-specific password, which gh reads without echoing them.
# The release workflow then signs and notarizes the Mac app.
#
# Run it yourself. It handles your certificate and passwords, and prints none of them.

param([string]$Dir = (Join-Path $env:USERPROFILE '.basesmall-signing'))
$ErrorActionPreference = 'Stop'

$repo = 'Zaious/ownvoice-meet'
$openssl = (Get-Command openssl -ErrorAction SilentlyContinue).Source
if (-not $openssl) { $openssl = Join-Path $env:ProgramFiles 'Git\mingw64\bin\openssl.exe' }
if (-not (Test-Path -LiteralPath $openssl)) { throw 'openssl not found (it comes with Git for Windows).' }

$p12 = Get-ChildItem -LiteralPath $Dir -Filter '*.p12' -ErrorAction SilentlyContinue | Sort-Object LastWriteTime -Descending | Select-Object -First 1
$passFile = Join-Path $Dir 'p12-password.txt'
$pem = Join-Path $Dir 'developer-id.pem'
if (-not $p12 -or -not (Test-Path -LiteralPath $passFile) -or -not (Test-Path -LiteralPath $pem)) {
  throw "No Developer ID certificate in $Dir (needs the .p12, p12-password.txt and developer-id.pem)."
}

$subject = & $openssl x509 -in $pem -noout -subject -nameopt 'RFC2253,-esc_msb'
$identity = if ($subject -match '(?:^|,)CN=([^,]+)') { $Matches[1] } else { '' }
$team = if ($subject -match '(?:^|,)OU=([^,]+)') { $Matches[1] } else { '' }
if ($identity -notlike 'Developer ID Application:*' -or -not $team) { throw "Not a Developer ID Application certificate: $subject" }
$end = & $openssl x509 -in $pem -noout -enddate
Write-Host "Certificate: $identity ($end)"

# Piped, so the values never appear on a command line; gh drops the trailing newline.
function Secret([string]$name, [string]$value) {
  $value | gh secret set $name --repo $repo
  if ($LASTEXITCODE -ne 0) { throw "gh secret set $name failed (is gh logged in?)" }
}
Secret 'APPLE_CERTIFICATE' ([Convert]::ToBase64String([IO.File]::ReadAllBytes($p12.FullName)))
Secret 'APPLE_CERTIFICATE_PASSWORD' (Get-Content -LiteralPath $passFile -Raw).Trim()
Secret 'APPLE_SIGNING_IDENTITY' $identity
Secret 'APPLE_TEAM_ID' $team

Write-Host ''
Write-Host 'Now two things only you know. gh asks for each and does not show what you type or paste.'
Write-Host '1. Your Apple ID: the email you sign in to developer.apple.com with.'
gh secret set APPLE_ID --repo $repo
Write-Host '2. An app-specific password: account.apple.com > Sign-In and Security > App-Specific Passwords > +'
Write-Host '   (the one made for Basesmall works too, if you kept it).'
gh secret set APPLE_PASSWORD --repo $repo

Write-Host ''
gh secret list --repo $repo
Write-Host 'Done. The next release signs and notarizes the Mac app.'
