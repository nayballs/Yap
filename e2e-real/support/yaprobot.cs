// yaprobot.exe — the real-app robot tests' Windows helper (e2e-real/,
// docs/e2e-tests.md "Real-app robot tests"). Compiled on the fly with the
// C# compiler every Windows has (.NET Framework 4, C# 5: no string
// interpolation, no `?.`), by support/targets.js.
//
//   yaprobot testpad <label>      the test pad, "Yap robot test pad <label>":
//                                 a multi-line text box, a rich text box, a
//                                 password box and a box that ignores Ctrl+V
//                                 ("nopaste"). Driven over stdin/stdout,
//                                 one JSON object per line; quits when its
//                                 stdin closes. It can also play a WAV on a
//                                 loop (`play` / `stop`): sound of the
//                                 test's own, for "Silence other apps".
//   yaprobot focus <hwnd>         bring a window to the front (no input sent
//                                 to any other app)
//   yaprobot foreground           the window in front (its handle only)
//   yaprobot find <nonce>         the visible top-level windows whose title
//                                 has the nonce (no other titles are printed)
//   yaprobot text <hwnd>          a window's text over UI Automation (the
//                                 TextPattern document, else ValuePattern)
//   yaprobot keys <hwnd> <nonce> <key>   press a key (escape, enter, ctrl+s,
//                                 ctrl+w, ctrl+end) — only when that window
//                                 is in front and titled with the nonce
//   yaprobot hookkeys <hwnd> <nonce> <seq>   keys for the robot's own Yap's
//                                 input hook (robot mode acts only on keys
//                                 marked as the robot's): "d:135,w:300,u:135"
//                                 — d/u a key down/up, U a key-up marked for
//                                 the hook to miss (a key-up the lock screen
//                                 took), w a wait in ms — only while that
//                                 window is in front and titled with the nonce
//   yaprobot close <hwnd> <nonce> ask a window titled with the nonce to close
//   yaprobot caret <hwnd> <nonce> what UI Automation reports around the caret
//                                 in that window (in front, titled with the
//                                 nonce), the way Yap reads it: for
//                                 diagnosing a fit
//   yaprobot yapwindow <pid>      the robot's own Yap's main window (the
//                                 largest visible window of that process)
//   yaprobot rightclick <hwnd> <pid> <x> <y> [shift]   a real right-click
//                                 at CSS point (x, y) of that Yap window (in
//                                 front, the point on it; with `shift`, Shift
//                                 held through it), then the windows of
//                                 Yap's process tree (WebView2's included)
//                                 that came up — a context menu — closed
//                                 again with Esc; the pointer goes back
//   yaprobot devtools <hwnd> <pid>   F12 in that Yap window (in front), then
//                                 the windows of Yap's process tree that
//                                 came up — DevTools — closed again
//                                 (WM_CLOSE)
//   yaprobot place <hwnd> <nonce> <x> <y>   move a window titled with the
//                                 nonce to (x, y), physical pixels (its size
//                                 kept; Windows rescales it for that
//                                 monitor's DPI)
//   yaprobot contentrect <hwnd>   a browser window's page area on screen
//                                 (its Chrome_RenderWidgetHostHWND), in
//                                 physical pixels
//   yaprobot snipdrag <pid> <x0> <y0> <x1> <y1> [ctrl|shift]   a real drag
//                                 from (x0, y0) to (x1, y1), physical pixels
//                                 (the same point twice: a click) — only when
//                                 the robot's own Yap's snip overlay is under
//                                 the pointer; Ctrl or Shift held as it's let
//                                 go only while that overlay is in front; the
//                                 pointer goes back
//   yaprobot snipkey <pid> <key> [<x> <y>]   escape, enter or space:
//                                 pressed while the robot's own Yap's snip
//                                 overlay is in front (the pointer first moved
//                                 to (x, y), only onto that overlay); says
//                                 what was in front
//   yaprobot framerect <hwnd>     a window's visible frame (DWM's extended
//                                 frame bounds), physical pixels
//   yaprobot keystate             which modifier keys Windows says are down
//   yaprobot modkey <hwnd> <owner> <key> <down|up>   a modifier (lalt,
//                                 lshift, lctrl, lwin…) pressed as an injected
//                                 key — stuck, no finger on it — only while
//                                 the test's window (`owner`: the nonce, or
//                                 pid:<n>) is in front; let go always
//   yaprobot clipreader <ms> <delay>   a careless clipboard manager's
//                                 stand-in, for `ms`: at each clipboard
//                                 change, `delay` ms later, it opens the
//                                 clipboard with its own window and reads
//                                 the text, whatever the privacy formats
//                                 say. Prints {"listening":true} when ready,
//                                 then what each read got: whether there was
//                                 text and its length — never the text
//   yaprobot idle | watch … | watchtest   the hands-off guard: how long the
//                                 PC has been idle, and the input watcher
//                                 that ends a run on the person's own input
//                                 (inputwatch.cs, compiled in with this file)
//
// The test pad also has a button ("button": a focus that takes no text)
// and holds the clipboard open on request (`holdClipboard {ms}`, or
// `holdAfterCopy {ms}`: once its own next Ctrl+C has copied), as a busy
// clipboard manager would. And a box that's no edit control ("slow"):
// Ctrl+V there reads the clipboard `slowPaste {ms}` later (an app slow to
// take a paste), or never (ms < 0).
//
// Every command prints one JSON object. Nothing here reads or prints the
// title or text of a window the test didn't open.
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;
using System.Threading;
using System.Windows.Automation;
using System.Windows.Automation.Text;
using System.Windows.Forms;
using System.Web.Script.Serialization;

static class Native {
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern bool BringWindowToTop(IntPtr h);
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int cmd);
    [DllImport("user32.dll")] public static extern bool IsIconic(IntPtr h);
    [DllImport("user32.dll")] public static extern bool IsWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll")] public static extern bool AttachThreadInput(uint a, uint b, bool attach);
    [DllImport("kernel32.dll")] public static extern uint GetCurrentThreadId();
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetWindowText(IntPtr h, StringBuilder sb, int max);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetClassName(IntPtr h, StringBuilder sb, int max);
    public delegate bool EnumProc(IntPtr h, IntPtr l);
    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr l);
    [DllImport("user32.dll")] public static extern bool EnumChildWindows(IntPtr parent, EnumProc cb, IntPtr l);
    [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int cx, int cy, uint flags);
    [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint msg, IntPtr w, IntPtr l);
    [DllImport("user32.dll")] public static extern IntPtr GetAncestor(IntPtr h, uint flags);
    [DllImport("user32.dll", SetLastError = true)] public static extern uint SendInput(uint n, INPUT[] inputs, int size);
    [DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(IntPtr ctx);
    [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr h, ref POINT p);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern IntPtr WindowFromPoint(POINT p);
    [DllImport("user32.dll")] public static extern bool GetCursorPos(out POINT p);
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("dwmapi.dll")] public static extern int DwmGetWindowAttribute(IntPtr h, int attr, out RECT r, int size);
    [DllImport("kernel32.dll", SetLastError = true)] public static extern IntPtr CreateToolhelp32Snapshot(uint flags, uint pid);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode)] public static extern bool Process32FirstW(IntPtr snap, ref PROCESSENTRY32W e);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode)] public static extern bool Process32NextW(IntPtr snap, ref PROCESSENTRY32W e);
    [DllImport("kernel32.dll")] public static extern bool CloseHandle(IntPtr h);

    [StructLayout(LayoutKind.Sequential)]
    public struct POINT { public int X; public int Y; }
    [StructLayout(LayoutKind.Sequential)]
    public struct RECT { public int Left; public int Top; public int Right; public int Bottom; }
    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    public struct PROCESSENTRY32W {
        public uint dwSize; public uint cntUsage; public uint th32ProcessID; public IntPtr th32DefaultHeapID;
        public uint th32ModuleID; public uint cntThreads; public uint th32ParentProcessID; public int pcPriClassBase;
        public uint dwFlags; [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 260)] public string szExeFile;
    }

    [StructLayout(LayoutKind.Sequential)]
    public struct KEYBDINPUT { public ushort wVk; public ushort wScan; public uint dwFlags; public uint time; public IntPtr dwExtraInfo; }
    [StructLayout(LayoutKind.Sequential)]
    public struct MOUSEINPUT { public int dx; public int dy; public uint mouseData; public uint dwFlags; public uint time; public IntPtr dwExtraInfo; }
    [StructLayout(LayoutKind.Explicit)]
    public struct INPUTUNION { [FieldOffset(0)] public MOUSEINPUT mi; [FieldOffset(0)] public KEYBDINPUT ki; }
    [StructLayout(LayoutKind.Sequential)]
    public struct INPUT { public uint type; public INPUTUNION u; }

    public static IntPtr Root(IntPtr h) {
        IntPtr r = GetAncestor(h, 2);
        return r == IntPtr.Zero ? h : r;
    }

    public static string Title(IntPtr h) {
        var sb = new StringBuilder(512);
        GetWindowText(h, sb, sb.Capacity);
        return sb.ToString();
    }

    public static string Class(IntPtr h) {
        var sb = new StringBuilder(256);
        GetClassName(h, sb, sb.Capacity);
        return sb.ToString();
    }
}

