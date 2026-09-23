<#
.SYNOPSIS
  Automated end-to-end check: boot Shaman, confirm the UI came alive, capture a
  screenshot, and shut everything down.

.DESCRIPTION
  Debug builds of Tauri load `devUrl` (the Vite server), NOT the bundled dist.
  Running target\debug\shaman.exe on its own therefore yields a blank window and
  no IPC. This script starts Vite first, which is what makes the check valid.

  Verification is based on the UI_READY log line rather than on pixels: WebView2
  renders out of process, so an empty page and a failed capture look identical.

  Cleanup only ever targets the process listening on the Vite port, so unrelated
  node.exe processes are left alone.
#>
param(
  [string]$Root = (Split-Path -Parent $PSScriptRoot),
  [int]$Port = 5173,
  [int]$SettleSeconds = 7,
  [string]$Marker = "UI_READY",
  [string]$Shot = "C:\Users\Public\shaman-window.png"
)

$ErrorActionPreference = "Stop"
$exe = Join-Path $Root "target\debug\shaman.exe"
$log = "$env:TEMP\shaman-verify.log"
$errlog = "$env:TEMP\shaman-verify.err"

function Stop-Port([int]$p) {
  $conns = Get-NetTCPConnection -LocalPort $p -State Listen -ErrorAction SilentlyContinue
  foreach ($pid_ in ($conns.OwningProcess | Select-Object -Unique)) {
    Stop-Process -Id $pid_ -Force -ErrorAction SilentlyContinue
  }
}

# Build first, always. A stale exe is the worst possible failure mode here: the
# frontend is served fresh by Vite while the Rust half is whatever was last
# compiled, so a new command appears to exist and then rejects at runtime -- and
# the run still reports PASS, because UI_READY is unrelated. Rebuilding costs a
# second when nothing changed.
#
# NOTE: no `2>&1` here. cargo writes its progress to stderr, and redirecting
# that into the PowerShell pipeline turns every "Compiling ..." line into a
# NativeCommandError, which `$ErrorActionPreference = "Stop"` then treats as
# fatal. Let it go to the console and judge the build by its exit code.
Write-Output "==> building shaman (debug)"
& cargo.exe build -p shaman-app
if ($LASTEXITCODE -ne 0) { Write-Error "build failed"; exit 1 }

if (-not (Test-Path $exe)) { Write-Error "missing $exe -- run: cargo.exe build"; exit 1 }

$startedVite = $false
$conns = Get-NetTCPConnection -LocalPort $Port -State Listen -ErrorAction SilentlyContinue
if (-not $conns) {
  Write-Output "==> starting vite on $Port"
  Start-Process -FilePath "cmd.exe" -ArgumentList "/c", "npm run dev" `
    -WorkingDirectory $Root -WindowStyle Hidden | Out-Null
  $startedVite = $true

  $ok = $false
  for ($i = 0; $i -lt 60; $i++) {
    try {
      if ((Invoke-WebRequest "http://localhost:$Port/" -UseBasicParsing -TimeoutSec 2).StatusCode -eq 200) { $ok = $true; break }
    } catch { Start-Sleep -Milliseconds 500 }
  }
  if (-not $ok) { Write-Error "vite never came up on $Port"; exit 1 }
} else {
  Write-Output "==> reusing vite already on $Port"
}

try {
  Remove-Item $log, $errlog -ErrorAction SilentlyContinue
  Write-Output "==> launching shaman"
  $p = Start-Process -FilePath $exe -PassThru -RedirectStandardOutput $log -RedirectStandardError $errlog
  Start-Sleep -Seconds $SettleSeconds

  if (-not $p.HasExited) {
    # Capture OUR instance by pid. The user may well have their own Shaman
    # window open; grabbing that one yields a screenshot of a different build.
    & (Join-Path $PSScriptRoot "screenshot.ps1") -ProcessId $p.Id -Out $Shot 2>&1 | Out-Null
  }
  Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
  Start-Sleep -Milliseconds 500
} finally {
  if ($startedVite) { Stop-Port $Port }
}

Write-Output "===== app log ====="
if (Test-Path $log) { Get-Content $log }
# -Raw yields $null (not "") for an empty file, so never call methods on these
# directly -- IsNullOrWhiteSpace is null-safe, .Trim() is not.
$errText = if (Test-Path $errlog) { Get-Content $errlog -Raw } else { $null }
if (-not [string]::IsNullOrWhiteSpace($errText)) {
  Write-Output "===== stderr ====="
  Write-Output $errText
}

$logText = if (Test-Path $log) { Get-Content $log -Raw } else { $null }
if ($logText -match [regex]::Escape($Marker)) {
  Write-Output "==> PASS: found '$Marker'"
  Write-Output "==> screenshot: $Shot"
  exit 0
} else {
  Write-Output "==> FAIL: '$Marker' never appeared"
  exit 1
}
