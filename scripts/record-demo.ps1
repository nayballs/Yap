# Manual demo recorder with a start/stop "button" (the Enter key).
#
#   powershell -ExecutionPolicy Bypass -File scripts\record-demo.ps1
#
# Flow: get your windows arranged -> press ENTER to start -> do the dictation
# (hotkey, talk, hotkey) -> press ENTER again to stop. The recording is
# palette-encoded into an optimized docs/demo.gif.
#
# Captures the PRIMARY monitor only by default (multi-monitor desktops would
# otherwise squash into an unreadable ribbon). Use -Region "x,y,w,h" to crop
# tighter (e.g. just Notepad + the overlay).

param(
    [int]$Fps = 12,
    [int]$Width = 960,
    [string]$Region = "",   # "x,y,w,h" — empty = primary monitor
    [string]$Out = "docs\demo.gif"
)

$ErrorActionPreference = "Stop"
Set-Location (Split-Path -Parent $PSScriptRoot)  # repo root
New-Item -ItemType Directory -Force -Path (Split-Path $Out) | Out-Null
$tmp = Join-Path $env:TEMP "yap-demo-raw.mp4"

# Capture area: explicit region, else the primary monitor.
if ($Region) {
    $x, $y, $w, $h = ($Region -split ",") | ForEach-Object { [int]$_.Trim() }
} else {
    Add-Type -AssemblyName System.Windows.Forms
    $b = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
    $x = $b.X; $y = $b.Y; $w = $b.Width; $h = $b.Height
}
# gdigrab needs even dimensions for libx264.
$w -= ($w % 2); $h -= ($h % 2)

Write-Host ""
Write-Host "  Capture area: ${w}x${h} at ($x,$y)" -ForegroundColor DarkGray
Write-Host "  Arrange your windows (Notepad front and center), then..." -ForegroundColor Cyan
Read-Host  "  Press ENTER to START recording"

# ffmpeg with stdin attached: writing 'q' stops it cleanly (valid mp4 trailer).
$psi = New-Object System.Diagnostics.ProcessStartInfo
$psi.FileName = "ffmpeg"
$psi.Arguments = "-y -f gdigrab -framerate 30 -offset_x $x -offset_y $y -video_size ${w}x${h} -i desktop " +
                 "-c:v libx264 -preset ultrafast -pix_fmt yuv420p `"$tmp`" -loglevel error"
$psi.UseShellExecute = $false
$psi.RedirectStandardInput = $true
$proc = [System.Diagnostics.Process]::Start($psi)

Write-Host "  ● RECORDING — do the take! (hotkey, talk, hotkey)" -ForegroundColor Red
Read-Host  "  Press ENTER to STOP"

$proc.StandardInput.Write("q")   # graceful stop
$proc.WaitForExit(10000) | Out-Null
if (-not $proc.HasExited) { $proc.Kill() }

Write-Host "  Encoding GIF..." -ForegroundColor Cyan
$filters = "fps=$Fps,scale=${Width}:-1:flags=lanczos"
& ffmpeg -y -i $tmp -vf "$filters,palettegen=stats_mode=diff" "$env:TEMP\yap-demo-palette.png" -loglevel error
& ffmpeg -y -i $tmp -i "$env:TEMP\yap-demo-palette.png" `
    -lavfi "$filters [x]; [x][1:v] paletteuse=dither=bayer:bayer_scale=4:diff_mode=rectangle" `
    $Out -loglevel error
Remove-Item $tmp, "$env:TEMP\yap-demo-palette.png" -ErrorAction SilentlyContinue

$size = [math]::Round((Get-Item $Out).Length / 1MB, 2)
Write-Host ""
Write-Host "  Saved $Out ($size MB)" -ForegroundColor Green
if ($size -gt 10) {
    Write-Host "  (>10 MB is heavy for a README — do a shorter take or crop with -Region)" -ForegroundColor Yellow
}
