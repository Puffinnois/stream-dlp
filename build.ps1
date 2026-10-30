param([switch]$SkipTools, [switch]$SkipTests, [switch]$Installer, [string]$IsccPath, [string]$PortableDirectory)
$ErrorActionPreference = 'Stop'
Push-Location $PSScriptRoot
try {
    $manifest = Get-Content -LiteralPath 'Cargo.toml' -Raw
    if ($manifest -notmatch '(?m)^version\s*=\s*"([^"]+)"') { throw 'Cannot find app version in Cargo.toml.' }
    $appVersion = $Matches[1]
    if ($appVersion -notmatch '^\d+\.\d+\.\d+$') { throw "Releases use X.Y.Z versions, not $appVersion." }
    $releaseOutput = Join-Path $PSScriptRoot 'dist'
    if (-not $PortableDirectory) { $PortableDirectory = 'dist\stream-dlp' }
    New-Item -ItemType Directory -Path $releaseOutput -Force | Out-Null
    # CI runs `cargo xtask check` first and passes -SkipTests.
    if (-not $SkipTests) {
        cargo test --locked
        if ($LASTEXITCODE -ne 0) { throw 'Rust tests failed.' }
    }
    cargo build --release --locked
    if ($LASTEXITCODE -ne 0) { throw 'Release build failed.' }
    $portable = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot $PortableDirectory))
    New-Item -ItemType Directory -Path $portable -Force | Out-Null
    Copy-Item -LiteralPath 'target\release\stream-dlp.exe' -Destination (Join-Path $portable 'stream-dlp.exe') -Force
    foreach ($file in @('Setup tools.ps1', 'README.md', 'LICENSE', 'THIRD-PARTY.md')) { Copy-Item -LiteralPath $file -Destination $portable -Force }
    $docs = Join-Path $portable 'docs'
    if (Test-Path -LiteralPath $docs) { Remove-Item -LiteralPath $docs -Recurse -Force }
    New-Item -ItemType Directory -Path $docs -Force | Out-Null
    # The user documents sit directly in docs/ and ship; its subfolders are for development.
    Get-ChildItem -LiteralPath 'docs' -Filter '*.md' -File | Copy-Item -Destination $docs -Force
    if (-not $SkipTools) { & '.\Setup tools.ps1' -Destination (Join-Path $portable 'tools') }
    # The distributable installs third-party tools directly from their official releases.
    $zip = Join-Path $releaseOutput 'stream-dlp-Windows-x64.zip'
    Compress-Archive -Path (Join-Path $portable 'stream-dlp.exe'),(Join-Path $portable 'Setup tools.ps1'),(Join-Path $portable 'README.md'),(Join-Path $portable 'LICENSE'),(Join-Path $portable 'THIRD-PARTY.md'),$docs -DestinationPath $zip -Force
    Write-Host "Portable app: $portable"
    Write-Host "App + setup zip: $zip"
    $assets = @($zip)
    $setup = Join-Path $releaseOutput 'stream-dlp-Setup-Windows-x64.exe'
    $update = Join-Path $releaseOutput 'stream-dlp-Update-Windows-x64.exe'
    if ($Installer) {
        foreach ($helper in @('yt-dlp.exe', 'ffmpeg.exe', 'ffprobe.exe', 'deno.exe', 'versions.json')) {
            if (-not (Test-Path -LiteralPath (Join-Path $portable "tools\$helper"))) {
                throw "Installer requires tools\$helper. Run build.ps1 without -SkipTools first."
            }
        }
        if (-not $IsccPath) {
            $compiler = Get-Command ISCC.exe -ErrorAction SilentlyContinue
            if ($compiler) { $IsccPath = $compiler.Source }
            else {
                # tools\inno-setup in this checkout, else a standard Inno Setup 6 or 7 installation.
                $IsccPath = @('tools\inno-setup\ISCC.exe', "${env:ProgramFiles(x86)}\Inno Setup *\ISCC.exe", "$env:ProgramFiles\Inno Setup *\ISCC.exe") |
                    ForEach-Object { Get-ChildItem -Path $_ -ErrorAction SilentlyContinue } |
                    Select-Object -First 1 -ExpandProperty FullName
            }
        }
        if (-not $IsccPath -or -not (Test-Path -LiteralPath $IsccPath)) { throw 'Install Inno Setup or pass -IsccPath to its ISCC.exe.' }
        $defines = @("/DAppVersion=$appVersion", "/DNumericVersion=$appVersion.0", "/DReleaseOutput=$releaseOutput", "/DPortableDir=$portable")
        & $IsccPath @defines 'installer\stream-dlp.iss'
        if ($LASTEXITCODE -ne 0) { throw 'Windows installer compilation failed.' }
        # The slim update installer: the same script without the tools, for in-app updates.
        & $IsccPath @defines '/DUpdateOnly' 'installer\stream-dlp.iss'
        if ($LASTEXITCODE -ne 0) { throw 'Update installer compilation failed.' }
        Write-Host "Windows installer: $setup"
        Write-Host "Update installer: $update"
        $assets = @($setup, $update) + $assets
    }
    # sha256sum format (lowercase hash, two spaces, name, LF) for the release's SHA256SUMS.txt.
    $sums = Join-Path $releaseOutput 'SHA256SUMS.txt'
    $lines = foreach ($asset in $assets) {
        '{0}  {1}' -f (Get-FileHash -LiteralPath $asset -Algorithm SHA256).Hash.ToLowerInvariant(), (Split-Path -Leaf $asset)
    }
    [IO.File]::WriteAllText($sums, (($lines -join "`n") + "`n"))
    Write-Host "Checksums: $sums"
    if ($env:GITHUB_OUTPUT) {
        # Paths for the GitHub workflows (package check, release).
        Add-Content -LiteralPath $env:GITHUB_OUTPUT -Encoding utf8 -Value @(
            "version=$appVersion", "portable=$portable", "zip=$zip", "sums=$sums",
            "installer=$(if ($Installer) { $setup })", "update=$(if ($Installer) { $update })"
        )
    }
} finally { Pop-Location }
