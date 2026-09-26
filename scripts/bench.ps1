<#
.SYNOPSIS
  Measure Shaman's terminal pipeline: ConPTY alone, keystroke echo, bulk output
  and xterm rendering, plus peak memory. Prints a JSON report.

.DESCRIPTION
  Launches the release build with SHAMAN_BENCH set, which opens one cmd tab and
  runs the harness in it (crates/shaman-app/src/bench.rs, ui/src/lib/bench.ts).
  The app closes itself when done.

  A window opens for ~20-40s while it runs. Leave it alone: resizing it or
  typing into it skews the numbers.

  -OsConpty moves the bundled conpty.dll aside for the run, to compare against
  the copy built into Windows. They are put back afterwards whatever happens.

  Settings, pins and saved connections are redirected to a scratch directory, so
  nothing the run does touches the real ones.

.EXAMPLE
  powershell -File scripts\bench.ps1
  powershell -File scripts\bench.ps1 -OsConpty -MB 32
#>
param(
  [switch]$OsConpty,
  [int]$MB = 16,
  [string]$Root = (Split-Path -Parent $PSScriptRoot),
  [int]$TimeoutSeconds = 240
)

$ErrorActionPreference = "Stop"
$exe = Join-Path $Root "target\release\shaman.exe"
if (-not (Test-Path $exe)) { throw "no release build at $exe -- run scripts/release.sh first" }

$dir = Split-Path $exe
$moved = @()
$out = Join-Path $env:TEMP "shaman-bench-report.json"
$data = Join-Path $env:TEMP "shaman-bench-data"
Remove-Item $out -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $data | Out-Null

function Get-TreeMemory([int]$rootId) {
  # WebView2 renders in child processes (browser, GPU, renderer), so the app's
  # real footprint is the whole tree, not shaman.exe alone.
  $all = Get-CimInstance Win32_Process -Property ProcessId, ParentProcessId, WorkingSetSize
  $ids = @{ $rootId = $true }
  do {
    $grew = $false
    foreach ($p in $all) {
      if ($ids.ContainsKey([int]$p.ParentProcessId) -and -not $ids.ContainsKey([int]$p.ProcessId)) {
        $ids[[int]$p.ProcessId] = $true
        $grew = $true
      }
    }
  } while ($grew)
  $bytes = ($all | Where-Object { $ids.ContainsKey([int]$_.ProcessId) } | Measure-Object WorkingSetSize -Sum).Sum
  return [math]::Round($bytes / 1MB)
}

try {
  if ($OsConpty) {
    foreach ($f in "conpty.dll", "OpenConsole.exe") {
      $p = Join-Path $dir $f
      if (Test-Path $p) { Rename-Item $p "$f.bench-off"; $moved += $f }
    }
  }

  $env:SHAMAN_BENCH = "1"
  $env:SHAMAN_BENCH_MB = "$MB"
  $env:SHAMAN_BENCH_OUT = $out
  $env:SHAMAN_DATA_DIR = $data
  $proc = Start-Process -FilePath $exe -PassThru

  $peak = 0
  $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
  while (-not $proc.HasExited) {
    if ((Get-Date) -gt $deadline) {
      Stop-Process -Id $proc.Id -Force
      throw "bench did not finish within $TimeoutSeconds s"
    }
    try { $peak = [math]::Max($peak, (Get-TreeMemory $proc.Id)) } catch {}
    Start-Sleep -Milliseconds 1000
  }

  if (-not (Test-Path $out)) { throw "the app exited without writing a report" }
  $report = Get-Content $out -Raw | ConvertFrom-Json
  $report | Add-Member peakTreeWorkingSetMB $peak
  $report | ConvertTo-Json -Depth 6
}
finally {
  foreach ($f in $moved) { Rename-Item (Join-Path $dir "$f.bench-off") $f }
  Remove-Item Env:SHAMAN_BENCH, Env:SHAMAN_BENCH_MB, Env:SHAMAN_BENCH_OUT, Env:SHAMAN_DATA_DIR -ErrorAction SilentlyContinue
}