static class Program {
    static readonly JavaScriptSerializer Json = new JavaScriptSerializer { MaxJsonLength = int.MaxValue };

    static void Print(object o) {
        Console.Out.WriteLine(Json.Serialize(o));
        Console.Out.Flush();
    }

    static Dictionary<string, object> Obj(params object[] kv) {
        var d = new Dictionary<string, object>();
        for (int i = 0; i + 1 < kv.Length; i += 2) d[(string)kv[i]] = kv[i + 1];
        return d;
    }

    [STAThread]
    static int Main(string[] args) {
        // UTF-8 both ways (titles like "✳ …", any text the pad is given).
        var utf8 = new UTF8Encoding(false);
        Console.SetOut(new System.IO.StreamWriter(Console.OpenStandardOutput(), utf8) { AutoFlush = true });
        Console.SetIn(new System.IO.StreamReader(Console.OpenStandardInput(), utf8));
        try {
            if (args.Length == 0) throw new ArgumentException("no command");
            switch (args[0]) {
                case "testpad": return TestPad.Run(args[1]);
                case "focus": Print(Focus(new IntPtr(long.Parse(args[1])))); return 0;
                case "foreground": Print(Obj("hwnd", (long)Native.Root(Native.GetForegroundWindow()))); return 0;
                case "find": Print(Find(args[1])); return 0;
                case "text": Print(ReadText(new IntPtr(long.Parse(args[1])))); return 0;
                case "keys": Print(PressKey(new IntPtr(long.Parse(args[1])), args[2], args[3])); return 0;
                case "hookkeys": Print(HookKeys(new IntPtr(long.Parse(args[1])), args[2], args[3])); return 0;
                case "close": Print(Close(new IntPtr(long.Parse(args[1])), args[2])); return 0;
                case "caret": Print(Caret(new IntPtr(long.Parse(args[1])), args[2])); return 0;
                case "yapwindow": Print(YapWindow(uint.Parse(args[1]))); return 0;
                case "rightclick":
                    Print(RightClick(new IntPtr(long.Parse(args[1])), uint.Parse(args[2]),
                        double.Parse(args[3], System.Globalization.CultureInfo.InvariantCulture),
                        double.Parse(args[4], System.Globalization.CultureInfo.InvariantCulture),
                        args.Length > 5 && args[5] == "shift"));
                    return 0;
                case "devtools": Print(DevTools(new IntPtr(long.Parse(args[1])), uint.Parse(args[2]))); return 0;
                case "barclick":
                    Print(BarClick(uint.Parse(args[1]), int.Parse(args[2]), int.Parse(args[3]), int.Parse(args[4])));
                    return 0;
                case "contentrect": Print(ContentRect(new IntPtr(long.Parse(args[1])))); return 0;
                case "place": Print(Place(new IntPtr(long.Parse(args[1])), args[2], int.Parse(args[3]), int.Parse(args[4]))); return 0;
                case "snipdrag":
                    Print(SnipDrag(uint.Parse(args[1]), int.Parse(args[2]), int.Parse(args[3]), int.Parse(args[4]), int.Parse(args[5]),
                        args.Length > 6 ? args[6] : ""));
                    return 0;
                case "snipkey":
                    Print(args.Length > 4
                        ? SnipKey(uint.Parse(args[1]), args[2], int.Parse(args[3]), int.Parse(args[4]))
                        : SnipKey(uint.Parse(args[1]), args[2]));
                    return 0;
                case "framerect": Print(FrameRect(new IntPtr(long.Parse(args[1])))); return 0;
                case "keystate": Print(KeyState()); return 0;
                case "modkey": Print(ModKey(new IntPtr(long.Parse(args[1])), args[2], args[3], args[4])); return 0;
                case "clipreader": return ClipReader.Run(int.Parse(args[1]), int.Parse(args[2]));
                case "idle": Print(Watcher.Idle()); return 0;
                case "watch": return Watcher.Run(uint.Parse(args[1]), args[2], args[3]);
                case "watchtest": return Watcher.SelfTest(args.Length > 1 ? int.Parse(args[1]) : 0);
                default: throw new ArgumentException("unknown command " + args[0]);
            }
        } catch (Exception e) {
            Print(Obj("error", e.GetType().Name + ": " + e.Message));
            return 1;
        }
    }

    /// Bring `h` to the front: attach to the input of the window in front
    /// and of the target (the usual way round the foreground lock), then
    /// SetForegroundWindow. No keys or clicks go anywhere.
    static Dictionary<string, object> Focus(IntPtr h) {
        if (!Native.IsWindow(h)) return Obj("ok", false, "error", "no such window");
        if (Native.IsIconic(h)) Native.ShowWindow(h, 9);
        for (int attempt = 0; attempt < 5; attempt++) {
            IntPtr fg = Native.GetForegroundWindow();
            if (Native.Root(fg) == h) return Obj("ok", true);
            uint pid;
            uint cur = Native.GetCurrentThreadId();
            uint fgThread = fg == IntPtr.Zero ? 0 : Native.GetWindowThreadProcessId(fg, out pid);
            uint target = Native.GetWindowThreadProcessId(h, out pid);
            bool a1 = fgThread != 0 && fgThread != cur && Native.AttachThreadInput(cur, fgThread, true);
            bool a2 = target != cur && Native.AttachThreadInput(cur, target, true);
            Native.BringWindowToTop(h);
            Native.SetForegroundWindow(h);
            if (a2) Native.AttachThreadInput(cur, target, false);
            if (a1) Native.AttachThreadInput(cur, fgThread, false);
            for (int i = 0; i < 10; i++) {
                if (Native.Root(Native.GetForegroundWindow()) == h) return Obj("ok", true);
                Thread.Sleep(30);
            }
        }
        return Obj("ok", false, "error", "Windows didn't give the window the focus");
    }

