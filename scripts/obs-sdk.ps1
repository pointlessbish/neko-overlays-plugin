# Location: neko-overlays-plugin/scripts/obs-sdk.ps1
# File: obs-sdk.ps1
#
# Gets what a Windows build needs from OBS Studio, into .obs-sdk/ (ignored by git):
#   .obs-sdk/obs-studio/libobs   the OBS headers (from the obs-studio source, at the chosen version)
#   .obs-sdk/lib/obs.lib         an import library for obs.dll (made from the obs.dll in OBS's portable zip)
# build.rs finds both by itself. Needs git, and dumpbin.exe + lib.exe: run it in a "x64 Native Tools Command
# Prompt for VS" / "Developer PowerShell for VS". The GitHub workflow runs it too.
#
# Usage: ./scripts/obs-sdk.ps1 [-Version 32.2.1]
#
# The version should be the oldest OBS you want the plugin to run on: a plugin built against newer headers than
# the OBS running it can be refused (as far as I know), while an older one keeps working.

param([string]$Version = "32.2.1")

$ErrorActionPreference = "Stop"

function Fail($message) { throw $message }
function Run($exe, [string[]]$arguments) {
    & $exe @arguments
    if ($LASTEXITCODE -ne 0) { Fail "$exe failed (exit code $LASTEXITCODE)" }
}

$root = Split-Path -Parent $PSScriptRoot
$sdk = Join-Path $root ".obs-sdk"
$source = Join-Path $sdk "obs-studio"
$libDir = Join-Path $sdk "lib"
$stamp = Join-Path $sdk "version.txt"

if ((Test-Path $stamp) -and ((Get-Content $stamp -Raw).Trim() -eq $Version) -and (Test-Path (Join-Path $libDir "obs.lib")) -and (Test-Path (Join-Path $source "libobs/obs.h"))) {
    Write-Host "OBS SDK $Version is already in $sdk"
    return
}

if (-not (Get-Command dumpbin.exe -ErrorAction SilentlyContinue) -or -not (Get-Command lib.exe -ErrorAction SilentlyContinue)) {
    Fail "dumpbin.exe and lib.exe were not found. Run this from a 'Developer PowerShell for VS' (or x64 Native Tools prompt)."
}

if (Test-Path $sdk) { Remove-Item -Recurse -Force $sdk }
New-Item -ItemType Directory -Force -Path $sdk, $libDir | Out-Null

# 1. The headers: only the libobs folder of the OBS source, at the release tag.
Write-Host "Getting the OBS $Version headers"
Run git @("clone", "--quiet", "--depth", "1", "--branch", $Version, "--filter=blob:none", "--sparse", "https://github.com/obsproject/obs-studio.git", $source)
Run git @("-C", $source, "sparse-checkout", "set", "libobs")
if (-not (Test-Path (Join-Path $source "libobs/obs.h"))) { Fail "obs.h is not in the OBS source" }

# 2. obs.dll from OBS's portable zip. Only that one file is taken out of the zip.
$zip = Join-Path $sdk "obs.zip"
$url = "https://github.com/obsproject/obs-studio/releases/download/$Version/OBS-Studio-$Version-Windows-x64.zip"
Write-Host "Downloading $url"
Invoke-WebRequest -Uri $url -OutFile $zip

Add-Type -AssemblyName System.IO.Compression.FileSystem
$dll = Join-Path $sdk "obs.dll"
$archive = [System.IO.Compression.ZipFile]::OpenRead($zip)
try {
    $entry = $archive.Entries | Where-Object { $_.FullName -eq "bin/64bit/obs.dll" } | Select-Object -First 1
    if (-not $entry) { Fail "bin/64bit/obs.dll is not in the OBS zip" }
    [System.IO.Compression.ZipFileExtensions]::ExtractToFile($entry, $dll, $true)
} finally {
    $archive.Dispose()
}
Remove-Item $zip

# 3. The import library: the list of names obs.dll exports, as a .def file, turned into obs.lib.
Write-Host "Making obs.lib"
$names = & dumpbin.exe /nologo /exports $dll | ForEach-Object {
    if ($_ -match '^\s+\d+\s+[0-9A-Fa-f]+\s+[0-9A-Fa-f]{8}\s+(\S+)') { $Matches[1] }
}
if (-not $names -or $names.Count -lt 300) { Fail "could not read the export list of obs.dll" }
$def = Join-Path $sdk "obs.def"
Set-Content -Path $def -Value (@("LIBRARY obs.dll", "EXPORTS") + $names) -Encoding ascii
Run lib.exe @("/nologo", "/def:$def", "/machine:x64", "/out:$(Join-Path $libDir 'obs.lib')")
Remove-Item $dll, $def

Set-Content -Path $stamp -Value $Version
Write-Host "Done: $($names.Count) functions in obs.lib. Now: cargo build --release"
