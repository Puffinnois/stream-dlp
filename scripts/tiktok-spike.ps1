#Requires -Version 7.0
# Anonymous, metadata-only TikTok feasibility probe. Never loads browser cookies.
[CmdletBinding()]
param(
    [Parameter(Mandatory)][string[]]$YtDlp,
    [string]$OutputDirectory = 'test-output/tiktok-spike/results',
    [string[]]$CaseName = @(),
    [ValidateRange(5, 150)][int]$TimeoutSeconds = 40
)
$ErrorActionPreference = 'Stop'
New-Item -ItemType Directory -Force $OutputDirectory | Out-Null
# Public examples from yt-dlp's TikTok extractor tests. Availability can change.
$cases = @(
    @{ name = 'desktop'; url = 'https://www.tiktok.com/@patroxofficial/video/6742501081818877190?langCountry=en' },
    @{ name = 'second-video'; url = 'https://www.tiktok.com/@hankgreen1/video/7047596209028074758' },
    @{ name = 'share'; url = 'https://www.tiktok.com/share/video/7668090902816017671/' },
    @{ name = 'short-www'; url = 'https://www.tiktok.com/t/ZTRC5xgJp' },
    @{ name = 'short-vm'; url = 'https://vm.tiktok.com/ZTR45GpSF/' },
    @{ name = 'short-vt'; url = 'https://vt.tiktok.com/ZSe4FqkKd' },
    # Synthetic variants of a known video: these do not assert the target exists.
    @{ name = 'mobile-variant'; url = 'https://m.tiktok.com/v/6742501081818877190.html' },
    @{ name = 'photo-variant'; url = 'https://www.tiktok.com/@patroxofficial/photo/6742501081818877190' },
    @{ name = 'invalid-id'; url = 'https://www.tiktok.com/@_/video/0' }
)
if ($CaseName.Count) { $cases = @($cases | Where-Object { $_.name -in $CaseName }) }
if (-not $cases.Count) { throw 'No matching probe cases' }
$results = @()
foreach ($helper in $YtDlp) {
    $executable = (Resolve-Path -LiteralPath $helper).Path
    $version = (& $executable --ignore-config --version | Out-String).Trim()
    if ($LASTEXITCODE -ne 0) { throw "Cannot read helper version: $executable" }
    foreach ($case in $cases) {
        $start = [System.Diagnostics.ProcessStartInfo]::new()
        $start.FileName = $executable
        $start.UseShellExecute = $false
        $start.CreateNoWindow = $true
        $start.RedirectStandardOutput = $true
        $start.RedirectStandardError = $true
        foreach ($argument in @('--ignore-config', '--no-playlist', '--no-colors', '--encoding', 'utf-8', '--socket-timeout', '10', '--retries', '0', '--extractor-retries', '0', '--skip-download', '--dump-single-json', '--', $case.url)) {
            $start.ArgumentList.Add($argument)
        }
        $process = [System.Diagnostics.Process]::new()
        $process.StartInfo = $start
        $null = $process.Start()
        $stdout = $process.StandardOutput.ReadToEndAsync()
        $stderr = $process.StandardError.ReadToEndAsync()
        $timedOut = -not $process.WaitForExit($TimeoutSeconds * 1000)
        if ($timedOut) { $process.Kill($true); $process.WaitForExit() }
        $output = $stdout.GetAwaiter().GetResult()
        $errors = $stderr.GetAwaiter().GetResult()
        $row = [ordered]@{
            observed_at_utc = [DateTime]::UtcNow.ToString('o')
            version = $version; case = $case.name; url = $case.url
            exit_code = $process.ExitCode; timed_out = $timedOut
            # Do not retain signed media URLs, headers, cookies, or full metadata.
            error = (($errors -replace 'https?://\S+', '[URL]').Trim())
        }
        if (-not $timedOut -and $process.ExitCode -eq 0) {
            $info = $output | ConvertFrom-Json
            $row.metadata = [ordered]@{
                id = $info.id; extractor = $info.extractor_key; type = $info._type
                has_title = -not [string]::IsNullOrWhiteSpace($info.title)
                has_uploader = -not [string]::IsNullOrWhiteSpace($info.uploader)
                duration = $info.duration; is_live = $info.is_live
                has_is_live_field = 'is_live' -in $info.PSObject.Properties.Name
                formats = @($info.formats | Select-Object format_id,ext,width,height,vcodec,acodec)
                entry_count = @($info.entries | Where-Object { $null -ne $_ }).Count
            }
        }
        $results += [pscustomobject]$row
        $results | ConvertTo-Json -Depth 8 | Set-Content -Encoding utf8 (Join-Path $OutputDirectory 'summary.json')
        Write-Host "$version $($case.name): exit=$($row.exit_code), timeout=$timedOut"
        $process.Dispose()
    }
}