    static List<object> Find(string nonce) {
        var found = new List<object>();
        if (nonce.Length < 12) return found;
        Native.EnumWindows(delegate (IntPtr h, IntPtr l) {
            if (!Native.IsWindowVisible(h)) return true;
            string title = Native.Title(h);
            if (title.Contains(nonce)) {
                uint pid;
                Native.GetWindowThreadProcessId(h, out pid);
                found.Add(Obj("hwnd", (long)h, "pid", (long)pid, "cls", Native.Class(h), "title", title));
            }
            return true;
        }, IntPtr.Zero);
        return found;
    }

    /// The window's text: the TextPattern document of its focused element
    /// (when it's in this window), else of the first element with one, else
    /// the first ValuePattern's value.
    static Dictionary<string, object> ReadText(IntPtr h) {
        var root = AutomationElement.FromHandle(h);
        AutomationElement el = null;
        try {
            var f = AutomationElement.FocusedElement;
            if (f != null && Native.Root(new IntPtr(f.Current.NativeWindowHandle)) == h && Supports(f, TextPattern.Pattern)) el = f;
        } catch (Exception) { }
        if (el == null) {
            // A terminal's buffer (Windows Terminal's TermControl), else a
            // document or edit field, else whatever text there is (a tab's
            // title is a TextPattern too).
            var all = root.FindAll(TreeScope.Subtree, new PropertyCondition(AutomationElement.IsTextPatternAvailableProperty, true));
            foreach (AutomationElement e in all) if (e.Current.ClassName == "TermControl") { el = e; break; }
            if (el == null) foreach (AutomationElement e in all) {
                var t = e.Current.ControlType;
                if (t == ControlType.Document || t == ControlType.Edit) { el = e; break; }
            }
            if (el == null && all.Count > 0) el = all[0];
        }
        if (el != null) {
            var tp = (TextPattern)el.GetCurrentPattern(TextPattern.Pattern);
            return Obj("text", tp.DocumentRange.GetText(-1), "via", "TextPattern", "control", el.Current.ControlType.ProgrammaticName);
        }
        var v = root.FindFirst(TreeScope.Subtree, new PropertyCondition(AutomationElement.IsValuePatternAvailableProperty, true));
        if (v != null) {
            var vp = (ValuePattern)v.GetCurrentPattern(ValuePattern.Pattern);
            return Obj("text", vp.Current.Value, "via", "ValuePattern", "control", v.Current.ControlType.ProgrammaticName);
        }
        return Obj("error", "no text in that window over UI Automation");
    }

    /// What UI Automation says is around the caret in the focused element
    /// of the test's window (in front, titled with the nonce): the way
    /// pasting/selection.rs reads it (TextPattern selection, 500 characters either
    /// side), for diagnosing a fit.
    static Dictionary<string, object> Caret(IntPtr h, string nonce) {
        if (!Ours(h, nonce)) return Obj("error", "not the test's window");
        if (Native.Root(Native.GetForegroundWindow()) != h) return Obj("error", "the test's window isn't in front");
        var f = AutomationElement.FocusedElement;
        var info = Obj("control", f.Current.ControlType.ProgrammaticName, "class", f.Current.ClassName,
            "name", f.Current.Name);
        object o;
        if (!f.TryGetCurrentPattern(TextPattern.Pattern, out o)) {
            info["pattern"] = "no TextPattern";
            if (f.TryGetCurrentPattern(ValuePattern.Pattern, out o)) info["value"] = ((ValuePattern)o).Current.Value;
            return info;
        }
        var tp = (TextPattern)o;
        var sel = tp.GetSelection();
        info["ranges"] = sel.Length;
        if (sel.Length == 0) return info;
        var range = sel[0];
        var before = range.Clone();
        before.MoveEndpointByRange(TextPatternRangeEndpoint.End, range, TextPatternRangeEndpoint.Start);
        before.MoveEndpointByUnit(TextPatternRangeEndpoint.Start, TextUnit.Character, -501);
        var after = range.Clone();
        after.MoveEndpointByRange(TextPatternRangeEndpoint.Start, range, TextPatternRangeEndpoint.End);
        after.MoveEndpointByUnit(TextPatternRangeEndpoint.End, TextUnit.Character, 501);
        info["selection"] = range.GetText(1000);
        info["before"] = before.GetText(1000);
        info["after"] = after.GetText(1000);
        info["document"] = tp.DocumentRange.GetText(1000);
        return info;
    }

    static bool Supports(AutomationElement el, AutomationPattern p) {
        object o;
        return el.TryGetCurrentPattern(p, out o);
    }

    static bool Ours(IntPtr h, string nonce) {
        return nonce.Length >= 12 && Native.IsWindow(h) && Native.Title(h).Contains(nonce);
    }

    static Dictionary<string, object> PressKey(IntPtr h, string nonce, string key) {
        if (!Ours(h, nonce)) return Obj("ok", false, "error", "not the test's window");
        if (Native.Root(Native.GetForegroundWindow()) != h) return Obj("ok", false, "error", "the test's window isn't in front");
        ushort mod = 0, vk;
        switch (key) {
            case "escape": vk = 0x1B; break;
            case "enter": vk = 0x0D; break;
            case "ctrl+s": mod = 0x11; vk = 0x53; break;
            case "ctrl+w": mod = 0x11; vk = 0x57; break;
            case "ctrl+end": mod = 0x11; vk = 0x23; break;
            case "ctrl+v": mod = 0x11; vk = 0x56; break;
            case "ctrl+a": mod = 0x11; vk = 0x41; break;
            default: return Obj("ok", false, "error", "unknown key " + key);
        }
        var inputs = new List<Native.INPUT>();
        if (mod != 0) inputs.Add(Key(mod, false));
        inputs.Add(Key(vk, false));
        inputs.Add(Key(vk, true));
        if (mod != 0) inputs.Add(Key(mod, true));
        // Checked again right before the keys go out.
        if (Native.Root(Native.GetForegroundWindow()) != h || !Ours(h, nonce)) return Obj("ok", false, "error", "the test's window lost the focus");
        uint sent = Native.SendInput((uint)inputs.Count, inputs.ToArray(), Marshal.SizeOf(typeof(Native.INPUT)));
        Thread.Sleep(50);
        return Obj("ok", sent == inputs.Count);
    }

    /// Keys for the robot's own Yap's input hook (input_hook.rs, robot
    /// mode): each marked as the robot's own (ROBOT_MARK), or a key-up
    /// marked for the hook to miss (UNHEARD_MARK). `seq`: "d:135,w:300,u:135"
    /// (d/u down/up, U up unheard, w wait ms). Only while that window is in
    /// front and titled with the nonce — checked before every key.
    static Dictionary<string, object> HookKeys(IntPtr h, string nonce, string seq) {
        const long MARK = 0x59415052, UNHEARD = 0x59415055;
        if (!Ours(h, nonce)) return Obj("ok", false, "error", "not the test's window");
        int sent = 0;
        foreach (var step in seq.Split(',')) {
            var parts = step.Split(':');
            string op = parts[0];
            int n = int.Parse(parts[1]);
            if (op == "w") { Thread.Sleep(n); continue; }
            if (op != "d" && op != "u" && op != "U") return Obj("ok", false, "error", "unknown step " + step, "sent", sent);
            if (Native.Root(Native.GetForegroundWindow()) != h) return Obj("ok", false, "error", "the test's window isn't in front", "sent", sent);
            var i = new Native.INPUT { type = 1 };
            i.u.ki = new Native.KEYBDINPUT { wVk = (ushort)n, dwFlags = op == "d" ? 0u : 2u, dwExtraInfo = new IntPtr(op == "U" ? UNHEARD : MARK) };
            sent += (int)Native.SendInput(1, new[] { i }, Marshal.SizeOf(typeof(Native.INPUT)));
        }
        return Obj("ok", true, "sent", sent);
    }

