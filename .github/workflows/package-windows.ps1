# Build the two public downloads and exercise the release binary from the portable archive.
$ErrorActionPreference = "Stop"
$nsis = "${env:ProgramFiles(x86)}\NSIS\makensis.exe"
if (!(Test-Path $nsis)) {
    choco install nsis -y --no-progress
    if ($LASTEXITCODE -ne 0) { throw "NSIS installation failed" }
}
python package.py target/release/windao.exe --makensis $nsis
if ($LASTEXITCODE -ne 0) { throw "Windows packaging failed" }
$archive = @(Get-ChildItem release -Filter "*.zip")
if ($archive.Length -ne 1) { throw "Expected one portable release" }
$stage = Join-Path $env:RUNNER_TEMP ([Guid]::NewGuid().ToString())
$unpacked = Join-Path $stage "portable"
try {
    Expand-Archive -Path $archive[0].FullName -DestinationPath $unpacked
    $exe = Join-Path $unpacked "NotchCalendar.exe"
    if ((Get-FileHash $exe).Hash -ne (Get-FileHash target/release/windao.exe).Hash) {
        throw "Portable binary does not match the release build"
    }
    $setup = Get-ChildItem release -Filter "*-Setup.exe"
    python tests/verify-product-icon.py $exe $setup.FullName
    if ($LASTEXITCODE -ne 0) { throw "Packaged product icons do not match." }
    $id = [Guid]::NewGuid().ToString()
    $profile = Join-Path $stage "WinDao"
    New-Item -ItemType Directory -Path $profile | Out-Null
    @{accounts=@(@{id=$id;provider="ClaudeStatus";name="CI";color=0;endpoint=""});pinned=$id} |
        ConvertTo-Json -Depth 5 | Set-Content "$profile/ai-accounts.json" -Encoding utf8NoBOM
    $start = [Diagnostics.ProcessStartInfo]::new($exe)
    $start.EnvironmentVariables['APPDATA'] = $stage
    $start.UseShellExecute = $false
    $start.Arguments = "--claude-statusline $id"
    $start.RedirectStandardInput = $true
    $start.RedirectStandardOutput = $true
    $proc = [Diagnostics.Process]::Start($start)
    $proc.StandardInput.WriteLine('{"rate_limits":{"five_hour":{"used_percentage":12,"resets_at":1893456000}}}')
    $proc.StandardInput.Close()
    if (!$proc.WaitForExit(15000)) { $proc.Kill(); throw "Packaged status line timed out" }
    if ($proc.ExitCode -ne 0 -or $proc.StandardOutput.ReadToEnd() -notmatch '12%') { throw "Packaged status line failed" }
    $reading = Get-Content "$profile/ai-statusline/$id.json" -Raw | ConvertFrom-Json
    if ($reading.rate_limits.five_hour.used_percentage -ne 12) { throw "Invalid usage result" }
    Get-ChildItem release -Include '*.zip','*.exe' -File -Recurse | ForEach-Object {
        $expected = (Get-Content "$($_.FullName).sha256" -Raw).Split(' ')[0]
        if ($expected -ne (Get-FileHash $_.FullName).Hash) { throw "Distribution checksum mismatch" }
        Write-Host "$($_.Name) $($_.Length) bytes SHA-256 $expected"
    }
} finally {
    if (Test-Path $stage) { Remove-Item -LiteralPath $stage -Recurse -Force }
}
