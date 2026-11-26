# Run on Windows with the Windows SDK developer tools in PATH.
# A store certificate thumbprint is optional for producing an unsigned candidate.
[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$Executable,
    [Parameter(Mandatory)][string]$Publisher,
    [string]$CertificateThumbprint
)
$ErrorActionPreference = "Stop"
$root = $PSScriptRoot
$exe = (Resolve-Path -LiteralPath $Executable).Path
$version = [regex]::Match((Get-Content "$root/Cargo.toml" -Raw), '(?m)^version = "(\d+\.\d+\.\d+)"').Groups[1].Value
if (-not $version) { throw "Missing app version." }
$stage = New-Item -ItemType Directory -Path (Join-Path ([System.IO.Path]::GetTempPath()) ([Guid]::NewGuid().ToString()))
try {
    Copy-Item -LiteralPath $exe -Destination "$stage/NotchCalendar.exe"
    Copy-Item -LiteralPath "$root/LICENSE","$root/THIRD_PARTY_NOTICES.txt","$root/assets/fonts/OFL-NotoSansSC.txt" -Destination $stage
    $assets = New-Item -ItemType Directory -Path "$stage/Assets"
    Add-Type -AssemblyName System.Drawing
    $icon = [System.Drawing.Image]::FromFile("$root/assets/icon.png")
    try {
        foreach ($n in 44,50,150) {
            $image = [System.Drawing.Bitmap]::new($icon,$n,$n)
            try { $image.Save("$assets/Logo$n.png",[System.Drawing.Imaging.ImageFormat]::Png) } finally { $image.Dispose() }
        }
    } finally { $icon.Dispose() }
    $escapedPublisher = [System.Security.SecurityElement]::Escape($Publisher)
    $manifest = (Get-Content "$root/assets/msix.xml" -Raw).Replace("PUBLISHER_SUBJECT",$escapedPublisher).Replace("PACKAGE_VERSION","$version.0")
    [System.IO.File]::WriteAllText("$stage/AppxManifest.xml",$manifest,[System.Text.UTF8Encoding]::new($false))
    New-Item -ItemType Directory -Force -Path "$root/release" | Out-Null
    $candidate = "$root/release/NotchCalendar-Windows-$version-x64.msix"
    & makeappx.exe pack /d $stage.FullName /p $candidate /o
    if ($LASTEXITCODE -ne 0) { throw "MakeAppx failed." }
    if ($CertificateThumbprint) {
        & signtool.exe sign /sha1 $CertificateThumbprint /fd SHA256 $candidate
        if ($LASTEXITCODE -ne 0) { throw "SignTool failed. The publisher must match the signing certificate." }
        & signtool.exe verify /pa $candidate
        if ($LASTEXITCODE -ne 0) { throw "Package signature verification failed." }
    }
    Get-FileHash -Algorithm SHA256 $candidate
} finally { Remove-Item -LiteralPath $stage.FullName -Recurse -Force }