    // ---- right-clicking the robot's own Yap (shell/context_menu.rs) ----

    /// The processes of Yap's tree: the robot's Yap and every process it
    /// started, WebView2's browser and renderers included.
    static HashSet<uint> Tree(uint root) {
        var parent = new Dictionary<uint, uint>();
        IntPtr snap = Native.CreateToolhelp32Snapshot(2, 0); // TH32CS_SNAPPROCESS
        var e = new Native.PROCESSENTRY32W { dwSize = (uint)Marshal.SizeOf(typeof(Native.PROCESSENTRY32W)) };
        if (Native.Process32FirstW(snap, ref e)) {
            do { parent[e.th32ProcessID] = e.th32ParentProcessID; } while (Native.Process32NextW(snap, ref e));
        }
        Native.CloseHandle(snap);
        var tree = new HashSet<uint> { root };
        bool grew = true;
        while (grew) {
            grew = false;
            foreach (var kv in parent) if (!tree.Contains(kv.Key) && tree.Contains(kv.Value)) { tree.Add(kv.Key); grew = true; }
        }
        return tree;
    }

    /// The visible top-level windows of those processes (handle → class).
    static Dictionary<long, string> Shown(HashSet<uint> pids) {
        var shown = new Dictionary<long, string>();
        Native.EnumWindows(delegate (IntPtr h, IntPtr l) {
            uint pid;
            Native.GetWindowThreadProcessId(h, out pid);
            if (pids.Contains(pid) && Native.IsWindowVisible(h)) shown[(long)h] = Native.Class(h);
            return true;
        }, IntPtr.Zero);
        return shown;
    }

    static Dictionary<string, object> YapWindow(uint pid) {
        Native.SetProcessDpiAwarenessContext(new IntPtr(-4)); // per-monitor v2
        IntPtr best = IntPtr.Zero;
        long area = 0;
        Native.EnumWindows(delegate (IntPtr h, IntPtr l) {
            uint p;
            Native.GetWindowThreadProcessId(h, out p);
            if (p != pid || !Native.IsWindowVisible(h)) return true;
            Native.RECT r;
            Native.GetWindowRect(h, out r);
            long a = (long)(r.Right - r.Left) * (r.Bottom - r.Top);
            if (a > area) { area = a; best = h; }
            return true;
        }, IntPtr.Zero);
        if (best == IntPtr.Zero) return Obj("error", "no visible window of that process");
        return Obj("hwnd", (long)best, "title", Native.Title(best));
    }

    static Dictionary<string, object> RightClick(IntPtr h, uint pid, double x, double y, bool shift) {
        Native.SetProcessDpiAwarenessContext(new IntPtr(-4)); // per-monitor v2: physical pixels
        uint owner;
        Native.GetWindowThreadProcessId(h, out owner);
        if (owner != pid) return Obj("error", "not the robot's Yap");
        var focus = Focus(h);
        if (!(bool)focus["ok"]) return Obj("error", "Yap's window didn't come to the front");
        double scale = Native.GetDpiForWindow(h) / 96.0;
        var pt = new Native.POINT();
        Native.ClientToScreen(h, ref pt);
        pt.X += (int)Math.Round(x * scale);
        pt.Y += (int)Math.Round(y * scale);
        if (Native.Root(Native.WindowFromPoint(pt)) != h) return Obj("error", "the point isn't on Yap's window");
        var pids = Tree(pid);
        var before = Shown(pids);
        Native.POINT back;
        Native.GetCursorPos(out back);
        // Checked again right before the click goes out.
        if (Native.Root(Native.GetForegroundWindow()) != h) return Obj("error", "Yap's window lost the focus");
        Native.SetCursorPos(pt.X, pt.Y);
        Thread.Sleep(60);
        // Shift down first (a developer's Shift+right-click), up once the
        // menu had its time to come.
        if (shift) {
            Native.SendInput(1, new[] { Key(0x10, false) }, Marshal.SizeOf(typeof(Native.INPUT)));
            Thread.Sleep(40);
        }
        var click = new Native.INPUT[2];
        click[0].type = 0;
        click[0].u.mi = new Native.MOUSEINPUT { dwFlags = 0x0008 }; // right down
        click[1].type = 0;
        click[1].u.mi = new Native.MOUSEINPUT { dwFlags = 0x0010 }; // right up
        Native.SendInput(2, click, Marshal.SizeOf(typeof(Native.INPUT)));
        Thread.Sleep(900);
        if (shift) Native.SendInput(1, new[] { Key(0x10, true) }, Marshal.SizeOf(typeof(Native.INPUT)));
        var after = Shown(pids);
        var came = new List<object>();
        foreach (var kv in after) if (!before.ContainsKey(kv.Key)) {
            Native.RECT r;
            Native.GetWindowRect(new IntPtr(kv.Key), out r);
            came.Add(Obj("cls", kv.Value, "w", r.Right - r.Left, "h", r.Bottom - r.Top));
        }
        bool closed = true;
        if (came.Count > 0) {
            // Esc closes a menu (it has the keyboard while it's up).
            Native.SendInput(2, new[] { Key(0x1B, false), Key(0x1B, true) }, Marshal.SizeOf(typeof(Native.INPUT)));
            Thread.Sleep(400);
            var left = Shown(pids);
            foreach (var kv in left) if (!before.ContainsKey(kv.Key)) closed = false;
        }
        Native.SetCursorPos(back.X, back.Y);
        return Obj("ok", true, "popups", came, "closed", closed, "x", pt.X, "y", pt.Y);
    }

    /// A real left click on the robot's own Yap bar, a click-through window
    /// (bar.rs): the pointer goes to (x, y) in physical px and rests
    /// `hoverMs` so the bar's poller can make that spot clickable; the click
    /// goes out only when Windows puts a window of `pid` under the pointer,
    /// else nothing is clicked and the process that is there is named. The
    /// app in front is read before and after (the bar must never take the
    /// focus). The pointer goes back where it was.
    static Dictionary<string, object> BarClick(uint pid, int x, int y, int hoverMs) {
        Native.SetProcessDpiAwarenessContext(new IntPtr(-4)); // per-monitor v2: physical pixels
        Native.POINT back;
        Native.GetCursorPos(out back);
        IntPtr fgBefore = Native.Root(Native.GetForegroundWindow());
        Native.SetCursorPos(x, y);
        Thread.Sleep(hoverMs);
        var pt = new Native.POINT();
        pt.X = x;
        pt.Y = y;
        uint underPid;
        Native.GetWindowThreadProcessId(Native.Root(Native.WindowFromPoint(pt)), out underPid);
        if (underPid != pid) {
            Native.SetCursorPos(back.X, back.Y);
            return Obj("ok", false, "underPid", (long)underPid, "reason", "the spot isn't on the robot's Yap: a click would go to another app");
        }
        var click = new Native.INPUT[2];
        click[0].type = 0;
        click[0].u.mi = new Native.MOUSEINPUT { dwFlags = 0x0002 }; // left down
        click[1].type = 0;
        click[1].u.mi = new Native.MOUSEINPUT { dwFlags = 0x0004 }; // left up
        Native.SendInput(2, click, Marshal.SizeOf(typeof(Native.INPUT)));
        Thread.Sleep(400);
        IntPtr fgAfter = Native.Root(Native.GetForegroundWindow());
        Native.SetCursorPos(back.X, back.Y);
        return Obj("ok", true, "underPid", (long)underPid, "fgBefore", (long)fgBefore, "fgAfter", (long)fgAfter);
    }

