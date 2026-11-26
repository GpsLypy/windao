# Run on Windows with the Windows SDK developer tools in PATH.
# A store certificate thumbprint is optional for producing an unsigned candidate.
[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$Executable,
    [Parameter(Mandatory)][string]$Publisher,
    [string]$CertificateThumbprint,
    [switch]$UnsignedTest
)
$ErrorActionPreference = "Stop"
$testOid = 'OID.2.25.311729368913984317654407730594956997722=1'
if ($UnsignedTest -and $CertificateThumbprint) { throw "UnsignedTest and CertificateThumbprint are mutually exclusive." }
if ($Publisher -match '2\.25\.311729368913984317654407730594956997722') { throw "Pass the publisher subject without the test OID; use -UnsignedTest for a Windows 11 test package." }
if ($UnsignedTest) { $Publisher = "$Publisher, $testOid" }
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
    $suffix = if ($UnsignedTest) { "-UnsignedTest" } else { "" }
    $candidate = "$root/release/NotchCalendar-Windows-$version-x64$suffix.msix"
    & makeappx.exe pack /d $stage.FullName /p $candidate /o
    if ($LASTEXITCODE -ne 0) { throw "MakeAppx failed." }
    if ($CertificateThumbprint) {
        & signtool.exe sign /sha1 $CertificateThumbprint /fd SHA256 $candidate
        if ($LASTEXITCODE -ne 0) { throw "SignTool failed. The publisher must match the signing certificate." }
        & signtool.exe verify /pa $candidate
        if ($LASTEXITCODE -ne 0) { throw "Package signature verification failed." }
    }
    if ($UnsignedTest) {
        Add-Type -AssemblyName System.IO.Compression.FileSystem
        $archive = [System.IO.Compression.ZipFile]::OpenRead($candidate)
        try {
            if ($archive.GetEntry("AppxSignature.p7x")) { throw "Unexpected signature on test package." }
            $reader = [System.IO.StreamReader]::new($archive.GetEntry("AppxManifest.xml").Open())
            try { [xml]$built = $reader.ReadToEnd() } finally { $reader.Dispose() }
            if ($built.Package.Identity.Publisher -ne $Publisher) { throw "Unexpected test package identity." }
        } finally { $archive.Dispose() }
        $fileName = [System.IO.Path]::GetFileName($candidate)
        $instructions = @"
Windows 11 x64 test package

This MSIX is unsigned. Windows 11 permits testing this executable package from an
administrator PowerShell. The special publisher OID gives it a distinct test identity.
The install affects the machine. Use a test PC or disposable Windows 11 VM.

Extract both files into the same directory. From that directory, in administrator PowerShell:

    Add-AppxPackage -Path ".\$fileName" -AllowUnsigned

Open NotchCalendar, then 工作台 → 通知 → 连接 QQ 系统通知 to request notification access.

Microsoft's test installation instructions:
https://learn.microsoft.com/windows/msix/package/unsigned-package
"@
        [System.IO.File]::WriteAllText("$root/release/Windows-11-UnsignedTest.txt",$instructions,[System.Text.UTF8Encoding]::new($false))
    }
    Get-FileHash -Algorithm SHA256 $candidate
} finally { Remove-Item -LiteralPath $stage.FullName -Recurse -Force }
