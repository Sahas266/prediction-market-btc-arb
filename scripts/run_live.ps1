param(
    [int]$DurationSeconds = 300,
    [int]$PollIntervalMs = 1000,
    [switch]$Release
)

$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent $PSScriptRoot
Set-Location $repoRoot

$logDir = Join-Path $repoRoot "data\runs\logs"
New-Item -ItemType Directory -Force -Path $logDir | Out-Null

$stamp = Get-Date -Format "yyyyMMdd_HHmmss"
$logPath = Join-Path $logDir "live_$stamp.log"
$stdoutPath = Join-Path $logDir "live_$stamp.stdout.log"
$stderrPath = Join-Path $logDir "live_$stamp.stderr.log"

$cargoArgs = @("run")
if ($Release) {
    $cargoArgs += "--release"
}
$cargoArgs += @("--", "live", "--duration-seconds", $DurationSeconds, "--poll-interval-ms", $PollIntervalMs)
$cargoCommand = "cargo " + ($cargoArgs -join " ")

"[$(Get-Date -Format o)] Starting live run" | Tee-Object -FilePath $logPath
"Command: $cargoCommand" | Tee-Object -FilePath $logPath -Append
"Stdout: $stdoutPath" | Tee-Object -FilePath $logPath -Append
"Stderr: $stderrPath" | Tee-Object -FilePath $logPath -Append

$process = Start-Process `
    -FilePath "cargo" `
    -ArgumentList $cargoArgs `
    -WorkingDirectory $repoRoot `
    -Wait `
    -PassThru `
    -NoNewWindow `
    -RedirectStandardOutput $stdoutPath `
    -RedirectStandardError $stderrPath

if ($process.ExitCode -ne 0) {
    throw "cargo run failed with exit code $($process.ExitCode)"
}

"[$(Get-Date -Format o)] Finished live run" | Tee-Object -FilePath $logPath -Append
"Log file: $logPath" | Tee-Object -FilePath $logPath -Append