    /// Move a window the test opened (titled with the nonce) to (x, y).
    static Dictionary<string, object> Place(IntPtr h, string nonce, int x, int y) {
        Native.SetProcessDpiAwarenessContext(new IntPtr(-4));
        if (nonce.Length < 12 || !Native.Title(h).Contains(nonce)) return Obj("ok", false, "reason", "not the test's window");
        bool ok = Native.SetWindowPos(h, IntPtr.Zero, x, y, 0, 0, 0x0001 | 0x0004 | 0x0010); // NOSIZE | NOZORDER | NOACTIVATE
        Thread.Sleep(300);
        return Obj("ok", ok);
    }

    /// A browser window's page area on screen (physical pixels): its
    /// Chrome_RenderWidgetHostHWND child.
    static Dictionary<string, object> ContentRect(IntPtr h) {
        Native.SetProcessDpiAwarenessContext(new IntPtr(-4));
        IntPtr found = IntPtr.Zero;
        Native.EnumChildWindows(h, delegate (IntPtr c, IntPtr l) {
            if (Native.Class(c) == "Chrome_RenderWidgetHostHWND" && Native.IsWindowVisible(c)) { found = c; return false; }
            return true;
        }, IntPtr.Zero);
        if (found == IntPtr.Zero) return Obj("error", "no page area");
        Native.RECT r;
        Native.GetWindowRect(found, out r);
        return Obj("x", r.Left, "y", r.Top, "w", r.Right - r.Left, "h", r.Bottom - r.Top);
    }

    /// The snip overlay's window under (x, y), when it's the robot's own Yap's.
    static bool SnipOverlayAt(uint pid, int x, int y) {
        var pt = new Native.POINT();
        pt.X = x;
        pt.Y = y;
        IntPtr under = Native.Root(Native.WindowFromPoint(pt));
        uint underPid;
        Native.GetWindowThreadProcessId(under, out underPid);
        return underPid == pid && Native.Class(under) == "YapSnipOverlay";
    }

    /// Whether the robot's own snip overlay is the window in front (a key
    /// pressed now goes to it, nowhere else).
    static bool SnipOverlayInFront(uint pid) {
        IntPtr fg = Native.Root(Native.GetForegroundWindow());
        uint fgPid;
        Native.GetWindowThreadProcessId(fg, out fgPid);
        return fgPid == pid && Native.Class(fg) == "YapSnipOverlay";
    }

    /// A real drag over the robot's own snip overlay: down at (x0, y0), a
    /// few moves, up at (x1, y1) — the same point twice is a click. Refused
    /// unless that overlay is under the pointer at the start (a drag
    /// anywhere else would go to another app). `mod` "ctrl" / "shift": held
    /// as the button is let go (Snip to Chat's copy / save), only while the
    /// overlay is in front.
    static Dictionary<string, object> SnipDrag(uint pid, int x0, int y0, int x1, int y1, string mod) {
        Native.SetProcessDpiAwarenessContext(new IntPtr(-4)); // per-monitor v2: physical pixels
        ushort modVk = mod == "ctrl" ? (ushort)0x11 : mod == "shift" ? (ushort)0x10 : (ushort)0;
        if (mod != "" && modVk == 0) return Obj("ok", false, "reason", "only ctrl or shift");
        Native.POINT back;
        Native.GetCursorPos(out back);
        Native.SetCursorPos(x0, y0);
        Thread.Sleep(80);
        if (!SnipOverlayAt(pid, x0, y0)) {
            Native.SetCursorPos(back.X, back.Y);
            return Obj("ok", false, "reason", "the robot's snip overlay isn't under the pointer");
        }
        if (modVk != 0 && !SnipOverlayInFront(pid)) {
            Native.SetCursorPos(back.X, back.Y);
            return Obj("ok", false, "reason", "the snip overlay isn't in front");
        }
        var size = Marshal.SizeOf(typeof(Native.INPUT));
        var down = new Native.INPUT[1];
        down[0].type = 0;
        down[0].u.mi = new Native.MOUSEINPUT { dwFlags = 0x0002 };
        Native.SendInput(1, down, size);
        for (int i = 1; i <= 8; i++) {
            Thread.Sleep(15);
            Native.SetCursorPos(x0 + (x1 - x0) * i / 8, y0 + (y1 - y0) * i / 8);
        }
        if (modVk != 0) {
            Native.SendInput(1, new[] { Key(modVk, false) }, size);
            Thread.Sleep(30);
            // A move with the key held, as a person's hand would make.
            Native.SetCursorPos(x1, y1);
        }
        Thread.Sleep(40);
        var up = new Native.INPUT[1];
        up[0].type = 0;
        up[0].u.mi = new Native.MOUSEINPUT { dwFlags = 0x0004 };
        Native.SendInput(1, up, size);
        if (modVk != 0) {
            Thread.Sleep(20);
            Native.SendInput(1, new[] { Key(modVk, true) }, size);
        }
        Thread.Sleep(100);
        Native.SetCursorPos(back.X, back.Y);
        return Obj("ok", true);
    }

    /// A key (escape, enter, space) while the robot's own snip overlay is in
    /// front (else nothing); with (x, y), the pointer moved there first —
    /// only onto that overlay — and left there (Enter takes the monitor
    /// under it, Space lights the window under it).
    static Dictionary<string, object> SnipKey(uint pid, string key, int x = int.MinValue, int y = int.MinValue) {
        ushort vk = key == "escape" ? (ushort)0x1B : key == "enter" ? (ushort)0x0D : key == "space" ? (ushort)0x20 : (ushort)0;
        if (vk == 0) return Obj("ok", false, "reason", "only escape, enter or space");
        if (!SnipOverlayInFront(pid)) {
            IntPtr fg = Native.Root(Native.GetForegroundWindow());
            uint fgPid;
            Native.GetWindowThreadProcessId(fg, out fgPid);
            return Obj("ok", false, "reason", "the snip overlay isn't in front", "fgIsYap", fgPid == pid);
        }
        if (x != int.MinValue) {
            Native.SetProcessDpiAwarenessContext(new IntPtr(-4));
            Native.POINT back;
            Native.GetCursorPos(out back);
            Native.SetCursorPos(x, y);
            Thread.Sleep(80);
            if (!SnipOverlayAt(pid, x, y)) {
                Native.SetCursorPos(back.X, back.Y);
                return Obj("ok", false, "reason", "the robot's snip overlay isn't under the pointer");
            }
            // A small move so the overlay sees where the pointer is.
            Native.SetCursorPos(x + 1, y);
            Thread.Sleep(30);
            Native.SetCursorPos(x, y);
            Thread.Sleep(50);
        }
        Native.SendInput(2, new[] { Key(vk, false), Key(vk, true) }, Marshal.SizeOf(typeof(Native.INPUT)));
        return Obj("ok", true);
    }

    /// A window's visible frame on screen (DWM's extended frame bounds, no
    /// invisible resize borders), physical pixels: what Snip to Chat's
    /// window pick takes.
    static Dictionary<string, object> FrameRect(IntPtr h) {
        Native.SetProcessDpiAwarenessContext(new IntPtr(-4));
        Native.RECT r;
        int hr = Native.DwmGetWindowAttribute(h, 9, out r, Marshal.SizeOf(typeof(Native.RECT)));
        if (hr != 0) return Obj("error", "no frame bounds");
        return Obj("x", r.Left, "y", r.Top, "w", r.Right - r.Left, "h", r.Bottom - r.Top);
    }

