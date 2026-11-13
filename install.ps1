#Requires -Version 5.1
<#
.SYNOPSIS
  Install a SONOLYTICA release binary for Windows 10 or 11 (x64).

.DESCRIPTION
  Downloads the release zip, verifies its SHA-256, and installs
  sonolytica.exe and sonolytica-eval.exe. Re-run to upgrade.

  Environment:
    SONOLYTICA_REPO     GitHub owner/name (default: neksand/sonolytica)
    SONOLYTICA_VERSION  tag to install (default: latest release)
    SONOLYTICA_BIN_DIR  install directory (default: %LOCALAPPDATA%\sonolytica\bin)
#>
[CmdletBinding()]
param(
    [string]$Repo = $(if ($env:SONOLYTICA_REPO) { $env:SONOLYTICA_REPO } else { "neksand/sonolytica" }),
    [string]$Version = $(if ($env:SONOLYTICA_VERSION) { $env:SONOLYTICA_VERSION } else { "latest" }),
    [string]$BinDir = $(if ($env:SONOLYTICA_BIN_DIR) { $env:SONOLYTICA_BIN_DIR } else { Join-Path $env:LOCALAPPDATA "sonolytica\bin" })
)

$ErrorActionPreference = "Stop"

if ([Environment]::OSVersion.Version.Major -lt 10) {
    throw "SONOLYTICA requires Windows 10 or 11 (x64); this Windows version is too old."
}
if (-not [Environment]::Is64BitOperatingSystem) {
    throw "SONOLYTICA requires 64-bit Windows 10 or 11."
}

$target = "x86_64-pc-windows-msvc"
if ($Version -eq "latest") {
    $base = "https://github.com/$Repo/releases/latest/download"
} else {
    $base = "https://github.com/$Repo/releases/download/$Version"
}
$archive = "sonolytica-$target.zip"

$tmp = Join-Path ([IO.Path]::GetTempPath()) ("sonolytica-" + [guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Path $tmp -Force | Out-Null
try {
    $zip = Join-Path $tmp $archive
    $sum = "$zip.sha256"

    Write-Host "downloading $archive from $Repo"
    Invoke-WebRequest -UseBasicParsing -Uri "$base/$archive" -OutFile $zip
    Invoke-WebRequest -UseBasicParsing -Uri "$base/$archive.sha256" -OutFile $sum

    $expected = ((Get-Content $sum -Raw) -split '\s+')[0].ToLower()
    $actual = (Get-FileHash $zip -Algorithm SHA256).Hash.ToLower()
    if ($expected -ne $actual) {
        throw "checksum mismatch: expected $expected, got $actual"
    }

    Expand-Archive -Path $zip -DestinationPath $tmp -Force
    New-Item -ItemType Directory -Path $BinDir -Force | Out-Null
    Copy-Item (Join-Path $tmp "sonolytica.exe") $BinDir -Force
    Copy-Item (Join-Path $tmp "sonolytica-eval.exe") $BinDir -Force

    Write-Host "installed to $BinDir"
    if (($env:Path -split ';') -notcontains $BinDir) {
        Write-Host "add to PATH: [Environment]::SetEnvironmentVariable('Path', `"$BinDir;`$env:Path`", 'User')"
    }
    Write-Host "next: sonolytica setup"
} finally {
    Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
}
