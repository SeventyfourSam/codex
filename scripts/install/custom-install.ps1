# GitHub-only installer for Windows x64; never overwrite a running executable.
param([string]$Release = $env:CODEX_RELEASE)
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$repo = 'https://github.com/SeventyfourSam/codex'
# Windows PowerShell 5.1 may expose RuntimeInformation without OSArchitecture.
# Under WOW64, PROCESSOR_ARCHITEW6432 identifies the native OS architecture.
$architecture = if ($env:PROCESSOR_ARCHITEW6432) { $env:PROCESSOR_ARCHITEW6432 } else { $env:PROCESSOR_ARCHITECTURE }
if ($env:OS -ne 'Windows_NT' -or $architecture -ne 'AMD64') { throw "Only Windows x64 is supported (OS: $env:OS; architecture: $architecture)." }
if ([string]::IsNullOrEmpty($Release)) { $Release = 'latest' }
if ($Release -eq 'latest') {
    $response = Invoke-WebRequest -UseBasicParsing -Method Head -Uri "$repo/releases/latest" -TimeoutSec 30
    $uri = if ($response.BaseResponse.ResponseUri) { $response.BaseResponse.ResponseUri } else { $response.BaseResponse.RequestMessage.RequestUri }
    $prefix = "$repo/releases/tag/v"
    if (-not $uri.AbsoluteUri.StartsWith($prefix, [StringComparison]::Ordinal)) { throw 'Invalid latest release redirect.' }
    $version = $uri.AbsoluteUri.Substring($prefix.Length)
} else { $version = $Release -creplace '^v', '' }
if ($version -cnotmatch '^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)-custom\.(0|[1-9][0-9]*)$') { throw 'Expected x.y.z-custom.N.' }
$base = $version -replace '-custom\..*$', ''
$name = "$version-x86_64-pc-windows-msvc"
$asset = "codex-$name.zip"
$homeDir = if ($env:CODEX_HOME) { $env:CODEX_HOME } else { Join-Path $HOME '.codex' }
$daemonOnly = $env:CODEX_INSTALL_DAEMON_ONLY -eq '1'
$root = Join-Path $homeDir $(if ($daemonOnly) { 'packages\app-server-daemon' } else { 'packages\standalone' })
New-Item -ItemType Directory -Force (Join-Path $root 'releases') | Out-Null
$root = (Resolve-Path $root).Path
$current = Join-Path $root 'current'
if ($env:CODEX_INSTALL_DEFER_SELECTION -eq '1') {
    if (-not $daemonOnly -or (Test-Path $current)) { throw 'Cannot defer this selection.' }
    $current = Join-Path $root '.migration-current'
}
function Set-Link([string]$Link, [string]$Target) {
    $existing = Get-Item -LiteralPath $Link -Force -ErrorAction SilentlyContinue
    if ($existing -and $existing.LinkType -ne 'Junction') { throw "Refusing to replace unmanaged directory: $Link" }
    $pending = "$Link.pending.$PID"
    $backup = "$Link.previous.$PID"
    New-Item -ItemType Junction -Path $pending -Target $Target | Out-Null
    try {
        if ($existing) { [System.IO.Directory]::Move($Link, $backup) }
        try { [System.IO.Directory]::Move($pending, $Link) }
        catch { if ($existing) { [System.IO.Directory]::Move($backup, $Link) }; throw }
        if ($existing) { [System.IO.Directory]::Delete($backup) }
    } finally { if (Test-Path -LiteralPath $pending) { [System.IO.Directory]::Delete($pending) } }
}
function Test-Package([string]$Directory) {
    foreach ($file in @('codex-package.json', 'bin\codex.exe', 'bin\codex-code-mode-host.exe', 'codex-path\rg.exe', 'codex-resources\codex-command-runner.exe', 'codex-resources\codex-windows-sandbox-setup.exe')) {
        if (-not (Test-Path -LiteralPath (Join-Path $Directory $file) -PathType Leaf)) { return $false }
    }
    $binary = Join-Path $Directory 'bin\codex.exe'
    $upstream = & $binary --version
    if ($LASTEXITCODE -ne 0 -or $upstream -cne "codex-cli $base") { return $false }
    $custom = & $binary --custom
    return $LASTEXITCODE -eq 0 -and $custom -ceq "codex-cli $version"
}
$lock = $null
$deadline = [DateTime]::UtcNow.AddSeconds(60)
while ($null -eq $lock) {
    try { $lock = [System.IO.File]::Open((Join-Path $root 'install.lock'), 'OpenOrCreate', 'ReadWrite', 'None') }
    catch [System.IO.IOException] { if ([DateTime]::UtcNow -ge $deadline) { throw }; Start-Sleep -Milliseconds 250 }
}
$stage = Join-Path $root "releases\.custom-stage.$([Guid]::NewGuid())"
try {
    if ($env:CODEX_INSTALL_DEFER_SELECTION -eq '1' -and (Test-Path (Join-Path $root 'current'))) { throw 'Daemon selection changed; retry.' }
    if ($env:CODEX_INSTALL_IF_LATEST -eq '1' -or $env:CODEX_INSTALL_IF_CURRENT -eq '1') {
        $selected = Get-Item -LiteralPath $current -Force
        $expected = Join-Path $root "releases\$env:CODEX_UPDATE_FROM_RELEASE"
        if (-not $env:CODEX_UPDATE_FROM_RELEASE -or $selected.LinkType -ne 'Junction' -or $selected.Target -ne $expected) { throw 'Daemon selection changed; retry.' }
        if ($env:CODEX_INSTALL_IF_LATEST -eq '1' -and (Get-Content (Join-Path $root 'auto-update-version') -Raw) -cne $env:CODEX_UPDATE_FROM_RELEASE) { return }
    }
    $destination = Join-Path $root "releases\$name"
    if (-not (Test-Package $destination)) {
        if (Test-Path -LiteralPath $destination) { throw "Incomplete release exists: $destination; remove it manually before retrying." }
        New-Item -ItemType Directory $stage | Out-Null
        $sums = Join-Path $stage 'SHA256SUMS'
        Invoke-WebRequest -UseBasicParsing -Uri "$repo/releases/download/v$version/SHA256SUMS" -OutFile $sums -TimeoutSec 30
        $pattern = '^([a-fA-F0-9]{64})  ' + [regex]::Escape($asset) + '$'
        $digests = @(Get-Content $sums | Where-Object { $_ -cmatch $pattern } | ForEach-Object { [regex]::Match($_, $pattern).Groups[1].Value })
        if ($digests.Count -ne 1) { throw 'Missing or ambiguous package checksum.' }
        $archive = Join-Path $stage $asset
        Invoke-WebRequest -UseBasicParsing -Uri "$repo/releases/download/v$version/$asset" -OutFile $archive -TimeoutSec 300
        if ((Get-FileHash $archive -Algorithm SHA256).Hash -ine $digests[0]) { throw 'Package SHA-256 mismatch.' }
        $package = Join-Path $stage 'package'
        Expand-Archive -LiteralPath $archive -DestinationPath $package
        if (-not (Test-Package $package)) { throw 'Package validation failed.' }
        if ($daemonOnly) {
            & (Join-Path $package 'bin\codex.exe') app-server daemon pid-update-loop --check-package-ownership
            if ($LASTEXITCODE -ne 0) { throw 'Package does not support daemon ownership.' }
        }
        Move-Item -LiteralPath $package -Destination $destination
    }
    Set-Link $current $destination
    $marker = Join-Path $root 'auto-update-version'
    if ($Release -eq 'latest') { [System.IO.File]::WriteAllText("$marker.tmp.$PID", $name); Move-Item "$marker.tmp.$PID" $marker -Force }
    elseif (Test-Path $marker) { Remove-Item $marker }
    if ($daemonOnly) { return }
    $bin = if ($env:CODEX_INSTALL_DIR) { $env:CODEX_INSTALL_DIR } else { Join-Path $env:LOCALAPPDATA 'Programs\CodexCustom\bin' }
    New-Item -ItemType Directory -Force (Split-Path $bin -Parent) | Out-Null
    Set-Link $bin (Join-Path $current 'bin')
    $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
    $rest = @($userPath -split ';' | Where-Object { $_ -and $_ -ine $bin }) -join ';'
    [Environment]::SetEnvironmentVariable('Path', "$bin;$rest", 'User')
    Write-Host "Installed $version. Start a new terminal, or run: & '$bin\codex.exe'"
} finally {
    if (Test-Path -LiteralPath $stage) { Remove-Item -LiteralPath $stage -Recurse -Force }
    $lock.Dispose()
}