    /// F12 in the robot's own Yap window (in front): what came up in Yap's
    /// process tree (DevTools, in a debug build) — closed again.
    static Dictionary<string, object> DevTools(IntPtr h, uint pid) {
        uint owner;
        Native.GetWindowThreadProcessId(h, out owner);
        if (owner != pid) return Obj("error", "not the robot's Yap");
        var focus = Focus(h);
        if (!(bool)focus["ok"]) return Obj("error", "Yap's window didn't come to the front");
        var pids = Tree(pid);
        var before = Shown(pids);
        if (Native.Root(Native.GetForegroundWindow()) != h) return Obj("error", "Yap's window lost the focus");
        Native.SendInput(2, new[] { Key(0x7B, false), Key(0x7B, true) }, Marshal.SizeOf(typeof(Native.INPUT)));
        var came = new List<object>();
        var opened = new List<IntPtr>();
        for (int i = 0; i < 40 && opened.Count == 0; i++) {
            Thread.Sleep(100);
            foreach (var kv in Shown(pids)) if (!before.ContainsKey(kv.Key)) {
                Native.RECT r;
                Native.GetWindowRect(new IntPtr(kv.Key), out r);
                came.Add(Obj("cls", kv.Value, "w", r.Right - r.Left, "h", r.Bottom - r.Top));
                opened.Add(new IntPtr(kv.Key));
            }
        }
        foreach (var w in opened) Native.PostMessage(w, 0x0010, IntPtr.Zero, IntPtr.Zero);
        bool closed = true;
        for (int i = 0; i < 30; i++) {
            Thread.Sleep(100);
            closed = true;
            foreach (var w in opened) if (Native.IsWindow(w) && Native.IsWindowVisible(w)) closed = false;
            if (closed) break;
        }
        return Obj("ok", true, "popups", came, "closed", closed);
    }

    static Native.INPUT Key(ushort vk, bool up) {
        var i = new Native.INPUT { type = 1 };
        i.u.ki = new Native.KEYBDINPUT { wVk = vk, dwFlags = up ? 2u : 0u };
        return i;
    }

    [DllImport("user32.dll")] static extern short GetAsyncKeyState(int vk);

    static readonly string[] ModNames = { "lshift", "rshift", "lctrl", "rctrl", "lalt", "ralt", "lwin", "rwin" };
    static readonly ushort[] ModKeys = { 0xA0, 0xA1, 0xA2, 0xA3, 0xA4, 0xA5, 0x5B, 0x5C };

    /// Which modifier keys Windows says are down (its key state, nothing
    /// else: no window is read).
    static Dictionary<string, object> KeyState() {
        var down = new List<object>();
        for (int i = 0; i < ModKeys.Length; i++) if (GetAsyncKeyState(ModKeys[i]) < 0) down.Add(ModNames[i]);
        return Obj("down", down);
    }

    /// A modifier key (`lalt`, `lshift`, `lctrl`, `lwin`…) pressed or let
    /// go, as an injected key — "stuck" as far as Windows knows, no finger
    /// on it. Pressing only while the test's window is in front (`owner`:
    /// the run's nonce in its title, or `pid:<n>`, its process: the test
    /// pad's); letting go always (the cleanup), with the menu mask key
    /// tapped first so an Alt or Win release opens no menu or Start.
    static Dictionary<string, object> ModKey(IntPtr h, string owner, string name, string how) {
        int at = Array.IndexOf(ModNames, name);
        if (at < 0) return Obj("ok", false, "error", "unknown key " + name);
        bool up = how == "up";
        if (!up) {
            bool ours;
            if (owner.StartsWith("pid:")) {
                uint pid;
                Native.GetWindowThreadProcessId(h, out pid);
                ours = Native.IsWindow(h) && pid.ToString() == owner.Substring(4);
            } else {
                ours = Ours(h, owner);
            }
            if (!ours) return Obj("ok", false, "error", "not the test's window");
            if (Native.Root(Native.GetForegroundWindow()) != h) return Obj("ok", false, "error", "the test's window isn't in front");
        }
        var inputs = new List<Native.INPUT>();
        if (up && at >= 4) { inputs.Add(Key(0xE8, false)); inputs.Add(Key(0xE8, true)); }
        inputs.Add(Key(ModKeys[at], up));
        uint sent = Native.SendInput((uint)inputs.Count, inputs.ToArray(), Marshal.SizeOf(typeof(Native.INPUT)));
        Thread.Sleep(30);
        return Obj("ok", sent == inputs.Count);
    }

    /// WM_CLOSE; and if the window asks first (Windows Terminal's "Do you
    /// want to close all tabs?"), its "Close all" over UI Automation.
    static Dictionary<string, object> Close(IntPtr h, string nonce) {
        if (!Ours(h, nonce)) return Obj("ok", false, "error", "not the test's window");
        var root = AutomationElement.FromHandle(h);
        Native.PostMessage(h, 0x0010, IntPtr.Zero, IntPtr.Zero);
        for (int i = 0; i < 20 && Native.IsWindow(h); i++) {
            Thread.Sleep(100);
            if (i % 5 != 4) continue;
            try {
                var btn = root.FindFirst(TreeScope.Descendants,
                    new PropertyCondition(AutomationElement.NameProperty, "Close all"));
                if (btn != null) ((InvokePattern)btn.GetCurrentPattern(InvokePattern.Pattern)).Invoke();
            } catch (Exception) { }
        }
        return Obj("ok", !Native.IsWindow(h));
    }
}

/// The test pad: what a dictation pastes into, and what it saw.
class TestPad : Form {
    static readonly JavaScriptSerializer Json = new JavaScriptSerializer { MaxJsonLength = int.MaxValue };
    static readonly string[] Private = {
        "ExcludeClipboardContentFromMonitorProcessing",
        "CanIncludeInClipboardHistory",
        "CanUploadToCloudClipboard",
        "Clipboard Viewer Ignore",
    };

    readonly PadBox box;
    readonly PadRich rich;
    readonly PadBox password;
    readonly PadBox nopaste;
    readonly Button button;
    readonly SlowBox slow;
    readonly List<object> pastes = new List<object>();
    // The clipboard held open (a busy clipboard manager's stand-in).
    System.Windows.Forms.Timer holding;
    int holdAfterCopyMs;
    System.Media.SoundPlayer player;

    public static int Run(string label) {
        Application.EnableVisualStyles();
        var pad = new TestPad(label);
        var reader = new Thread(delegate () {
            string line;
            while ((line = Console.In.ReadLine()) != null) {
                Dictionary<string, object> reply;
                try {
                    var req = Json.Deserialize<Dictionary<string, object>>(line);
                    reply = (Dictionary<string, object>)pad.Invoke(new Func<Dictionary<string, object>>(delegate () { return pad.Dispatch(req); }));
                } catch (Exception e) {
                    reply = new Dictionary<string, object> { { "error", e.Message } };
                }
                Console.Out.WriteLine(Json.Serialize(reply));
                Console.Out.Flush();
            }
            // The harness closed our stdin (it finished, or died): go.
            try { pad.BeginInvoke(new Action(pad.Close)); } catch (Exception) { }
        });
        reader.IsBackground = true;
        pad.Shown += delegate { reader.Start(); };
        Application.Run(pad);
        return 0;
    }

