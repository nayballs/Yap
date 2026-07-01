@echo off
REM ── Yap Dev launcher ─────────────────────────────────────────────
REM Runs Yap from SOURCE with hot-reload (Vite on :51437 + Tauri dev).
REM This is the LIVE code, not the installed/compiled release build.
REM Edit files in src/ or src-tauri/ and the app reloads automatically.
REM Keep this window open while you work; close it to stop Yap.

title Yap Dev (live)
cd /d "%~dp0.."

REM whisper-rs-sys' cmake build nests paths past Windows MAX_PATH (260) and
REM MSBuild's FileTracker fails with FTK1011 regardless of the LongPathsEnabled
REM registry opt-in — same workaround as CI (nightly.yml): build into a SHORT
REM target dir on this drive instead of src-tauri\target.
set CARGO_TARGET_DIR=%~d0\t

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
REM Without the SDK the engines build panics ("Please install Vulkan SDK"), so
REM fall back to the STUB automatically: UI + hot reload all work, transcription
REM returns placeholder text.
if defined VULKAN_SDK (
  call npm run tauri dev -- --features engines
) else (
  echo   NOTE: Vulkan SDK not found ^(VULKAN_SDK is not set^) - running the
  echo   STUB build instead: full UI + hot reload, but NO real transcription.
  echo   For the real GPU pipeline install the SDK: winget install LunarG.VulkanSDK
  echo   ^(then reopen this window so the new environment is picked up^).
  echo.
  call npm run tauri dev
)

echo.
echo Yap Dev stopped. Press any key to close this window.
pause >nul
