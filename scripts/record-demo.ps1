# Manual demo recorder with a start/stop "button" (the Enter key).
#
#   powershell -ExecutionPolicy Bypass -File record-demo.ps1
#
# Flow: arrange your windows -> ENTER to start -> do the take (the script to
# read is in docs\demo-script.md - put it on your other screen) -> ENTER to
# stop. Produces BOTH:
#   docs\demo.mp4  - screen + your MICROPHONE audio (for social/website/blob view)
#   docs\demo.gif  - silent, optimized (for the README hero)
#
# Captures the PRIMARY monitor only. Use -Region "x,y,w,h" to crop tighter.
# The mic is read from Yap's own config so it matches what Yap hears; pass
# -Mic "" to record without audio.

param(
    [int]$Fps = 12,
    [int]$Width = 960,
    [string]$Region = "",   # "x,y,w,h" - empty = primary monitor
    [string]$Mic = $null,   # DirectShow mic name; $null = read from Yap config; "" = no audio
    [string]$OutDir = "docs"
)

$ErrorActionPreference = "Stop"
Set-Location (Split-Path -Parent $PSScriptRoot)  # repo root
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
$mp4 = Join-Path $OutDir "demo.mp4"
$gif = Join-Path $OutDir "demo.gif"

# Capture area: explicit region, else the primary monitor.
if ($Region) {
    $x, $y, $w, $h = ($Region -split ",") | ForEach-Object { [int]$_.Trim() }
} else {
    Add-Type -AssemblyName System.Windows.Forms
    $b = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
    $x = $b.X; $y = $b.Y; $w = $b.Width; $h = $b.Height
}
$w -= ($w % 2); $h -= ($h % 2)

# Mic: same device Yap listens to, unless overridden.
if ($null -eq $Mic) {
    $cfgPath = Join-Path $env:APPDATA "yap\config.json"
    if (Test-Path $cfgPath) {
        try { $Mic = (Get-Content $cfgPath -Raw | ConvertFrom-Json).inputDevice } catch { $Mic = "" }
    }
}

$audioArgs = ""
if ($Mic) { $audioArgs = "-f dshow -i audio=`"$Mic`" " }

Write-Host ""
Write-Host "  Capture: ${w}x${h} at ($x,$y)   Mic: $(if ($Mic) { $Mic } else { '(none - silent)' })" -ForegroundColor DarkGray
Write-Host "  Script to read: docs\demo-script.md (open it on your other screen)" -ForegroundColor Cyan
Read-Host  "  Press ENTER to START recording"

# ffmpeg with stdin attached: writing 'q' stops it cleanly.
$psi = New-Object System.Diagnostics.ProcessStartInfo
$psi.FileName = "ffmpeg"
$psi.Arguments = "-y $audioArgs-f gdigrab -framerate 30 -offset_x $x -offset_y $y -video_size ${w}x${h} -i desktop " +
                 "-c:v libx264 -preset ultrafast -pix_fmt yuv420p -c:a aac -b:a 128k `"$mp4`" -loglevel error"
$psi.UseShellExecute = $false
$psi.RedirectStandardInput = $true
$proc = [System.Diagnostics.Process]::Start($psi)

Write-Host "  * RECORDING - do the take!" -ForegroundColor Red
Read-Host  "  Press ENTER to STOP"

$proc.StandardInput.Write("q")
$proc.WaitForExit(10000) | Out-Null
if (-not $proc.HasExited) { $proc.Kill() }

Write-Host "  Encoding GIF from the take..." -ForegroundColor Cyan
$filters = "fps=$Fps,scale=${Width}:-1:flags=lanczos"
& ffmpeg -y -i $mp4 -vf "$filters,palettegen=stats_mode=diff" "$env:TEMP\yap-demo-palette.png" -loglevel error
& ffmpeg -y -i $mp4 -i "$env:TEMP\yap-demo-palette.png" `
    -lavfi "$filters [x]; [x][1:v] paletteuse=dither=bayer:bayer_scale=4:diff_mode=rectangle" `
    $gif -loglevel error
Remove-Item "$env:TEMP\yap-demo-palette.png" -ErrorAction SilentlyContinue

$mp4Size = [math]::Round((Get-Item $mp4).Length / 1MB, 2)
$gifSize = [math]::Round((Get-Item $gif).Length / 1MB, 2)
Write-Host ""
Write-Host "  Saved $mp4 ($mp4Size MB, with audio) and $gif ($gifSize MB, silent)" -ForegroundColor Green
if ($gifSize -gt 10) {
    Write-Host "  (over 10 MB is heavy for a README - do a shorter take or crop with -Region)" -ForegroundColor Yellow
}
