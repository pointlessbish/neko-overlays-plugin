# Usage: ./scripts/obs-sdk.ps1 [-Version 32.2.1]
# Run it in a Visual Studio Developer PowerShell: it fetches the OBS headers and obs.lib into .obs-sdk/.

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

Write-Host "Getting the OBS $Version headers"
Run git @("clone", "--quiet", "--depth", "1", "--branch", $Version, "--filter=blob:none", "--sparse", "https://github.com/obsproject/obs-studio.git", $source)
Run git @("-C", $source, "sparse-checkout", "set", "libobs")
if (-not (Test-Path (Join-Path $source "libobs/obs.h"))) { Fail "obs.h is not in the OBS source" }

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