    TestPad(string label) {
        Text = "Yap robot test pad " + label;
        StartPosition = FormStartPosition.Manual;
        Location = new System.Drawing.Point(60, 60);
        ClientSize = new System.Drawing.Size(560, 424);
        box = new PadBox { Name = "box", Multiline = true, AcceptsReturn = true, ScrollBars = ScrollBars.Vertical,
            Location = new System.Drawing.Point(10, 10), Size = new System.Drawing.Size(540, 160) };
        rich = new PadRich { Name = "rich", Location = new System.Drawing.Point(10, 180), Size = new System.Drawing.Size(540, 160) };
        password = new PadBox { Name = "password", UseSystemPasswordChar = true,
            Location = new System.Drawing.Point(10, 350), Size = new System.Drawing.Size(540, 24) };
        // A text box that ignores Ctrl+V (its keyboard shortcuts off), as if
        // every paste into it were lost: typing still works. For Yap's check
        // that a paste landed (it types the text instead).
        nopaste = new PadBox { Name = "nopaste", ShortcutsEnabled = false,
            Location = new System.Drawing.Point(10, 390), Size = new System.Drawing.Size(540, 24) };
        // A button: a focus that takes no text ("Select a text box first").
        button = new Button { Name = "button", Text = "A button",
            Location = new System.Drawing.Point(10, 424), Size = new System.Drawing.Size(140, 30) };
        // A box that's no edit control and takes a paste late (`slowPaste`).
        slow = new SlowBox { Name = "slow", Location = new System.Drawing.Point(160, 424), Size = new System.Drawing.Size(390, 30) };
        ClientSize = new System.Drawing.Size(560, 464);
        foreach (Control c in new Control[] { box, rich, password, nopaste, button, slow }) {
            c.Font = new System.Drawing.Font("Segoe UI", 11f);
            Controls.Add(c);
        }
        box.Pasting += Record;
        box.Copying = CopyThenHold;
        rich.Pasting += Record;
        password.Pasting += Record;
        ActiveControl = box;
    }

    [DllImport("user32.dll")] static extern bool IsClipboardFormatAvailable(uint format);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern uint RegisterClipboardFormat(string name);
    [DllImport("user32.dll")] static extern IntPtr GetOpenClipboardWindow();
    [DllImport("user32.dll")] static extern bool OpenClipboard(IntPtr owner);
    [DllImport("user32.dll")] static extern bool CloseClipboard();

    /// Open the clipboard (on the pad's own thread) and close it `ms` later.
    bool Hold(int ms) {
        if (holding != null) return true;
        if (!OpenClipboard(Handle)) return false;
        holding = new System.Windows.Forms.Timer { Interval = Math.Max(1, ms) };
        holding.Tick += delegate {
            holding.Stop();
            holding.Dispose();
            holding = null;
            CloseClipboard();
        };
        holding.Start();
        return true;
    }

    /// Ctrl+C in a box while asked to hold the clipboard after a copy: the
    /// copy, then the clipboard held at once (before anyone else can open it).
    bool CopyThenHold(string field) {
        if (holdAfterCopyMs <= 0) return false;
        ((TextBoxBase)Controls[field]).Copy();
        int ms = holdAfterCopyMs;
        holdAfterCopyMs = 0;
        Hold(ms);
        return true;
    }

    protected override void OnFormClosed(FormClosedEventArgs e) {
        if (holding != null) { holding.Stop(); holding = null; CloseClipboard(); }
        base.OnFormClosed(e);
    }

    /// Ctrl+V reached a field: which formats the clipboard held at that
    /// moment (never the text), whether another window had the clipboard
    /// open (a paste then silently does nothing), and — once the field has
    /// handled the key — whether its text changed. IsClipboardFormatAvailable
    /// and GetOpenClipboardWindow don't open the clipboard, so this can't
    /// hold the paste up.
    void Record(string field) {
        var priv = new Dictionary<string, object>();
        foreach (var name in Private) priv[name] = IsClipboardFormatAvailable(RegisterClipboardFormat(name));
        var holder = GetOpenClipboardWindow();
        string heldBy = null;
        if (holder != IntPtr.Zero) {
            uint pid;
            Native.GetWindowThreadProcessId(holder, out pid);
            try { heldBy = System.Diagnostics.Process.GetProcessById((int)pid).ProcessName; } catch (Exception) { heldBy = "process " + pid; }
        }
        var box = (TextBoxBase)Controls[field];
        var entry = new Dictionary<string, object> {
            { "field", field },
            { "text", IsClipboardFormatAvailable(13) },
            { "private", priv },
            { "clipboardHeldBy", heldBy },
        };
        pastes.Add(entry);
        string before = box.Text;
        // A moment later, the field has handled the key (well inside the
        // half second before Yap puts the clipboard back).
        var later = new System.Windows.Forms.Timer { Interval = 300 };
        later.Tick += delegate {
            later.Stop();
            later.Dispose();
            entry["pasted"] = box.Text != before;
            if (box.Text == before) {
                Console.Error.WriteLine("test pad: Ctrl+V reached the " + field + " but nothing was pasted" +
                    (heldBy != null ? " (the clipboard was held open by " + heldBy + ")" : ""));
            }
        };
        later.Start();
    }

    Control Field(Dictionary<string, object> req) {
        object f;
        string name = req.TryGetValue("field", out f) ? (string)f : "box";
        switch (name) {
            case "box": return box;
            case "rich": return rich;
            case "password": return password;
            case "nopaste": return nopaste;
            case "button": return button;
            case "slow": return slow;
            default: throw new ArgumentException("no field " + name);
        }
    }

    Dictionary<string, object> Dispatch(Dictionary<string, object> req) {
        var cmd = (string)req["cmd"];
        var reply = new Dictionary<string, object> { { "ok", true } };
        switch (cmd) {
            case "hwnd": {
                reply["hwnd"] = (long)Handle;
                reply["pid"] = System.Diagnostics.Process.GetCurrentProcess().Id;
                break;
            }
            case "set": {
                object t, c;
                if (Field(req) == slow) {
                    slow.Contents = req.TryGetValue("text", out t) && t != null ? (string)t : "";
                    ActiveControl = slow;
                    slow.Focus();
                    break;
                }
                var field = (TextBoxBase)Field(req);
                string text = req.TryGetValue("text", out t) && t != null ? (string)t : "";
                field.Text = text;
                int caret = req.TryGetValue("caret", out c) && c != null ? Convert.ToInt32(c) : field.TextLength;
                field.Select(Math.Min(caret, field.TextLength), 0);
                ActiveControl = field;
                field.Focus();
                break;
            }
            case "select": {
                var field = (TextBoxBase)Field(req);
                field.Select(Convert.ToInt32(req["start"]), Convert.ToInt32(req["length"]));
                ActiveControl = field;
                field.Focus();
                break;
            }
            case "focus": {
                var field = Field(req);
                ActiveControl = field;
                field.Focus();
                break;
            }
            case "get": {
                if (Field(req) == slow) {
                    reply["text"] = slow.Contents;
                    reply["caret"] = slow.Contents.Length;
                    break;
                }
                var field = (TextBoxBase)Field(req);
                reply["text"] = field.Text;
                reply["caret"] = field.SelectionStart;
                break;
            }
            case "slowPaste": {
                // The slow box's Ctrl+V reads the clipboard `ms` later; never
                // when `ms` < 0.
                slow.DelayMs = Convert.ToInt32(req["ms"]);
                break;
            }
            case "pastes": {
                reply["pastes"] = new List<object>(pastes);
                pastes.Clear();
                break;
            }
            case "play": {
                // A WAV on a loop from this process: an audio session of the
                // test's own for "Silence other apps while you dictate".
                if (player != null) player.Stop();
                player = new System.Media.SoundPlayer((string)req["path"]);
                player.Load();
                player.PlayLooping();
                break;
            }
            case "stop": {
                if (player != null) { player.Stop(); player.Dispose(); player = null; }
                break;
            }
            case "holdClipboard": {
                // The clipboard open for `ms` (as a busy clipboard manager
                // holds it): nobody else can read or write it meanwhile.
                reply["held"] = Hold(Convert.ToInt32(req["ms"]));
                break;
            }
            case "holdAfterCopy": {
                // At the box's next Ctrl+C: its copy, then the clipboard
                // held for `ms`.
                holdAfterCopyMs = Convert.ToInt32(req["ms"]);
                reply["listening"] = true;
                break;
            }
            case "clipboardHeld": {
                reply["held"] = holding != null;
                break;
            }
            default: throw new ArgumentException("unknown command " + cmd);
        }
        return reply;
    }
}

