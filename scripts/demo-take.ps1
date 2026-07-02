# Fully-automated README demo take — no human performance required.
#
# Plays a TTS voice through the SPEAKERS; the real microphone picks it up and
# Yap genuinely transcribes it (same as a person talking at the desk). The
# script opens Notepad, presses the dictation hotkey, "speaks", presses it
# again, and screen-records the whole thing into an optimized docs/demo.gif.
#
# Before running: Yap must be running (ONE instance only), speakers audible
# (not headphones — the mic must hear the voice), and hands off the keyboard
# for ~20 seconds.
#
#   powershell -ExecutionPolicy Bypass -File scripts\demo-take.ps1
#   ... -Text "custom line with um filler words" -Seconds 16

param(
    [string]$Text = "so um this is me dictating with yap, it uh, types straight into any app you're using",
    [int]$Seconds = 16,
    [int]$Fps = 12,
    [int]$Width = 960,
    [string]$Voice = "Microsoft Hazel Desktop",
    [string]$Out = "docs\demo.gif"
)

$ErrorActionPreference = "Stop"
Set-Location (Split-Path -Parent $PSScriptRoot)
New-Item -ItemType Directory -Force -Path (Split-Path $Out) | Out-Null
$tmp = Join-Path $env:TEMP "yap-demo-raw.mp4"

# Dictation hotkey from Yap's own config ("kb:56" = the '8' key, etc.).
$cfgPath = Join-Path $env:APPDATA "yap\config.json"
$vk = 120 # F9 fallback
if (Test-Path $cfgPath) {
    $hk = (Get-Content $cfgPath -Raw | ConvertFrom-Json).hotkey
    if ($hk -match '^kb:(\d+)$') { $vk = [int]$Matches[1] }
}

# Native key press (keybd_event) — SendKeys can't do raw VK codes.
Add-Type @"
using System;
using System.Runtime.InteropServices;
public static class VKey {
    [DllImport("user32.dll")] static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);
    public static void Tap(byte vk) { keybd_event(vk, 0, 0, UIntPtr.Zero); System.Threading.Thread.Sleep(60); keybd_event(vk, 0, 2, UIntPtr.Zero); }
}
"@

Add-Type -AssemblyName System.Speech
$tts = New-Object System.Speech.Synthesis.SpeechSynthesizer
try { $tts.SelectVoice($Voice) } catch { Write-Host "  ($Voice not found - using default voice)" -ForegroundColor Yellow }
$tts.Volume = 100
$tts.Rate = 0

# Baseline history count so we can confirm the take actually transcribed.
$histPath = Join-Path $env:APPDATA "yap\history.json"
$histBefore = 0
if (Test-Path $histPath) {
    try { $histBefore = (Get-Content $histPath -Raw | ConvertFrom-Json).Count } catch {}
}

Write-Host ""
Write-Host "  Demo take: hotkey vk=$vk, voice=$($tts.Voice.Name)" -ForegroundColor Cyan
Write-Host "  Opening Notepad + starting recorder - HANDS OFF for ~$Seconds s" -ForegroundColor Yellow

# Fresh Notepad, focused.
$np = Start-Process notepad -PassThru
Start-Sleep -Milliseconds 1500

# Screen recorder in the background for the whole take.
$ff = Start-Process ffmpeg -ArgumentList @(
    "-y", "-f", "gdigrab", "-framerate", "30", "-t", "$Seconds", "-i", "desktop",
    "-c:v", "libx264", "-preset", "ultrafast", "-pix_fmt", "yuv420p", "$tmp", "-loglevel", "error"
) -PassThru -WindowStyle Hidden
Start-Sleep -Milliseconds 1200

# The take: hotkey -> speak -> hotkey -> wait for transcription + injection.
[VKey]::Tap($vk)
Start-Sleep -Milliseconds 700
$tts.Speak($Text)          # blocks until finished speaking
Start-Sleep -Milliseconds 400
[VKey]::Tap($vk)

$ff.WaitForExit()
Write-Host "  Recording done - encoding GIF..." -ForegroundColor Cyan

$filters = "fps=$Fps,scale=${Width}:-1:flags=lanczos"
& ffmpeg -y -i $tmp -vf "$filters,palettegen=stats_mode=diff" "$env:TEMP\yap-demo-palette.png" -loglevel error
& ffmpeg -y -i $tmp -i "$env:TEMP\yap-demo-palette.png" `
    -lavfi "$filters [x]; [x][1:v] paletteuse=dither=bayer:bayer_scale=4:diff_mode=rectangle" `
    $Out -loglevel error
Remove-Item $tmp, "$env:TEMP\yap-demo-palette.png" -ErrorAction SilentlyContinue

# Verify the dictation really landed (new history entry).
Start-Sleep -Milliseconds 500
$landed = "(history unavailable)"
if (Test-Path $histPath) {
    try {
        $hist = Get-Content $histPath -Raw | ConvertFrom-Json
        if ($hist.Count -gt $histBefore) { $landed = "TRANSCRIBED: `"$($hist[0].text)`"" }
        else { $landed = "WARNING: no new history entry - the mic may not have heard the speakers" }
    } catch {}
}

$size = [math]::Round((Get-Item $Out).Length / 1MB, 2)
Write-Host ""
Write-Host "  Saved $Out ($size MB)" -ForegroundColor Green
Write-Host "  $landed" -ForegroundColor Green
if ($np -and -not $np.HasExited) { Write-Host "  (Notepad left open so you can inspect the result)" -ForegroundColor DarkGray }
