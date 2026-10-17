// yaprobot.exe — the real-app robot tests' Windows helper (e2e-real/,
// docs/e2e-tests.md "Real-app robot tests"). Compiled on the fly with the
// C# compiler every Windows has (.NET Framework 4, C# 5: no string
// interpolation, no `?.`), by support/targets.js.
//
//   yaprobot testpad <label>      the test pad, "Yap robot test pad <label>":
//                                 a multi-line text box, a rich text box and
//                                 a password box. Driven over stdin/stdout,
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
//   yaprobot close <hwnd> <nonce> ask a window titled with the nonce to close
//   yaprobot caret <hwnd> <nonce> what UI Automation reports around the caret
//                                 in that window (in front, titled with the
//                                 nonce), the way Yap reads it: for
//                                 diagnosing a fit
//   yaprobot yapwindow <pid>      the robot's own Yap's main window (the
//                                 largest visible window of that process)
//   yaprobot rightclick <hwnd> <pid> <x> <y>   a real right-click at CSS
//                                 point (x, y) of that Yap window (in front,
//                                 the point on it), then the windows of
//                                 Yap's process tree (WebView2's included)
//                                 that came up — a context menu — closed
//                                 again with Esc; the pointer goes back
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
                case "close": Print(Close(new IntPtr(long.Parse(args[1])), args[2])); return 0;
                case "caret": Print(Caret(new IntPtr(long.Parse(args[1])), args[2])); return 0;
                case "yapwindow": Print(YapWindow(uint.Parse(args[1]))); return 0;
                case "rightclick":
                    Print(RightClick(new IntPtr(long.Parse(args[1])), uint.Parse(args[2]),
                        double.Parse(args[3], System.Globalization.CultureInfo.InvariantCulture),
                        double.Parse(args[4], System.Globalization.CultureInfo.InvariantCulture)));
                    return 0;
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
    /// selection.rs reads it (TextPattern selection, 500 characters either
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

    // ---- right-clicking the robot's own Yap (context_menu.rs) ----

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

    static Dictionary<string, object> RightClick(IntPtr h, uint pid, double x, double y) {
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
        var click = new Native.INPUT[2];
        click[0].type = 0;
        click[0].u.mi = new Native.MOUSEINPUT { dwFlags = 0x0008 }; // right down
        click[1].type = 0;
        click[1].u.mi = new Native.MOUSEINPUT { dwFlags = 0x0010 }; // right up
        Native.SendInput(2, click, Marshal.SizeOf(typeof(Native.INPUT)));
        Thread.Sleep(900);
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

    static Native.INPUT Key(ushort vk, bool up) {
        var i = new Native.INPUT { type = 1 };
        i.u.ki = new Native.KEYBDINPUT { wVk = vk, dwFlags = up ? 2u : 0u };
        return i;
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
    readonly List<object> pastes = new List<object>();
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
        ClientSize = new System.Drawing.Size(560, 430);
        box = new PadBox { Name = "box", Multiline = true, AcceptsReturn = true, ScrollBars = ScrollBars.Vertical,
            Location = new System.Drawing.Point(10, 10), Size = new System.Drawing.Size(540, 160) };
        rich = new PadRich { Name = "rich", Location = new System.Drawing.Point(10, 180), Size = new System.Drawing.Size(540, 160) };
        password = new PadBox { Name = "password", UseSystemPasswordChar = true,
            Location = new System.Drawing.Point(10, 350), Size = new System.Drawing.Size(540, 24) };
        foreach (Control c in new Control[] { box, rich, password }) {
            c.Font = new System.Drawing.Font("Segoe UI", 11f);
            Controls.Add(c);
        }
        box.Pasting += Record;
        rich.Pasting += Record;
        password.Pasting += Record;
        ActiveControl = box;
    }

    [DllImport("user32.dll")] static extern bool IsClipboardFormatAvailable(uint format);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern uint RegisterClipboardFormat(string name);
    [DllImport("user32.dll")] static extern IntPtr GetOpenClipboardWindow();

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
                var field = (TextBoxBase)Field(req);
                object t, c;
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
                var field = (TextBoxBase)Field(req);
                reply["text"] = field.Text;
                reply["caret"] = field.SelectionStart;
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
            default: throw new ArgumentException("unknown command " + cmd);
        }
        return reply;
    }
}

class PadBox : TextBox {
    public event Action<string> Pasting;
    protected override bool ProcessCmdKey(ref Message msg, Keys keyData) {
        if (keyData == (Keys.Control | Keys.V) && Pasting != null) Pasting(Name);
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
