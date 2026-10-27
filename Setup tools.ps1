param([string]$Destination = (Join-Path $PSScriptRoot 'tools'))
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
$headers = @{ 'User-Agent' = 'stream-dlp-Setup'; 'Accept' = 'application/vnd.github+json' }
# CI passes its token so shared runners stay under the API rate limit; it is sent only to the API.
if ($env:GITHUB_TOKEN) { $headers['Authorization'] = "Bearer $env:GITHUB_TOKEN" }
$staging = Join-Path ([IO.Path]::GetTempPath()) ('stream-dlp-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $staging -Force | Out-Null
New-Item -ItemType Directory -Path $Destination -Force | Out-Null

function Get-ReleaseAsset([string]$Repository, [string]$AssetName) {
    Write-Host "Getting $AssetName from $Repository..."
    $release = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repository/releases/latest" -Headers $headers
    $asset = $release.assets | Where-Object { $_.name -eq $AssetName } | Select-Object -First 1
    if (-not $asset) { throw "The official release has no $AssetName asset." }
    if ($asset.digest -notmatch '^sha256:([a-fA-F0-9]{64})$') { throw "GitHub did not supply a SHA-256 digest for $AssetName. Refusing an unverified download." }
    $expected = $Matches[1]
    $target = Join-Path $staging $AssetName
    Invoke-WebRequest -Uri $asset.browser_download_url -OutFile $target -UseBasicParsing
    $actual = (Get-FileHash -LiteralPath $target -Algorithm SHA256).Hash
    if ($actual -ne $expected) { throw "SHA-256 verification failed for $AssetName." }
    [pscustomobject]@{ Repository = $Repository; Version = $release.tag_name; File = $target; Sha256 = $actual; Url = $asset.browser_download_url; Source = $release.html_url }
}

try {
    $yt = Get-ReleaseAsset 'yt-dlp/yt-dlp' 'yt-dlp.exe'
    $ff = Get-ReleaseAsset 'yt-dlp/FFmpeg-Builds' 'ffmpeg-master-latest-win64-gpl.zip'
    $deno = Get-ReleaseAsset 'denoland/deno' 'deno-x86_64-pc-windows-msvc.zip'
    Expand-Archive -LiteralPath $ff.File -DestinationPath (Join-Path $staging 'ffmpeg')
    Expand-Archive -LiteralPath $deno.File -DestinationPath (Join-Path $staging 'deno')
    Copy-Item -LiteralPath $yt.File -Destination (Join-Path $Destination 'yt-dlp.exe') -Force
    foreach ($name in @('ffmpeg.exe', 'ffprobe.exe')) {
        $binary = Get-ChildItem -LiteralPath (Join-Path $staging 'ffmpeg') -Recurse -Filter $name | Select-Object -First 1
        if (-not $binary) { throw "FFmpeg archive is missing $name." }
        Copy-Item -LiteralPath $binary.FullName -Destination (Join-Path $Destination $name) -Force
    }
    Copy-Item -LiteralPath (Join-Path $staging 'deno\deno.exe') -Destination (Join-Path $Destination 'deno.exe') -Force
    $notices = Join-Path $Destination 'licenses'
    New-Item -ItemType Directory -Path $notices -Force | Out-Null
    Get-ChildItem -LiteralPath (Join-Path $staging 'ffmpeg') -Recurse -File | Where-Object { $_.Name -match '^(LICENSE|COPYING|README)' } | ForEach-Object { Copy-Item -LiteralPath $_.FullName -Destination (Join-Path $notices ('ffmpeg-' + $_.Name)) -Force }
    Invoke-WebRequest -Uri 'https://raw.githubusercontent.com/yt-dlp/yt-dlp/master/LICENSE' -OutFile (Join-Path $notices 'yt-dlp-LICENSE.txt') -UseBasicParsing
    Invoke-WebRequest -Uri 'https://raw.githubusercontent.com/yt-dlp/yt-dlp/master/THIRD_PARTY_LICENSES.txt' -OutFile (Join-Path $notices 'yt-dlp-THIRD_PARTY_LICENSES.txt') -UseBasicParsing
    Invoke-WebRequest -Uri 'https://raw.githubusercontent.com/denoland/deno/main/LICENSE.md' -OutFile (Join-Path $notices 'deno-LICENSE.md') -UseBasicParsing
    @($yt, $ff, $deno) | Select-Object Repository,Version,Sha256,Url,Source | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $Destination 'versions.json') -Encoding UTF8
    Write-Host 'All tools are installed and SHA-256 verified. You can open stream-dlp.' -ForegroundColor Green
} finally {
    # Remove only this script's uniquely named temporary directory.
    $tempRoot = [IO.Path]::GetFullPath([IO.Path]::GetTempPath())
    $resolvedStaging = [IO.Path]::GetFullPath($staging)
    if ($resolvedStaging.StartsWith($tempRoot, [StringComparison]::OrdinalIgnoreCase) -and ([IO.Path]::GetFileName($resolvedStaging)).StartsWith('stream-dlp-')) {
        Remove-Item -LiteralPath $resolvedStaging -Recurse -Force -ErrorAction SilentlyContinue
    }
}
