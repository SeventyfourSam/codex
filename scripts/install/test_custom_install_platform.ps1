# Exercise the real installer up to its first network request, without installing.
$ErrorActionPreference = 'Stop'
function Invoke-WebRequest { throw 'Platform accepted; network intercepted.' }
$original = @{}
foreach ($name in @('OS', 'PROCESSOR_ARCHITECTURE', 'PROCESSOR_ARCHITEW6432')) {
    $original[$name] = [Environment]::GetEnvironmentVariable($name, 'Process')
}
try {
    foreach ($case in @(
        @{OS='Windows_NT'; Process='AMD64'; Native=$null; Accepted=$true},
        @{OS='Windows_NT'; Process='x86'; Native='AMD64'; Accepted=$true},
        @{OS='Windows_NT'; Process='x86'; Native=$null; Accepted=$false},
        @{OS='Windows_NT'; Process='ARM64'; Native=$null; Accepted=$false},
        @{OS='Windows_NT'; Process='AMD64'; Native='ARM64'; Accepted=$false},
        @{OS='Windows_NT'; Process=$null; Native=$null; Accepted=$false},
        @{OS=$null; Process='AMD64'; Native=$null; Accepted=$false}
    )) {
        $env:OS = $case.OS
        $env:PROCESSOR_ARCHITECTURE = $case.Process
        $env:PROCESSOR_ARCHITEW6432 = $case.Native
        $message = $null
        try { & "$PSScriptRoot/custom-install.ps1" -Release latest } catch { $message = $_.Exception.Message }
        if ($case.Accepted) {
            if ($message -cne 'Platform accepted; network intercepted.') { throw "Supported platform rejected: $message" }
        } elseif ($message -notlike 'Only Windows x64 is supported*') {
            throw "Unsupported platform was not rejected: $message"
        }
    }
    Write-Host 'Passed 7 installer platform cases.'
} finally {
    foreach ($name in $original.Keys) { [Environment]::SetEnvironmentVariable($name, $original[$name], 'Process') }
}
