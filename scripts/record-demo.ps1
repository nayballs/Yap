# Records a short screen capture and converts it into an optimized GIF for the
# README (docs/demo.gif). Two-pass palette encode keeps the file small & crisp.
#
# Usage (from the repo root, or double-click -> "Run with PowerShell"):
#   powershell -ExecutionPolicy Bypass -File scripts\record-demo.ps1
#   powershell ... -Seconds 15                 # longer take
#   powershell ... -Region "100,200,1280,720"  # capture x,y,width,height only
#
# Suggested demo take (~10s): a Notepad window front and center, press F9,
# say "this is me dictating with yap um it types straight into any app",
# press F9 again, let the cleaned text land. Cut!

param(
    [int]$Seconds = 12,
    [int]$Fps = 12,
    [int]$Width = 960,
    [string]$Region = "",   # "x,y,w,h" — empty = full primary screen
    [string]$Out = "docs\demo.gif"
)

$ErrorActionPreference = "Stop"
Set-Location (Split-Path -Parent $PSScriptRoot)  # repo root
New-Item -ItemType Directory -Force -Path (Split-Path $Out) | Out-Null
$tmp = Join-Path $env:TEMP "yap-demo-raw.mp4"

# Grab args: full desktop, or a region if given.
$grab = @("-f", "gdigrab", "-framerate", "30")
if ($Region) {
    $x, $y, $w, $h = $Region -split ","
    $grab += @("-offset_x", $x.Trim(), "-offset_y", $y.Trim(), "-video_size", "$($w.Trim())x$($h.Trim())")
}
$grab += @("-t", $Seconds, "-i", "desktop")

Write-Host ""
Write-Host "  Recording $Seconds seconds in..." -ForegroundColor Cyan
foreach ($n in 3, 2, 1) { Write-Host "  $n..." -ForegroundColor Yellow; Start-Sleep 1 }
Write-Host "  RECORDING - do the dictation now! (F9, talk, F9)" -ForegroundColor Red

& ffmpeg -y @grab -c:v libx264 -preset ultrafast -pix_fmt yuv420p $tmp -loglevel error
Write-Host "  Recording done - encoding GIF..." -ForegroundColor Cyan

# Two-pass palette encode: dramatically better colours + smaller file than
# a naive -f gif. fps + width tuned for a README hero.
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
    Write-Host "  (>10 MB is heavy for a README - consider -Seconds 8 or a -Region crop)" -ForegroundColor Yellow
}
