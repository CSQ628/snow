#requires -Version 7.2
[CmdletBinding()]
param(
    [ValidateRange(1, 100)][int]$Iterations = 8,
    [ValidateRange(80, 2000)][int]$DurationMilliseconds = 240,
    [ValidateRange(0, 1000)][int]$AcquisitionDelayMilliseconds = 20,
    [ValidateRange(1, 10)][int]$Rounds = 2,
    [string]$Sizes = "1080p,4k",
    [string]$Overlays = "1,2",
    [string]$Picker = "off,on",
    [string]$BaselineDirectory = "",
    [string]$OutputDirectory = "",
    [switch]$SkipBuild,
    [ValidateSet("x64", "arm64")][string]$Architecture = "x64"
)

$ErrorActionPreference = "Stop"
$workspace = (Resolve-Path (Join-Path $PSScriptRoot "../..")).Path
. (Join-Path $PSScriptRoot "performance-environment.ps1")
$target = Get-SnowPerformanceTarget -Architecture $Architecture -RequireNative
Initialize-SnowPerformanceEnvironment -Architecture $Architecture
if ([string]::IsNullOrWhiteSpace($OutputDirectory)) {
    $OutputDirectory = Join-Path $workspace "build/external-drag-startup-perf/$(Get-Date -Format yyyyMMdd-HHmmss)"
}
$OutputDirectory = [IO.Path]::GetFullPath($OutputDirectory)
New-Item -ItemType Directory -Path $OutputDirectory -Force | Out-Null
$targets = @("snow-shot-external-drag-startup-benchmark")
if (![string]::IsNullOrWhiteSpace($BaselineDirectory)) {
    $BaselineDirectory = (Resolve-Path -LiteralPath $BaselineDirectory).Path
    $targets += "snow-shot-external-drag-startup-baseline-benchmark"
}
Push-Location $workspace
try {
    if (!$SkipBuild) {
        & cmake --preset $target.Preset "-DSNOW_SHOT_DRAG_STARTUP_BASELINE_DIR=$BaselineDirectory"
        if ($LASTEXITCODE -ne 0) { throw "Startup benchmark configuration failed" }
        & cmake --build (Join-Path $workspace "build/$($target.Preset)") --config Release `
            --target @targets --parallel
        if ($LASTEXITCODE -ne 0) { throw "Startup benchmark build failed" }
    }
    $qtRuntime = Set-SnowPerformanceQtRuntime -Platform windows -Architecture $Architecture
    $reports = @()
    try {
        for ($round = 0; $round -lt $Rounds; ++$round) {
            # Reverse execution order between rounds to reduce temperature/order bias.
            $orderedTargets = if ($round % 2 -eq 0) { $targets } else { @($targets[($targets.Count - 1)..0]) }
            foreach ($benchmarkTarget in $orderedTargets) {
                $executable = Join-Path $workspace "build/$($target.Preset)/snow_shot/test-bin/Release/$benchmarkTarget.exe"
                Assert-SnowPerformanceExecutable -Path $executable -Architecture $Architecture
                $runtime = if ($benchmarkTarget -match "baseline") { "baseline" } else { "optimized" }
                $reportPath = Join-Path $OutputDirectory "$runtime-$round.json"
                & $executable --iterations $Iterations --duration-ms $DurationMilliseconds `
                    --acquire-delay-ms $AcquisitionDelayMilliseconds --sizes $Sizes `
                    --overlays $Overlays --picker $Picker --output $reportPath
                if ($LASTEXITCODE -ne 0) { throw "Startup benchmark failed: $benchmarkTarget" }
                $report = Get-Content -LiteralPath $reportPath -Raw | ConvertFrom-Json
                $reports += $report
            }
        }
    }
    finally { Restore-SnowPerformanceQtRuntime -Snapshot $qtRuntime }

    function Get-Median([object[]]$Values) {
        $sorted = @($Values | Sort-Object)
        $middle = [int][Math]::Floor($sorted.Count / 2)
        if ($sorted.Count % 2 -eq 0) {
            return ([double]$sorted[$middle - 1] + [double]$sorted[$middle]) / 2
        }
        return $sorted[$middle]
    }
    function Get-P95([object[]]$Values) {
        $sorted = @($Values | Sort-Object)
        return $sorted[[int][Math]::Ceiling(($sorted.Count - 1) * 0.95)]
    }
    $metricPaths = [ordered]@{
        startup_to_first_paint_end_ms = "startup_to_first_paint_end_ms"
        reveal_to_first_paint_end_ms = "reveal_to_first_paint_end_ms"
        startup_to_first_frame_dispatch_end_ms = "startup_to_first_frame_dispatch_end_ms"
        reveal_to_first_frame_dispatch_end_ms = "reveal_to_first_frame_dispatch_end_ms"
        first_paint_end_to_frame_dispatch_end_ms = "first_paint_end_to_frame_dispatch_end_ms"
        first_paint_wall_ms = "first_paint_wall_ms"
        first_paint_gui_cpu_ms = "first_paint_gui_cpu_ms"
        worker_ready_after_dispatch_ms = "worker_ready_after_dispatch_ms"
        worker_ready_deadline_slack_ms = "worker_ready_deadline_slack_ms"
        acquisition_delivery_after_worker_ready_ms = "acquisition_delivery_after_worker_ready_ms"
        acquisition_ready_after_dispatch_ms = "acquisition_ready_after_dispatch_ms"
        first_moving_frame_dispatch_end_after_reveal_ms = "first_moving_frame_dispatch_end_after_reveal_ms"
        first_moving_frame_dispatch_end_after_first_source_ms = "first_moving_frame_dispatch_end_after_first_source_ms"
        first_moving_paint_applied_sample_age_ms = "first_moving_paint_applied_sample_age_ms"
        first_moving_paint_source_latest_age_ms = "first_moving_paint_source_latest_age_ms"
        first_moving_paint_pending_source_lag_ms = "first_moving_paint_pending_source_lag_ms"
        activation_hook_after_reveal_ms = "activation_hook_after_reveal_ms"
        activation_hook_wall_ms = "activation_hook.wall_ms"
        max_event_loop_pulse_gap_ms = "max_event_loop_pulse_gap_ms"
        gui_cpu_ms = "gui_cpu_ms"
        initial_state_notifications = "initial_state_notifications"
        window_activation_events = "window_activation_events"
        surface_prepare_wall_ms = "surface_prepare.wall_ms"
        layout_presentation_wall_ms = "layout_presentation.wall_ms"
        post_reveal_selection_wall_ms = "post_reveal_selection.wall_ms"
        post_reveal_lifecycle_wall_ms = "post_reveal_lifecycle.wall_ms"
        surface_warm_dispatch_wall_ms = "surface_warm_dispatch.wall_ms"
        surface_warm_dispatch_gui_cpu_ms = "surface_warm_dispatch.gui_cpu_ms"
        surface_warm_dispatch_max_wall_ms = "surface_warm_dispatch_durations_ms.max"
        surface_warm_dispatches = "surface_warm_dispatches"
        native_surface_warms_before_acquire = "native_surface_warms_before_acquire"
        native_surface_warms_total = "native_surface_warms_total"
        initial_selection_wall_ms = "initial_selection.wall_ms"
        native_update_max_wall_ms = "native_update_wall_ms.max"
        owner_paint_gap_p95_ms = "owner_paint_gap_ms.p95"
        input_source_interval_p95_ms = "input_source_interval_ms.p95"
        input_source_interval_max_ms = "input_source_interval_ms.max"
        source_latest_age_p95_ms = "source_latest_age_ms.p95"
        pending_source_lag_p95_ms = "pending_source_lag_ms.p95"
    }
    $comparison = @()
    $cases = @($reports | ForEach-Object { $_.runs } | ForEach-Object { "$($_.configuration)/$($_.phase)" } | Sort-Object -Unique)
    foreach ($case in $cases) {
        $entry = [ordered]@{ case = $case }
        foreach ($metric in $metricPaths.Keys) {
            $values = @{}
            foreach ($runtime in @("baseline", "optimized")) {
                $runs = @($reports | Where-Object runtime -eq $runtime | ForEach-Object { $_.runs } |
                    Where-Object { "$($_.configuration)/$($_.phase)" -eq $case })
                if ($runs.Count -gt 0) {
                    $samples = @($runs | ForEach-Object {
                        $value = $_
                        foreach ($component in $metricPaths[$metric].Split('.')) {
                            # The archived runtime has no optional warming events, so its
                            # empty warm distribution deliberately has no maximum field.
                            if ($metric -eq 'surface_warm_dispatch_max_wall_ms' -and
                                $component -eq 'max' -and $null -eq $value.PSObject.Properties[$component]) {
                                $value = 0
                            } else { $value = $value.$component }
                        }
                        [double]$value
                    })
                    $values[$runtime] = Get-Median $samples
                    $values["${runtime}_p95"] = Get-P95 $samples
                    $values["${runtime}_max"] = ($samples | Measure-Object -Maximum).Maximum
                    $values["${runtime}_count"] = $samples.Count
                }
            }
            if ($values.ContainsKey("baseline") -and $values.baseline -gt 0 -and $values.ContainsKey("optimized")) {
                $values["change_percent"] = 100 * ($values.optimized / $values.baseline - 1)
            }
            if ($values.ContainsKey("baseline_p95") -and $values.baseline_p95 -gt 0 -and $values.ContainsKey("optimized_p95")) {
                $values["p95_change_percent"] = 100 * ($values.optimized_p95 / $values.baseline_p95 - 1)
            }
            $entry[$metric] = $values
        }
        $comparison += [pscustomobject]$entry
    }
    $optimizedRuns = @($reports | Where-Object runtime -eq "optimized" | ForEach-Object { $_.runs })
    $invariants = [ordered]@{
        painted_geometry_matches_model = @($optimizedRuns | Where-Object geometry_matches_model -ne $true).Count -eq 0
        exactly_one_keyboard_activation = @($optimizedRuns | Where-Object keyboard_activation_calls -ne 1).Count -eq 0
        exactly_one_initial_state_notification = @($optimizedRuns | Where-Object initial_state_notifications -ne 1).Count -eq 0
        preferred_reveal_owner_is_first_display = @($reports | ForEach-Object { $_.runs } | Where-Object preferred_reveal_owner_is_first_display -ne $true).Count -eq 0
        only_preferred_keyboard_owner_activated = @($optimizedRuns | Where-Object only_preferred_keyboard_owner_activated -ne $true).Count -eq 0
        no_surface_warm_after_acquisition_delivery = @($optimizedRuns | Where-Object { $_.native_surface_warms_total -ne $_.native_surface_warms_before_acquire }).Count -eq 0
    }
    $result = [ordered]@{
        measurement = "Native Qt Paint and enclosing Expose/UpdateRequest dispatch including backing-store flush; a persistent background producer feeds synthetic input into the real GlobalMouseManager mailbox and posts acquisition through a normal queued Qt call. Wall time includes native waits; GUI CPU uses GetThreadTimes and may be quantized. Worker readiness and GUI delivery delay are separate. No DWM presentation fence."
        baseline_source_root = $BaselineDirectory
        baseline_revision = if (![string]::IsNullOrWhiteSpace($BaselineDirectory) -and (Test-Path -LiteralPath (Join-Path $BaselineDirectory "../revision.txt"))) {
            (Get-Content -LiteralPath (Join-Path $BaselineDirectory "../revision.txt") -Raw).Trim()
        } else { $null }
        actual_screens = $reports[0].actual_screens
        acquisition_delay_ms = $AcquisitionDelayMilliseconds
        duration_ms = $DurationMilliseconds
        iterations_per_round = $Iterations
        rounds = $Rounds
        statistics = [ordered]@{
            median = "Central value for odd samples; arithmetic mean of the two central values for even samples."
            p95 = "Sorted sample at ceiling(0.95 * (count - 1)); for 8 or 16 samples this is the maximum."
        }
        invariants = $invariants
        comparison = $comparison
        reports = @($reports | ForEach-Object { "$($_.runtime): $($_.runs.Count) captures" })
    }
    $result | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath (Join-Path $OutputDirectory "comparison.json") -Encoding utf8NoBOM
    $comparison | ForEach-Object {
        [pscustomobject]@{ case = $_.case; baseline_reveal_ms = $_.reveal_to_first_frame_dispatch_end_ms.baseline;
            optimized_reveal_ms = $_.reveal_to_first_frame_dispatch_end_ms.optimized;
            change_percent = $_.reveal_to_first_frame_dispatch_end_ms.change_percent }
    } | Format-Table -AutoSize
    Write-Host "Startup performance reports: $OutputDirectory"
    if ($invariants.Values -contains $false) { throw "Startup invariants failed; inspect comparison.json" }
}
finally { Pop-Location }
