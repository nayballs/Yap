@echo off
REM ── Blip Dev launcher ─────────────────────────────────────────────
REM Runs Blip from SOURCE with hot-reload (Vite on :1430 + Tauri dev).
REM This is the LIVE code, not the installed/compiled release build.
REM Edit files in src/ or src-tauri/ and the app reloads automatically.
REM Keep this window open while you work; close it to stop Blip.

title Blip Dev (live)
cd /d "%~dp0.."

echo ============================================================
echo   Blip Dev - running the LIVE source with hot reload
echo   Project: %cd%
echo   (Close this window to stop Blip.)
echo ============================================================
echo.

REM Runs the REAL multi-engine pipeline: Whisper on CUDA + ONNX models on
REM DirectML. CMAKE_CUDA_ARCHITECTURES=native lets nvcc target this machine's
REM GPU. The FIRST build is slow (compiles whisper.cpp + downloads ONNX
REM Runtime); later builds are fast.
set CMAKE_CUDA_ARCHITECTURES=native
call npm run tauri dev -- --features cuda

REM ── Fast / no-GPU alternative ─────────────────────────────────────
REM Comment the two lines above and uncomment the line below to build the
REM STUB instead (no real transcription, fastest compile — handy for pure
REM UI work or machines without CUDA):
REM call npm run tauri dev

echo.
echo Blip Dev stopped. Press any key to close this window.
pause >nul
