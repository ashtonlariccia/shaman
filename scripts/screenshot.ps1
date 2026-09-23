<#
.SYNOPSIS
  Capture the Shaman window to a PNG without disturbing the user.

.DESCRIPTION
  Uses PrintWindow with PW_RENDERFULLCONTENT rather than a screen-region grab.
  This matters for two reasons:

    1. It does NOT bring the window to the foreground, so whatever the user is
       working on keeps focus.
    2. It captures only Shaman's own pixels. A CopyFromScreen approach would
       capture whatever happens to be on top of it -- disruptive and a privacy
       problem.
#>
param(
  [string]$ProcessName = "shaman",
  # Target a specific process. Without this we would capture whichever Shaman
  # window happens to be first -- including one the USER opened, which silently
  # produces screenshots of the wrong (often older) build.
  [int]$ProcessId = 0,
  [string]$Out = "$env:TEMP\shaman-window.png"
)

$ErrorActionPreference = "Stop"
Add-Type -AssemblyName System.Drawing

Add-Type @"
using System;
using System.Runtime.InteropServices;
public class WinCap {
  [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr hdc, uint flags);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern IntPtr SetProcessDpiAwarenessContext(IntPtr ctx);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
}
"@

# MUST run before any window measurement. Without it this process is
# DPI-unaware, so Windows hands back *virtualized* coordinates: on a 150%
# display a 1800x1200 window measures as 1200x800, and PrintWindow then fills
# only the top-left two-thirds of the bitmap. That silently cropped every
# screenshot and made real UI (the window controls) look absent.
$DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2 = [IntPtr](-4)
if ([WinCap]::SetProcessDpiAwarenessContext($DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) -eq [IntPtr]::Zero) {
  # Older Windows: fall back to system-DPI awareness, which is enough here.
  [void][WinCap]::SetProcessDPIAware()
}

if ($ProcessId -ne 0) {
  $proc = Get-Process -Id $ProcessId -ErrorAction SilentlyContinue
  if (-not $proc) { Write-Error "process $ProcessId is not running"; exit 1 }
  if ($proc.MainWindowHandle -eq 0) { Write-Error "process $ProcessId has no window"; exit 1 }
} else {
  $candidates = @(Get-Process -Name $ProcessName -ErrorAction SilentlyContinue |
                  Where-Object { $_.MainWindowHandle -ne 0 })
  if ($candidates.Count -eq 0) {
    Write-Error "no '$ProcessName' process with a visible window found"
    exit 1
  }
  if ($candidates.Count -gt 1) {
    # Ambiguous: one of these is probably the user's own window.
    Write-Warning "$($candidates.Count) '$ProcessName' windows found; pass -ProcessId to disambiguate. Using PID $($candidates[0].Id)."
  }
  $proc = $candidates[0]
}

$hwnd = $proc.MainWindowHandle
$rect = New-Object WinCap+RECT
[void][WinCap]::GetWindowRect($hwnd, [ref]$rect)

$w = $rect.R - $rect.L
$h = $rect.B - $rect.T
if ($w -le 0 -or $h -le 0) { Write-Error "window has zero size"; exit 1 }

$bmp = New-Object System.Drawing.Bitmap($w, $h)
$gfx = [System.Drawing.Graphics]::FromImage($bmp)
$hdc = $gfx.GetHdc()
try {
  # 2 = PW_RENDERFULLCONTENT, required to capture composited/WebView2 content.
  [void][WinCap]::PrintWindow($hwnd, $hdc, 2)
} finally {
  $gfx.ReleaseHdc($hdc)
}

$bmp.Save($Out, [System.Drawing.Imaging.ImageFormat]::Png)
$gfx.Dispose()
$bmp.Dispose()

Write-Output "$Out ($w x $h)"
