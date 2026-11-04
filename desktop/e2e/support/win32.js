// Public Win32 queries about a window, asked from OUTSIDE the app (a
// PowerShell process calling user32), for the Yap bar's real-window checks:
// its extended styles (click-through, never activated, tool window…), and
// which top-level window a click at a point would reach — WindowFromPoint is
// hit-testing only: no input is sent and the cursor never moves.
import { execFileSync } from 'node:child_process';

const WS_EX = {
  topmost: 0x8,
  transparent: 0x20,
  toolWindow: 0x80,
  appWindow: 0x40000,
  layered: 0x80000,
  noActivate: 0x8000000,
};

/**
 * @param {number} hwnd  the window (from `bar_debug`)
 * @param {[number, number] | null} point  physical screen px to hit-test
 */
export function windowFacts(hwnd, point = null) {
  const [x, y] = point ?? [0, 0];
  const script = `
$ErrorActionPreference = 'Stop'
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class YapWin {
  [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X; public int Y; }
  [DllImport("user32.dll")] public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr c);
  [DllImport("user32.dll")] public static extern IntPtr GetWindowLongPtrW(IntPtr h, int i);
  [DllImport("user32.dll")] public static extern IntPtr WindowFromPoint(POINT p);
  [DllImport("user32.dll")] public static extern IntPtr GetAncestor(IntPtr h, uint f);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
}
'@
[void][YapWin]::SetThreadDpiAwarenessContext([IntPtr](-4))
$h = [IntPtr][Int64]${Number(hwnd)}
$ex = [Int64][YapWin]::GetWindowLongPtrW($h, -20)
$at = 0
if (${point ? '$true' : '$false'}) {
  $p = New-Object YapWin+POINT
  $p.X = ${Math.round(x)}; $p.Y = ${Math.round(y)}
  $at = [Int64][YapWin]::GetAncestor([YapWin]::WindowFromPoint($p), 2)
}
$fg = [YapWin]::GetForegroundWindow()
$fgPid = [uint32]0
[void][YapWin]::GetWindowThreadProcessId($fg, [ref]$fgPid)
@{ ex = $ex; visible = [YapWin]::IsWindowVisible($h); at = $at; fgPid = $fgPid } | ConvertTo-Json -Compress
`;
  const out = execFileSync('powershell.exe', ['-NoProfile', '-NonInteractive', '-Command', script], {
    encoding: 'utf8',
    windowsHide: true,
  });
  const r = JSON.parse(out.trim());
  const ex = Number(r.ex);
  const styles = Object.fromEntries(Object.entries(WS_EX).map(([k, bit]) => [k, (ex & bit) !== 0]));
  return {
    ...styles,
    visible: !!r.visible,
    /** The top-level window a click at `point` would reach (0 without one). */
    windowAtPoint: Number(r.at),
    foregroundPid: Number(r.fgPid),
  };
}