class PadBox : TextBox {
    public event Action<string> Pasting;
    /// Ctrl+C: the pad's own copy, when it wants to hold the clipboard after it.
    public Func<string, bool> Copying;
    protected override bool ProcessCmdKey(ref Message msg, Keys keyData) {
        if (keyData == (Keys.Control | Keys.V) && Pasting != null) Pasting(Name);
        if (keyData == (Keys.Control | Keys.C) && Copying != null && Copying(Name)) return true;
        return base.ProcessCmdKey(ref msg, keyData);
    }
}

class PadRich : RichTextBox {
    public event Action<string> Pasting;
    protected override bool ProcessCmdKey(ref Message msg, Keys keyData) {
        if (keyData == (Keys.Control | Keys.V) && Pasting != null) Pasting(Name);
        return base.ProcessCmdKey(ref msg, keyData);
    }
}

/// The test pad's box that's no edit control (UI Automation sees a pane):
/// Ctrl+V reads the clipboard `DelayMs` later and adds what it got (an app
/// slow to take a paste), or never reads it (`DelayMs` < 0).
class SlowBox : Control {
    public int DelayMs;
    string contents = "";
    public string Contents {
        get { return contents; }
        set { contents = value ?? ""; Invalidate(); }
    }

    public SlowBox() {
        SetStyle(ControlStyles.Selectable | ControlStyles.UserPaint | ControlStyles.AllPaintingInWmPaint, true);
        TabStop = true;
        BackColor = System.Drawing.Color.White;
    }

    protected override void OnMouseDown(MouseEventArgs e) {
        Focus();
        base.OnMouseDown(e);
    }

    protected override void OnGotFocus(EventArgs e) { Invalidate(); base.OnGotFocus(e); }
    protected override void OnLostFocus(EventArgs e) { Invalidate(); base.OnLostFocus(e); }

    protected override void OnPaint(PaintEventArgs e) {
        e.Graphics.DrawRectangle(Focused ? System.Drawing.Pens.SteelBlue : System.Drawing.Pens.Gray, 0, 0, Width - 1, Height - 1);
        TextRenderer.DrawText(e.Graphics, contents.Length > 0 ? contents : "(slow box)", Font, new System.Drawing.Point(4, 5),
            contents.Length > 0 ? System.Drawing.Color.Black : System.Drawing.Color.Gray);
    }

    protected override bool ProcessCmdKey(ref Message msg, Keys keyData) {
        if (keyData == (Keys.Control | Keys.V)) {
            if (DelayMs >= 0) {
                var later = new System.Windows.Forms.Timer { Interval = Math.Max(1, DelayMs) };
                later.Tick += delegate {
                    later.Stop();
                    later.Dispose();
                    // The clipboard's text as it is now (OLE, as most apps read it).
                    string text = null;
                    try { if (Clipboard.ContainsText()) text = Clipboard.GetText(); } catch (Exception) { }
                    if (!string.IsNullOrEmpty(text)) Contents = contents + text;
                };
                later.Start();
            }
            return true;
        }
        return base.ProcessCmdKey(ref msg, keyData);
    }
}

/// `yaprobot clipreader <ms> <delay>`: a careless clipboard manager's
/// stand-in (it reads whatever the privacy formats say), for `ms`. Prints
/// {"listening":true} once listening, then {"updates":n,"reads":[…]} —
/// whether each read got text, and its length; never the text.
class ClipReader : NativeWindow {
    [DllImport("user32.dll")] static extern bool AddClipboardFormatListener(IntPtr h);
    [DllImport("user32.dll")] static extern bool RemoveClipboardFormatListener(IntPtr h);
    [DllImport("user32.dll")] static extern bool OpenClipboard(IntPtr owner);
    [DllImport("user32.dll")] static extern bool CloseClipboard();
    [DllImport("user32.dll")] static extern IntPtr GetClipboardData(uint format);
    [DllImport("kernel32.dll")] static extern IntPtr GlobalLock(IntPtr h);
    [DllImport("kernel32.dll")] static extern bool GlobalUnlock(IntPtr h);
    const int WM_CLIPBOARDUPDATE = 0x031D;

    readonly int delay;
    readonly List<object> reads = new List<object>();
    int updates;

    ClipReader(int delay) { this.delay = delay; }

    public static int Run(int ms, int delay) {
        var r = new ClipReader(delay);
        // A message-only window (HWND_MESSAGE), its own.
        r.CreateHandle(new CreateParams { Parent = new IntPtr(-3) });
        if (!AddClipboardFormatListener(r.Handle)) {
            Console.Out.WriteLine(new JavaScriptSerializer().Serialize(new Dictionary<string, object> { { "error", "couldn't listen to the clipboard" } }));
            return 1;
        }
        var stop = new System.Windows.Forms.Timer { Interval = Math.Max(1, ms) };
        stop.Tick += delegate { stop.Stop(); Application.ExitThread(); };
        stop.Start();
        Console.Out.WriteLine("{\"listening\":true}");
        Console.Out.Flush();
        Application.Run();
        RemoveClipboardFormatListener(r.Handle);
        Console.Out.WriteLine(new JavaScriptSerializer().Serialize(new Dictionary<string, object> {
            { "updates", r.updates }, { "reads", r.reads } }));
        Console.Out.Flush();
        r.DestroyHandle();
        return 0;
    }

    protected override void WndProc(ref Message m) {
        if (m.Msg == WM_CLIPBOARDUPDATE) {
            updates++;
            if (delay <= 0) {
                Read();
            } else {
                var later = new System.Windows.Forms.Timer { Interval = delay };
                later.Tick += delegate { later.Stop(); later.Dispose(); Read(); };
                later.Start();
            }
        }
        base.WndProc(ref m);
    }

    void Read() {
        bool open = false;
        for (int i = 0; i < 20 && !open; i++) {
            open = OpenClipboard(Handle);
            if (!open) Thread.Sleep(10);
        }
        if (!open) {
            reads.Add(new Dictionary<string, object> { { "open", false } });
            return;
        }
        IntPtr h = GetClipboardData(13);
        int len = 0;
        if (h != IntPtr.Zero) {
            IntPtr p = GlobalLock(h);
            if (p != IntPtr.Zero) {
                string s = Marshal.PtrToStringUni(p);
                len = s == null ? 0 : s.Length;
                GlobalUnlock(h);
            }
        }
        CloseClipboard();
        reads.Add(new Dictionary<string, object> { { "open", true }, { "got", h != IntPtr.Zero }, { "len", len } });
    }
}
