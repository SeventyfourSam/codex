# Run on the Windows release runner against the package it just built, without network.
param([Parameter(Mandatory)][string]$Archive, [Parameter(Mandatory)][string]$Version)
$ErrorActionPreference = 'Stop'
$fixtureArchive = (Resolve-Path $Archive).Path
$fixtureName = Split-Path $fixtureArchive -Leaf
$fixtureChecksum = (Get-FileHash $fixtureArchive -Algorithm SHA256).Hash.ToLowerInvariant()
$fixtureDownloads = [System.Collections.Generic.List[string]]::new()
$fixtureBadChecksum = $false
function Invoke-WebRequest([string]$Uri, [string]$OutFile, [switch]$UseBasicParsing, [int]$TimeoutSec, [string]$Method) {
    if ($Method -eq 'Head') {
        if ($Uri -cne 'https://github.com/SeventyfourSam/codex/releases/latest') { throw "Unexpected metadata URL: $Uri" }
        return @{BaseResponse=@{ResponseUri=[uri]"https://github.com/SeventyfourSam/codex/releases/tag/v$Version"}}
    }
    $fixtureDownloads.Add($Uri)
    $prefix = "https://github.com/SeventyfourSam/codex/releases/download/v$Version/"
    if ($Uri -ceq "${prefix}SHA256SUMS") {
        $digest = if ($fixtureBadChecksum) { '0' * 64 } else { $fixtureChecksum }
        [System.IO.File]::WriteAllText($OutFile, "$digest  $fixtureName`n")
    } elseif ($Uri -ceq "$prefix$fixtureName") { Copy-Item $fixtureArchive $OutFile }
    else { throw "Unexpected download URL: $Uri" }
}
$temp = Join-Path ([System.IO.Path]::GetTempPath()) "custom-install-test-$([Guid]::NewGuid())"
$originalPath = [Environment]::GetEnvironmentVariable('Path', 'User')
$originalHome = $env:CODEX_HOME
$originalInstall = $env:CODEX_INSTALL_DIR
$originalRelease = $env:CODEX_RELEASE
try {
    $env:CODEX_HOME = Join-Path $temp 'home'
    $env:CODEX_INSTALL_DIR = Join-Path $temp 'visible\bin'
    $env:CODEX_RELEASE = 'latest'
    & "$PSScriptRoot/custom-install.ps1"
    & "$PSScriptRoot/custom-install.ps1"
    if ($fixtureDownloads.Count -ne 2) { throw 'Repeated install downloaded the package again.' }
    $binary = Join-Path $env:CODEX_INSTALL_DIR 'codex.exe'
    if ((& $binary --custom) -cne "codex-cli $Version") { throw 'Wrong installed custom version.' }
    if ((& $binary --version) -cne "codex-cli $($Version -replace '-custom(?:\..*)?$', '')") { throw 'Upstream version changed.' }
    $configPath = Join-Path $env:CODEX_HOME 'config.toml'
    $config = Get-Content -Raw $configPath
    if ($config -notmatch '(?s)\[feedback\]\s+enabled = false') { throw 'Custom configuration was not initialized.' }
    $current = Join-Path $env:CODEX_HOME 'packages\standalone\current'
    $selected = (Get-Item $current).Target
    [System.IO.File]::WriteAllText($configPath, 'invalid = [')
    $failed = $false
    try { & "$PSScriptRoot/custom-install.ps1" } catch { if ($_ -notmatch 'Could not initialize custom settings') { throw }; $failed = $true }
    if (-not $failed -or (Get-Item $current).Target -ne $selected) { throw 'Invalid configuration changed the active package.' }
    if ((Get-Content -Raw $configPath) -cne 'invalid = [') { throw 'Invalid configuration was overwritten.' }
    $env:CODEX_HOME = Join-Path $temp 'failed-home'
    $root = Join-Path $env:CODEX_HOME 'packages\standalone'
    $old = Join-Path $root 'releases\old'
    New-Item -ItemType Directory -Force $old | Out-Null
    $current = Join-Path $root 'current'
    New-Item -ItemType Junction -Path $current -Target $old | Out-Null
    $fixtureBadChecksum = $true
    $failed = $false
    try { & "$PSScriptRoot/custom-install.ps1" } catch { if ($_ -notmatch 'SHA-256 mismatch') { throw }; $failed = $true }
    if (-not $failed -or (Get-Item $current).Target -ne $old) { throw 'Invalid package replaced the current release.' }
} finally {
    [Environment]::SetEnvironmentVariable('Path', $originalPath, 'User')
    $env:CODEX_HOME = $originalHome
    $env:CODEX_INSTALL_DIR = $originalInstall
    $env:CODEX_RELEASE = $originalRelease
    if (Test-Path $temp) { Remove-Item $temp -Recurse -Force }
}
