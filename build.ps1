param([switch]$SkipTools, [switch]$Installer, [string]$IsccPath, [string]$PortableDirectory = 'dist\stream-dlp')
$ErrorActionPreference = 'Stop'
Push-Location $PSScriptRoot
try {
    cargo test --locked
    if ($LASTEXITCODE -ne 0) { throw 'Rust tests failed.' }
    cargo build --release --locked
    if ($LASTEXITCODE -ne 0) { throw 'Release build failed.' }
    $portable = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot $PortableDirectory))
    New-Item -ItemType Directory -Path $portable -Force | Out-Null
    Copy-Item -LiteralPath 'target\release\stream-dlp.exe' -Destination (Join-Path $portable 'stream-dlp.exe') -Force
    foreach ($file in @('Setup tools.ps1', 'README.md', 'LICENSE', 'THIRD-PARTY.md')) { Copy-Item -LiteralPath $file -Destination $portable -Force }
    $docs = Join-Path $portable 'docs'
    New-Item -ItemType Directory -Path $docs -Force | Out-Null
    foreach ($document in @('browser-session-spike.md', 'chrome-auto-mode-options.md', 'streamable-feasibility.md', 'release-0.3.4.md')) {
        Copy-Item -LiteralPath (Join-Path 'docs' $document) -Destination $docs -Force
    }
    if (-not $SkipTools) { & '.\Setup tools.ps1' -Destination (Join-Path $portable 'tools') }
    # The distributable installs third-party tools directly from their official releases.
    $zip = Join-Path $PSScriptRoot 'dist\stream-dlp-Windows-x64.zip'
    Compress-Archive -Path (Join-Path $portable 'stream-dlp.exe'),(Join-Path $portable 'Setup tools.ps1'),(Join-Path $portable 'README.md'),(Join-Path $portable 'LICENSE'),(Join-Path $portable 'THIRD-PARTY.md'),$docs -DestinationPath $zip -Force
    Write-Host "Portable app: $portable"
    Write-Host "App + setup zip: $zip"
    if ($Installer) {
        foreach ($helper in @('yt-dlp.exe', 'ffmpeg.exe', 'ffprobe.exe', 'deno.exe', 'versions.json')) {
            if (-not (Test-Path -LiteralPath (Join-Path $portable "tools\$helper"))) {
                throw "Installer requires tools\$helper. Run build.ps1 without -SkipTools first."
            }
        }
        if (-not $IsccPath) {
            $compiler = Get-Command ISCC.exe -ErrorAction SilentlyContinue
            if ($compiler) { $IsccPath = $compiler.Source }
            else { $IsccPath = Join-Path ${env:ProgramFiles(x86)} 'Inno Setup 6\ISCC.exe' }
        }
        if (-not (Test-Path -LiteralPath $IsccPath)) { throw 'Install Inno Setup or pass -IsccPath to its ISCC.exe.' }
        $manifest = Get-Content -LiteralPath 'Cargo.toml' -Raw
        if ($manifest -notmatch '(?m)^version\s*=\s*"([^"]+)"') { throw 'Cannot find app version in Cargo.toml.' }
        & $IsccPath "/DAppVersion=$($Matches[1])" "/DPortableDir=$portable" 'installer\stream-dlp.iss'
        if ($LASTEXITCODE -ne 0) { throw 'Windows installer compilation failed.' }
        Write-Host "Windows installer: $(Join-Path $PSScriptRoot 'dist\stream-dlp-Setup-Windows-x64.exe')"
    }
} finally { Pop-Location }
