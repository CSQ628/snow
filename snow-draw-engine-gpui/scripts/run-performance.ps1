[CmdletBinding()]
param([string]$OutputFile)
$ErrorActionPreference='Stop'
$projectRoot=Split-Path -Parent $PSScriptRoot
if([string]::IsNullOrWhiteSpace($OutputFile)){$OutputFile=Join-Path $projectRoot '../artifacts/snow-draw-gpui-performance.json'}
Push-Location -LiteralPath $projectRoot
try {& cargo bench --locked --offline -p snow-draw-engine-gpui --bench canvas --profile canvas-perf -- ([System.IO.Path]::GetFullPath($OutputFile));if($LASTEXITCODE -ne 0){throw 'Canvas benchmark failed'}}finally{Pop-Location}
