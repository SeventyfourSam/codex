# Run the actual embedded updater in child shells to check execution and exits.
$ErrorActionPreference = 'Stop'
$commandLine = @(Get-Content "$PSScriptRoot/../../codex-rs/tui/src/custom_updates.rs" | Where-Object { $_ -match 'irm .* \| iex' })
if ($commandLine.Count -ne 1) { throw 'Could not locate the CLI/TUI installer command.' }
$command = $commandLine[0].Trim().TrimEnd(',') | ConvertFrom-Json
$daemonSource = Get-Content "$PSScriptRoot/../../codex-rs/app-server-daemon/src/update_loop.rs" -Raw
$daemonMatch = [regex]::Match($daemonSource, '"-Command", ("(?:[^"\\]|\\.)*")')
if (-not $daemonMatch.Success) { throw 'Could not locate the daemon installer command.' }
$daemonCommand = $daemonMatch.Groups[1].Value | ConvertFrom-Json
$shell = (Get-Process -Id $PID).Path
foreach ($case in @(
    @{Body="`"Write-Output 'installer-ran'`""; Status=0; Output='installer-ran'},
    @{Body="throw 'download-failed'"; Status=1; Output='download-failed'},
    @{Body="`"throw 'installer-failed'`""; Status=1; Output='installer-failed'},
    @{Command=$daemonCommand; Input="Write-Output 'daemon-installer-ran'"; Status=0; Output='daemon-installer-ran'},
    @{Command=$daemonCommand; Input="throw 'daemon-installer-failed'"; Status=1; Output='daemon-installer-failed'}
)) {
    $fixture = if ($case.Command) { $case.Command } else { "function Invoke-RestMethod { $($case.Body) }`n$command" }
    # Match Rust's process launch: bypass PowerShell's special XML remoting between shells.
    $process = [Diagnostics.Process]::new()
    $process.StartInfo.FileName = $shell
    $process.StartInfo.Arguments = '-NoProfile -ExecutionPolicy Bypass -Command "' + $fixture.Replace('"', '\"') + '"'
    $process.StartInfo.UseShellExecute = $false
    $process.StartInfo.RedirectStandardInput = $true
    $process.StartInfo.RedirectStandardOutput = $true
    $process.StartInfo.RedirectStandardError = $true
    try {
        if (-not $process.Start()) { throw 'Could not start updater shell.' }
        $process.StandardInput.Write([string]$case.Input)
        $process.StandardInput.Close()
        if (-not $process.WaitForExit(30000)) { $process.Kill(); throw 'Updater fixture timed out.' }
        $output = $process.StandardOutput.ReadToEnd() + $process.StandardError.ReadToEnd()
        $status = $process.ExitCode
    } finally { $process.Dispose() }
    if ($status -ne $case.Status -or ($output | Out-String) -notmatch $case.Output) {
        throw "Unexpected updater result for $($case.Output): exit=$status, output=$output"
    }
}
Write-Host 'Passed 5 CLI/TUI and daemon updater command cases.'
