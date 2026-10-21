param([switch]$SkipTools)
$ErrorActionPreference = 'Stop'
Push-Location $PSScriptRoot
try {
    cargo test --locked
    if ($LASTEXITCODE -ne 0) { throw 'Rust tests failed.' }
    cargo build --release --locked
    if ($LASTEXITCODE -ne 0) { throw 'Release build failed.' }
    $portable = Join-Path $PSScriptRoot 'dist\stream-dlp'
    New-Item -ItemType Directory -Path $portable -Force | Out-Null
    Copy-Item -LiteralPath 'target\release\stream-dlp.exe' -Destination (Join-Path $portable 'stream-dlp.exe') -Force
    foreach ($file in @('Setup tools.ps1', 'README.md', 'LICENSE', 'THIRD-PARTY.md')) { Copy-Item -LiteralPath $file -Destination $portable -Force }
    if (-not $SkipTools) { & '.\Setup tools.ps1' -Destination (Join-Path $portable 'tools') }
    # The distributable installs third-party tools directly from their official releases.
    $zip = Join-Path $PSScriptRoot 'dist\stream-dlp-Windows-x64.zip'
    Compress-Archive -Path (Join-Path $portable 'stream-dlp.exe'),(Join-Path $portable 'Setup tools.ps1'),(Join-Path $portable 'README.md'),(Join-Path $portable 'LICENSE'),(Join-Path $portable 'THIRD-PARTY.md') -DestinationPath $zip -Force
    Write-Host "Portable app: $portable"
    Write-Host "App + setup zip: $zip"
} finally { Pop-Location }
