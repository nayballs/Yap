@echo off
REM ── Yap Dev launcher ─────────────────────────────────────────────
REM Runs Yap from SOURCE with hot-reload (Vite on :51437 + Tauri dev).
REM This is the LIVE code, not the installed/compiled release build.
REM Edit files in src/ or src-tauri/ and the app reloads automatically.
REM Keep this window open while you work; close it to stop Yap.

title Yap Dev (live)
cd /d "%~dp0.."

echo ============================================================
echo   Yap Dev - running the LIVE source with hot reload
echo   Project: %cd%
echo   (Close this window to stop Yap.)
echo ============================================================
echo.

REM Runs the REAL multi-engine pipeline: Whisper on VULKAN (any GPU) + ONNX
REM models on DirectML. Needs the Vulkan SDK installed (https://vulkan.lunarg.com)
REM so the whisper-vulkan backend can build. The FIRST build is slow (compiles
REM whisper.cpp + downloads ONNX Runtime); later builds are fast.
call npm run tauri dev -- --features engines

REM ── Fast / no-GPU alternative ─────────────────────────────────────
REM Comment the line above and uncomment the line below to build the STUB
REM instead (no real transcription, fastest compile — handy for pure UI work
REM or when the Vulkan SDK isn't installed):
REM call npm run tauri dev

echo.
echo Yap Dev stopped. Press any key to close this window.
pause >nul
