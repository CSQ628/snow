#requires -Version 7.2
[CmdletBinding()]
param(
    [ValidateRange(100, 5000)][int]$DurationMilliseconds = 360,
    [ValidateRange(10, 1000)][int]$WarmupMilliseconds = 60,
    [string]$Rates = "120,144,165,240",
    [string]$InputRates = "1000,8000",
    [ValidateSet("all", "steady", "jitter", "stall", "idle", "mixed")][string]$Scenario = "all",
    [string]$BaselineDirectory = "",
    [string]$OutputDirectory = "",
    [switch]$Magnifier,
    [switch]$SkipBuild,
    [ValidateSet("x64", "arm64")][string]$Architecture = "x64"
)

$ErrorActionPreference = "Stop"
$workspace = (Resolve-Path (Join-Path $PSScriptRoot "../..")).Path
. (Join-Path $PSScriptRoot "performance-environment.ps1")
$target = Get-SnowPerformanceTarget -Architecture $Architecture -RequireNative
Initialize-SnowPerformanceEnvironment -Architecture $Architecture
if ([string]::IsNullOrWhiteSpace($OutputDirectory)) {
    $OutputDirectory = Join-Path $workspace "build/global-mouse-presentation-perf/$(Get-Date -Format yyyyMMdd-HHmmss)"
}
$OutputDirectory = [IO.Path]::GetFullPath($OutputDirectory)
New-Item -ItemType Directory -Path $OutputDirectory -Force | Out-Null
$targets = @("snow-shot-global-mouse-presentation-benchmark")
if (![string]::IsNullOrWhiteSpace($BaselineDirectory)) {
    $BaselineDirectory = (Resolve-Path -LiteralPath $BaselineDirectory).Path
    $targets += "snow-shot-global-mouse-presentation-baseline-benchmark"
}

Push-Location $workspace
try {
    if (!$SkipBuild) {
        $configureArguments = @("--preset", $target.Preset)
        if ($BaselineDirectory) {
            $configureArguments += "-DSNOW_SHOT_GLOBAL_MOUSE_PRESENTATION_BASELINE_DIR=$BaselineDirectory"
        }
        & cmake @configureArguments
        if ($LASTEXITCODE -ne 0) { throw "Performance configuration failed" }
        & cmake --build (Join-Path $workspace "build/$($target.Preset)") --config Release `
            --target @targets --parallel
        if ($LASTEXITCODE -ne 0) { throw "Global mouse presentation benchmark build failed" }
    }
    $qtRuntime = Set-SnowPerformanceQtRuntime -Platform offscreen -Architecture $Architecture
    try {
        foreach ($benchmarkTarget in $targets) {
            $executable = Join-Path $workspace "build/$($target.Preset)/snow_shot/test-bin/Release/$benchmarkTarget.exe"
            Assert-SnowPerformanceExecutable -Path $executable -Architecture $Architecture
            $reportName = if ($benchmarkTarget -match "baseline") { "baseline.json" } else { "current.json" }
            $arguments = @("--duration-ms", "$DurationMilliseconds", "--warmup-ms", "$WarmupMilliseconds",
                "--rates", $Rates, "--input-rates", $InputRates, "--scenario", $Scenario,
                "--output", (Join-Path $OutputDirectory $reportName))
            if ($Magnifier) { $arguments += "--magnifier" }
            & $executable @arguments
            if ($LASTEXITCODE -ne 0) { throw "Global mouse presentation benchmark failed: $benchmarkTarget" }
        }
    }
    finally { Restore-SnowPerformanceQtRuntime -Snapshot $qtRuntime }
    Write-Host "Global mouse presentation reports: $OutputDirectory"
    Write-Host "Cadence is deterministic virtual replay; work timings measure active Release calls."
}
finally { Pop-Location }
