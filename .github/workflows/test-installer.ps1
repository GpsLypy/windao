# Exercise the real NSIS script in a disposable GitHub-hosted Windows profile.
$ErrorActionPreference = "Stop"
if ($env:GITHUB_ACTIONS -ne "true" -or $env:WINDAO_RUNNER_KIND -ne "github-hosted") {
    throw "Run on an isolated GitHub-hosted Windows runner."
}
$stage = New-Item -ItemType Directory -Path (Join-Path $env:RUNNER_TEMP ([Guid]::NewGuid().ToString()))
$install = Join-Path $stage.FullName "installed app"
$setup = Join-Path $stage.FullName "setup.exe"
$uninstallKey = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\WinDao"
$runKey = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Run"
$desktopLink = Join-Path ([Environment]::GetFolderPath('DesktopDirectory')) "NotchCalendar.lnk"
$startLink = Join-Path ([Environment]::GetFolderPath('Programs')) "NotchCalendar.lnk"
$login = { [Microsoft.Win32.Registry]::GetValue("HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Run", "WinDao", $null) }
if ((Test-Path $uninstallKey) -or (Test-Path $desktopLink) -or (Test-Path $startLink) -or (& $login)) {
    throw "Test requires a clean runner profile."
}
function Run-Setup([string]$exe, [string]$arguments) {
    $child = Start-Process -FilePath $exe -ArgumentList $arguments -PassThru
    if (!$child.WaitForExit(90000)) {
        $child.Kill()
        throw "Installer did not exit without interaction"
    }
    if ($child.ExitCode -ne 0) { throw "Installer failed with exit code $($child.ExitCode)" }
}
try {
    $nsis = "${env:ProgramFiles(x86)}\NSIS\makensis.exe"
    if (!(Test-Path $nsis)) {
        choco install nsis -y --no-progress
        if ($LASTEXITCODE -ne 0) { throw "Unable to install NSIS." }
    }
    # A GUI-subsystem fixture leaves only a marker when the update restarts it.
    $source = Join-Path $stage.FullName "Smoke.cs"
    $fixture = Join-Path $stage.FullName "Smoke.exe"
    'class Smoke { static void Main() { System.IO.File.WriteAllText(System.IO.Path.Combine(System.AppDomain.CurrentDomain.BaseDirectory,"restart.ok"), "ok"); } }' |
        Set-Content -LiteralPath $source
    & "$env:WINDIR\Microsoft.NET\Framework64\v4.0.30319\csc.exe" /nologo /target:winexe /platform:x64 "/out:$fixture" $source
    if ($LASTEXITCODE -ne 0) { throw "Unable to build test fixture." }
    $build = 'from package import build_installer; from pathlib import Path; import sys; build_installer(Path(sys.argv[1]), Path(sys.argv[2]), sys.argv[3], sys.argv[4])'
    python -c $build $fixture $setup 0.1.3 $nsis
    if ($LASTEXITCODE -ne 0) { throw "Unable to build initial installer." }
    Run-Setup $setup "/S /D=$install"
    $exe = Join-Path $install "NotchCalendar.exe"
    $shell = New-Object -ComObject WScript.Shell
    if (!(Test-Path $exe) -or $shell.CreateShortcut($desktopLink).TargetPath -ne $exe -or
        $shell.CreateShortcut($startLink).TargetPath -ne $exe -or (& $login)) {
        throw "Unexpected first-install settings."
    }
    if (!(Test-Path $runKey)) { New-Item -Path $runKey | Out-Null }
    New-ItemProperty -Path $runKey -Name WinDao -Value ('"' + $exe + '"') -Force | Out-Null
    Remove-Item -LiteralPath $desktopLink
    $sentinel = Join-Path $install "unrelated.txt"
    Set-Content -LiteralPath $sentinel -Value "keep"
    python -c $build $fixture $setup 0.1.4 $nsis
    if ($LASTEXITCODE -ne 0) { throw "Unable to build upgrade installer." }
    Run-Setup $setup "/UPDATE"
    $deadline = [DateTime]::UtcNow.AddSeconds(15)
    while (!(Test-Path "$install/restart.ok") -and [DateTime]::UtcNow -lt $deadline) { Start-Sleep -Milliseconds 100 }
    $registered = Get-ItemProperty -LiteralPath $uninstallKey
    if ($registered.InstallLocation -ne $install -or $registered.DisplayVersion -ne "0.1.4" -or
        (Test-Path $desktopLink) -or (& $login) -ne ('"' + $exe + '"') -or !(Test-Path "$install/restart.ok")) {
        throw "Upgrade lost settings, location or restart."
    }
    # Simulate a user repointing this name to another application.
    $foreign = $shell.CreateShortcut($desktopLink)
    $foreign.TargetPath = $env:ComSpec
    $foreign.Save()
    $uninstaller = Join-Path $stage.FullName "uninstall-test.exe"
    Copy-Item -LiteralPath "$install/Uninstall.exe" -Destination $uninstaller
    Run-Setup $uninstaller "/S _?=$install"
    if ((Test-Path $exe) -or (Test-Path $uninstallKey) -or (& $login) -or
        (Test-Path $startLink) -or !(Test-Path $sentinel) -or
        $shell.CreateShortcut($desktopLink).TargetPath -ne $env:ComSpec) {
        throw "Uninstall did not respect application ownership."
    }
} finally {
    foreach ($link in @($desktopLink, $startLink)) {
        if (Test-Path $link) { Remove-Item -LiteralPath $link }
    }
    if (Test-Path $uninstallKey) { Remove-Item -LiteralPath $uninstallKey }
    if (& $login) { Remove-ItemProperty -LiteralPath $runKey -Name WinDao }
    Remove-Item -LiteralPath $stage.FullName -Recurse -Force
}
