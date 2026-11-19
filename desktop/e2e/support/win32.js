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

/**
 * A window's placement as Windows keeps it: minimised or not, its rect, and
 * the rect it's restored to (`rcNormalPosition`), physical px. With
 * `minimise`, it's first minimised from outside, as Win+M, Show desktop or
 * a shake of another window does (`ShowWindow(SW_SHOWMINNOACTIVE)`); with
 * `size`, first sized from outside, where it is (`SetWindowPos`, no
 * activation).
 * @param {number} hwnd
 * @param {{ minimise?: boolean, size?: [number, number] }} [opts]
 */
export function windowPlacement(hwnd, { minimise = false, size = null } = {}) {
  const [sw, sh] = size ?? [0, 0];
  const script = `
$ErrorActionPreference = 'Stop'
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class YapPlace {
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
  [StructLayout(LayoutKind.Sequential)] public struct WP { public int len, flags, show; public int mx, my, xx, xy; public RECT normal; }
  [DllImport("user32.dll")] public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr c);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int cmd);
  [DllImport("user32.dll")] public static extern bool IsIconic(IntPtr h);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool GetWindowPlacement(IntPtr h, ref WP p);
  [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int cx, int cy, uint flags);
}
'@
[void][YapPlace]::SetThreadDpiAwarenessContext([IntPtr](-4))
$h = [IntPtr][Int64]${Number(hwnd)}
if (${minimise ? '$true' : '$false'}) { [void][YapPlace]::ShowWindow($h, 7) }
# SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE
if (${size ? '$true' : '$false'}) { [void][YapPlace]::SetWindowPos($h, [IntPtr]::Zero, 0, 0, ${Math.round(sw)}, ${Math.round(sh)}, 0x16) }
$r = New-Object YapPlace+RECT
[void][YapPlace]::GetWindowRect($h, [ref]$r)
$p = New-Object YapPlace+WP
$p.len = [System.Runtime.InteropServices.Marshal]::SizeOf($p)
[void][YapPlace]::GetWindowPlacement($h, [ref]$p)
@{
  iconic = [YapPlace]::IsIconic($h); visible = [YapPlace]::IsWindowVisible($h)
  rect = @($r.L, $r.T, ($r.R - $r.L), ($r.B - $r.T))
  normal = @($p.normal.L, $p.normal.T, ($p.normal.R - $p.normal.L), ($p.normal.B - $p.normal.T))
} | ConvertTo-Json -Compress
`;
  const out = execFileSync('powershell.exe', ['-NoProfile', '-NonInteractive', '-Command', script], {
    encoding: 'utf8',
    windowsHide: true,
  });
  const r = JSON.parse(out.trim());
  const [x, y, width, height] = r.rect;
  const [nx, ny, nwidth, nheight] = r.normal;
  return {
    iconic: !!r.iconic,
    visible: !!r.visible,
    rect: { x, y, width, height },
    normal: { x: nx, y: ny, width: nwidth, height: nheight },
  };
}
