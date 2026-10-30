// The robot tests' hands-off guard, part of yaprobot.exe (compiled with
// yaprobot.cs by support/targets.js; support/guard.js drives it;
// docs/e2e-tests.md "Hands off: the idle check and the input watcher").
// C# 5, like yaprobot.cs: no string interpolation, no `?.`.
//
//   yaprobot idle                 how long Windows has had no input at all
//                                 (GetLastInputInfo): {"idleMs":…,"tick":…}.
//                                 The robot's own injected input counts too:
//                                 guard.js tells it apart by the last watch.
//   yaprobot watch <runner pid> <lock file|-> <note file>
//                                 the input watcher, for one run: low-level
//                                 keyboard and mouse hooks on a thread of
//                                 their own. The person's input lacks the
//                                 injected flags (the robot's SendInput and
//                                 Yap's pastes carry them); any key, mouse
//                                 button or wheel, or the pointer moved more
//                                 than 10 px, ends the run at once — the
//                                 runner's whole process tree killed (the
//                                 test Yap, Edge, test pads, consoles; never
//                                 the watcher itself), the lock file deleted,
//                                 one line saying why written to the note file
//                                 and printed on stderr. Prints
//                                 {"watching":true,"idleMs":…,"tick":…} once
//                                 its hooks are in. Disarmed until "arm" on
//                                 stdin (sent once the idle check passes;
//                                 {"armed":true}): input seen before that
//                                 ends the run the moment it's armed, and a
//                                 refused run is never "stopped". On "stop"
//                                 (or its stdin closing)
//                                 {"stopped":true,"tick":…}; quits by itself
//                                 when the runner is gone.
//   yaprobot watchtest [listen ms]   its self-test: the judge on made-up
//                                 events, then the hooks for real on injected
//                                 1 px mouse moves there and back (the pointer
//                                 put back exactly; no key, click or wheel
//                                 sent): {"ok":…,"fails":[…],…}. With a
//                                 listen time it then only listens, and says
//                                 what of the person's own input it saw and
//                                 what would have stopped a run — nothing is
//                                 stopped.
//
// What reaches the hooks (measured 2026-10-30, Windows 11): the person's
// keys, clicks and moves with no injected flag; SendInput's moves flagged
// injected; a SendInput move of 0, 0 not at all; SetCursorPos (the robot's
// snipdrag, barclick, rightclick) not at all. Inside the hook GetCursorPos
// is still where the pointer was before the move.
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.Runtime.InteropServices;
using System.Text;
using System.Threading;
using System.Web.Script.Serialization;
using Microsoft.Win32.SafeHandles;

/// What the watcher makes of one low-level event: the person's own input,
/// or not. No Windows calls, so the self-test can feed it made-up events.
class InputJudge {
    // Any key, mouse button or wheel counts. A move counts once the pointer
    // has gone more than MovePx physical pixels, net, within one burst of
    // movement (moves less than BurstGapMs apart): a knocked desk or a
    // mouse on a vibrating surface jitters it a pixel or three back and
    // forth, which nets to almost nothing, and a burst that stops resets.
    // A hand on the mouse moves it tens of pixels in a fraction of a second.
    public const int MovePx = 10;
    public const uint BurstGapMs = 500;

    public const int None = 0, Key = 1, Button = 2, Wheel = 3, Move = 4;
    // LLKHF_INJECTED / LLMHF_INJECTED, and the lower-integrity variants
    // (Windows sets those with the injected flag; checked anyway).
    const uint KeyInjected = 0x10 | 0x02, MouseInjected = 0x01 | 0x02;
    const int WM_MOUSEMOVE = 0x0200, WM_MOUSEWHEEL = 0x020A, WM_MOUSEHWHEEL = 0x020E;

    int netX, netY;
    uint lastMove;
    bool moving;
    /// The net movement of the burst that counted, in pixels.
    public int MovedPx;

    public static bool Injected(bool key, uint flags) {
        return (flags & (key ? KeyInjected : MouseInjected)) != 0;
    }

    public int OnKey(uint flags) {
        return Injected(true, flags) ? None : Key;
    }

    /// `dx`, `dy`: how far this move takes the pointer from where it is.
    public int OnMouse(int msg, uint flags, int dx, int dy, uint time) {
        if (Injected(false, flags)) return None;
        if (msg == WM_MOUSEWHEEL || msg == WM_MOUSEHWHEEL) return Wheel;
        if (msg != WM_MOUSEMOVE) return Button; // a button down or up: left, right, middle, X
        if (!moving || unchecked(time - lastMove) > BurstGapMs) { netX = 0; netY = 0; }
        moving = true;
        lastMove = time;
        netX += dx;
        netY += dy;
        long d2 = (long)netX * netX + (long)netY * netY;
        if (d2 <= (long)MovePx * MovePx) return None;
        MovedPx = (int)Math.Ceiling(Math.Sqrt(d2)); // just past 10 says 11
        return Move;
    }

    public static string Say(int what, int px) {
        switch (what) {
            case Key: return "someone pressed a key";
            case Button: return "someone clicked a mouse button";
            case Wheel: return "someone turned the mouse wheel";
            default: return "someone moved the mouse (" + px + " px)";
        }
    }
}

static class Watcher {
    delegate IntPtr HookProc(int code, IntPtr w, IntPtr l);
    [DllImport("user32.dll", SetLastError = true)] static extern IntPtr SetWindowsHookEx(int id, HookProc proc, IntPtr mod, uint thread);
    [DllImport("user32.dll")] static extern bool UnhookWindowsHookEx(IntPtr h);
    [DllImport("user32.dll")] static extern IntPtr CallNextHookEx(IntPtr h, int code, IntPtr w, IntPtr l);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode)] static extern IntPtr GetModuleHandle(string name);
    [DllImport("kernel32.dll")] static extern uint GetCurrentThreadId();
    [StructLayout(LayoutKind.Sequential)]
    struct MSG { public IntPtr hwnd; public uint message; public IntPtr wParam; public IntPtr lParam; public uint time; public int x; public int y; }
    [DllImport("user32.dll")] static extern int GetMessage(out MSG m, IntPtr h, uint min, uint max);
    [DllImport("user32.dll")] static extern bool PeekMessage(out MSG m, IntPtr h, uint min, uint max, uint remove);
    [DllImport("user32.dll")] static extern bool PostThreadMessage(uint thread, uint msg, IntPtr w, IntPtr l);
    [StructLayout(LayoutKind.Sequential)]
    struct LASTINPUTINFO { public uint cbSize; public uint dwTime; }
    [DllImport("user32.dll")] static extern bool GetLastInputInfo(ref LASTINPUTINFO lii);
    [DllImport("kernel32.dll")] static extern uint GetTickCount();
    [DllImport("kernel32.dll", SetLastError = true)] static extern IntPtr OpenProcess(uint access, bool inherit, uint pid);
    [DllImport("kernel32.dll")] static extern bool TerminateProcess(IntPtr h, uint code);
    [DllImport("kernel32.dll")] static extern bool GetProcessTimes(IntPtr h, out long created, out long exited, out long kernel, out long user);

    const uint SYNCHRONIZE = 0x00100000, PROCESS_TERMINATE = 0x0001, PROCESS_QUERY_LIMITED_INFORMATION = 0x1000;
    const uint WM_QUIT = 0x0012;

    static readonly JavaScriptSerializer Json = new JavaScriptSerializer();
    static void Print(Dictionary<string, object> o) {
        try {
            Console.Out.WriteLine(Json.Serialize(o));
            Console.Out.Flush();
        } catch (Exception) { /* the runner may be gone */ }
    }

    // ---- idle ----

    /// Milliseconds since Windows last had any input (the person's or
    /// injected), and that input's tick (GetTickCount's clock).
    public static Dictionary<string, object> Idle() {
        var lii = new LASTINPUTINFO { cbSize = (uint)Marshal.SizeOf(typeof(LASTINPUTINFO)) };
        if (!GetLastInputInfo(ref lii)) return new Dictionary<string, object> { { "error", "GetLastInputInfo failed" } };
        uint idle = unchecked(GetTickCount() - lii.dwTime);
        return new Dictionary<string, object> { { "idleMs", (long)idle }, { "tick", (long)lii.dwTime } };
    }

    // ---- the hooks ----

    static HookProc keyProc, mouseProc; // kept alive while hooked
    static Thread hookThread;
    static uint hookThreadId;
    static readonly InputJudge judge = new InputJudge();
    static int seen; // the InputJudge kind of the person's first input, set once
    static int seenPx;
    static int armed; // 1 once the run goes ahead
    static readonly ManualResetEvent trip = new ManualResetEvent(false);

    // The self-test's counts (only kept while `testing`).
    static bool testing;
    static int injectedMoves, injectedFarthest, injectedCounted;
    static readonly int[] realSeen = new int[5]; // by InputJudge kind
    static string firstReal;

    /// Hooks in on a thread that does nothing else (its message loop), at
    /// the highest priority, so the person's input never waits on it.
    static string StartHooks() {
        string error = null;
        var ready = new ManualResetEvent(false);
        hookThread = new Thread(delegate () {
            hookThreadId = GetCurrentThreadId();
            MSG m;
            PeekMessage(out m, IntPtr.Zero, 0, 0, 0); // this thread's message queue, for WM_QUIT
            keyProc = KeyHook;
            mouseProc = MouseHook;
            IntPtr mod = GetModuleHandle(null);
            IntPtr k = SetWindowsHookEx(13, keyProc, mod, 0); // WH_KEYBOARD_LL
            int kErr = Marshal.GetLastWin32Error();
            IntPtr ms = SetWindowsHookEx(14, mouseProc, mod, 0); // WH_MOUSE_LL
            int mErr = Marshal.GetLastWin32Error();
            if (k == IntPtr.Zero || ms == IntPtr.Zero) {
                error = "couldn't hook the " + (k == IntPtr.Zero ? "keyboard (error " + kErr + ")" : "mouse (error " + mErr + ")");
                if (k != IntPtr.Zero) UnhookWindowsHookEx(k);
                if (ms != IntPtr.Zero) UnhookWindowsHookEx(ms);
                ready.Set();
                return;
            }
            ready.Set();
            while (GetMessage(out m, IntPtr.Zero, 0, 0) > 0) { }
            UnhookWindowsHookEx(k);
            UnhookWindowsHookEx(ms);
        });
        hookThread.IsBackground = true;
        hookThread.Priority = ThreadPriority.Highest;
        hookThread.Start();
        ready.WaitOne();
        return error;
    }

    static void StopHooks() {
        if (hookThread == null) return;
        PostThreadMessage(hookThreadId, WM_QUIT, IntPtr.Zero, IntPtr.Zero);
        hookThread.Join(2000);
        hookThread = null;
    }

    // Both hooks read their few fields, pass the event on (CallNextHookEx)
    // and return what it said: nothing is ever swallowed or held. What the
    // event was is a flag set here; the stopping happens on another thread.

    static IntPtr KeyHook(int code, IntPtr w, IntPtr l) {
        uint flags = code >= 0 ? (uint)Marshal.ReadInt32(l, 8) : 0; // KBDLLHOOKSTRUCT.flags
        IntPtr next = CallNextHookEx(IntPtr.Zero, code, w, l);
        if (code >= 0) Saw(judge.OnKey(flags), InputJudge.Injected(true, flags));
        return next;
    }

    static IntPtr MouseHook(int code, IntPtr w, IntPtr l) {
        if (code < 0) return CallNextHookEx(IntPtr.Zero, code, w, l);
        // MSLLHOOKSTRUCT: pt (where the pointer is going), mouseData, flags, time.
        int x = Marshal.ReadInt32(l, 0), y = Marshal.ReadInt32(l, 4);
        uint flags = (uint)Marshal.ReadInt32(l, 12), time = (uint)Marshal.ReadInt32(l, 16);
        IntPtr next = CallNextHookEx(IntPtr.Zero, code, w, l);
        int msg = w.ToInt32();
        bool injected = InputJudge.Injected(false, flags);
        int dx = 0, dy = 0;
        if (msg == 0x0200 && (!injected || testing)) {
            // The pointer hasn't moved yet while the hooks run: this move's
            // own distance, whatever the robot's SetCursorPos did before it.
            Native.POINT at;
            if (Native.GetCursorPos(out at)) { dx = x - at.X; dy = y - at.Y; }
        }
        if (testing && injected && msg == 0x0200) {
            injectedMoves++;
            injectedFarthest = Math.Max(injectedFarthest, Math.Max(Math.Abs(dx), Math.Abs(dy)));
        }
        Saw(judge.OnMouse(msg, flags, dx, dy, time), injected);
        return next;
    }

    static void Saw(int what, bool injected) {
        if (what == InputJudge.None) return;
        if (testing) {
            if (injected) {
                injectedCounted++;
            } else {
                realSeen[what]++;
                if (firstReal == null) firstReal = InputJudge.Say(what, judge.MovedPx);
            }
            return;
        }
        // Both hooks run on the one hook thread: no other event races this.
        if (Interlocked.CompareExchange(ref seen, 0, 0) == 0) {
            seenPx = judge.MovedPx;
            Interlocked.Exchange(ref seen, what);
        }
        // Armed (read after `seen` is written; Arm writes `armed` before it
        // reads `seen`, both with full fences): one of the two trips it.
        if (Interlocked.CompareExchange(ref armed, 0, 0) == 1) trip.Set();
    }

    static void Arm() {
        Interlocked.Exchange(ref armed, 1);
        if (Interlocked.CompareExchange(ref seen, 0, 0) != 0) trip.Set();
    }

    // ---- watch ----

    class ProcessWait : WaitHandle {
        public ProcessWait(IntPtr h) { SafeWaitHandle = new SafeWaitHandle(h, false); }
    }

    public static int Run(uint runnerPid, string lockFile, string noteFile) {
        // Physical pixels for GetCursorPos, as the hook's points are.
        Native.SetProcessDpiAwarenessContext(new IntPtr(-4));
        IntPtr runner = OpenProcess(SYNCHRONIZE | PROCESS_TERMINATE | PROCESS_QUERY_LIMITED_INFORMATION, false, runnerPid);
        if (runner == IntPtr.Zero) {
            Print(new Dictionary<string, object> { { "error", "no runner process " + runnerPid } });
            return 1;
        }
        long runnerStart = Created(runner);
        string err = StartHooks();
        if (err != null) {
            Print(new Dictionary<string, object> { { "error", err } });
            return 1;
        }
        // Read after the hooks are in: input from here on is theirs to see.
        var idle = Idle();
        idle["watching"] = true;
        Print(idle);

        var stop = new ManualResetEvent(false);
        var reader = new Thread(delegate () {
            try {
                string line;
                while ((line = Console.In.ReadLine()) != null) {
                    line = line.Trim();
                    if (line == "stop") break;
                    if (line == "arm") {
                        Arm();
                        Print(new Dictionary<string, object> { { "armed", true } });
                    }
                }
            } catch (Exception) { }
            stop.Set();
        });
        reader.IsBackground = true;
        reader.Start();

        int which = WaitHandle.WaitAny(new WaitHandle[] { trip, new ProcessWait(runner), stop });
        if (which != 0) {
            StopHooks();
            var done = Idle();
            done["stopped"] = true;
            if (which == 1) done["runnerGone"] = true;
            Print(done);
            return 0;
        }

        // The person's own input: end the run first, then say why.
        int killed = KillTree(runnerPid, runner, runnerStart);
        StopHooks();
        bool unlocked = false;
        try {
            if (lockFile != "-" && File.Exists(lockFile)) { File.Delete(lockFile); unlocked = true; }
        } catch (Exception) { }
        string why = "Robot tests stopped at " + DateTime.Now.ToString("HH:mm:ss") + ": " +
            InputJudge.Say(seen, seenPx) + " at the PC. The run was ended (" + killed + (killed == 1 ? " process)" : " processes)") +
            (unlocked ? " and the robot lock removed" : "") +
            ". Run them again only once the PC has been idle; YAP_ROBOT_FORCE=1 only when the person at the PC says so.";
        try {
            Directory.CreateDirectory(Path.GetDirectoryName(Path.GetFullPath(noteFile)));
            File.WriteAllText(noteFile, why + Environment.NewLine, new UTF8Encoding(false));
        } catch (Exception) { }
        try {
            var stderr = new StreamWriter(Console.OpenStandardError(), new UTF8Encoding(false));
            stderr.WriteLine();
            stderr.WriteLine(why);
            stderr.Flush();
        } catch (Exception) { }
        Print(new Dictionary<string, object> { { "stopped", true }, { "input", InputJudge.Say(seen, seenPx) }, { "killed", killed } });
        return 3;
    }

    static long Created(IntPtr h) {
        long created, exited, kernel, user;
        return GetProcessTimes(h, out created, out exited, out kernel, out user) ? created : 0;
    }

    /// What `taskkill /T /F /PID <runner>` means to do, leaving out the
    /// watcher (support/guard.js starts it detached, out of the runner's
    /// kill-on-close job, so it outlives the runner). The whole tree is read
    /// before anything is killed: Node kills a process's non-detached
    /// children with it (its job), and a walk after that would lose the
    /// detached ones (Edge, Windows Terminal, conhost) whose parent had
    /// gone. Then the runner (it starts nothing more), then the rest; then
    /// again for anything started meanwhile — a child of any process known
    /// to be the run's, dead or alive (a process keeps its parent's id).
    /// Only processes started after the runner count, so an id Windows has
    /// given to an older process never does. Returns how many processes
    /// the run had.
    static int KillTree(uint root, IntPtr rootHandle, long rootStart) {
        uint self = (uint)Process.GetCurrentProcess().Id;
        var known = new HashSet<uint> { root };
        var first = Grow(known, self, rootStart);
        TerminateProcess(rootHandle, 1);
        Kill(first);
        for (int pass = 0; pass < 10; pass++) {
            Thread.Sleep(50);
            var more = Grow(known, self, rootStart);
            if (more.Count == 0) break;
            Kill(more);
        }
        return known.Count;
    }

    /// Adds to `known` every process descended from it (started after the
    /// runner, never the watcher) and returns the newcomers, opened for
    /// killing.
    static List<IntPtr> Grow(HashSet<uint> known, uint self, long rootStart) {
        var parent = new Dictionary<uint, uint>();
        IntPtr snap = Native.CreateToolhelp32Snapshot(2, 0); // TH32CS_SNAPPROCESS
        var e = new Native.PROCESSENTRY32W { dwSize = (uint)Marshal.SizeOf(typeof(Native.PROCESSENTRY32W)) };
        if (Native.Process32FirstW(snap, ref e)) {
            do { parent[e.th32ProcessID] = e.th32ParentProcessID; } while (Native.Process32NextW(snap, ref e));
        }
        Native.CloseHandle(snap);
        var found = new List<IntPtr>();
        bool grew = true;
        while (grew) {
            grew = false;
            foreach (var kv in parent) {
                if (known.Contains(kv.Key) || kv.Key == self || !known.Contains(kv.Value)) continue;
                IntPtr h = OpenProcess(PROCESS_TERMINATE | PROCESS_QUERY_LIMITED_INFORMATION, false, kv.Key);
                if (h == IntPtr.Zero) continue;
                if (Created(h) < rootStart) { Native.CloseHandle(h); continue; }
                known.Add(kv.Key);
                found.Add(h);
                grew = true;
            }
        }
        return found;
    }

    static void Kill(List<IntPtr> handles) {
        foreach (var h in handles) {
            TerminateProcess(h, 1);
            Native.CloseHandle(h);
        }
    }

    // ---- the self-test ----

    public static int SelfTest(int listenMs) {
        Native.SetProcessDpiAwarenessContext(new IntPtr(-4));
        var fails = new List<object>();
        Action<bool, string> check = delegate (bool ok, string what) { if (!ok) fails.Add(what); };

        var j = new InputJudge();
        check(j.OnKey(0) == InputJudge.Key, "a key with no flags is the person's");
        check(j.OnKey(0x80) == InputJudge.Key, "a key let go (LLKHF_UP) is the person's");
        check(j.OnKey(0x20) == InputJudge.Key, "a key with Alt held (LLKHF_ALTDOWN) is the person's");
        check(j.OnKey(0x10) == InputJudge.None, "an injected key (LLKHF_INJECTED) isn't");
        check(j.OnKey(0x10 | 0x02 | 0x80) == InputJudge.None, "an injected key from a lower integrity level isn't");
        check(j.OnMouse(0x0201, 0, 0, 0, 1) == InputJudge.Button, "a left click is the person's");
        check(j.OnMouse(0x0205, 0, 0, 0, 1) == InputJudge.Button, "a right button let go is the person's");
        check(j.OnMouse(0x020B, 0, 0, 0, 1) == InputJudge.Button, "an X button is the person's");
        check(j.OnMouse(0x020A, 0, 0, 0, 1) == InputJudge.Wheel, "the wheel is the person's");
        check(j.OnMouse(0x020E, 0, 0, 0, 1) == InputJudge.Wheel, "the horizontal wheel is the person's");
        check(j.OnMouse(0x0201, 0x01, 0, 0, 1) == InputJudge.None, "an injected click isn't");
        check(j.OnMouse(0x020A, 0x01 | 0x02, 0, 0, 1) == InputJudge.None, "an injected wheel isn't");
        check(j.OnMouse(0x0200, 0x01, 900, 400, 1) == InputJudge.None, "an injected move of 900 px isn't");

        // A knocked desk: ±3 px back and forth every 8 ms for 3 s.
        j = new InputJudge();
        bool counted = false;
        uint t = 1000;
        for (int i = 0; i < 375; i++, t += 8) {
            if (j.OnMouse(0x0200, 0, i % 2 == 0 ? 3 : -3, i % 4 < 2 ? 1 : -1, t) != InputJudge.None) counted = true;
        }
        check(!counted, "a mouse jittering ±3 px for 3 s doesn't count");
        // A hand: 2 px every 8 ms, counted on the move past 10 px.
        j = new InputJudge();
        int at = -1;
        t = 5000;
        for (int i = 0; i < 20 && at < 0; i++, t += 8) if (j.OnMouse(0x0200, 0, 2, 0, t) == InputJudge.Move) at = i;
        check(at == 5, "a hand moving the mouse counts at 12 px (counted at move " + at + ")");
        check(j.MovedPx == 12, "the burst's 12 px reported (" + j.MovedPx + ")");
        // Exactly 10 px doesn't; a diagonal 8 + 8 (11 px) does.
        j = new InputJudge();
        check(j.OnMouse(0x0200, 0, 10, 0, 100) == InputJudge.None, "10 px doesn't count");
        j = new InputJudge();
        check(j.OnMouse(0x0200, 0, 8, 8, 100) == InputJudge.Move, "8 + 8 px diagonally (11.3 px) counts");
        check(j.MovedPx == 12, "and says 12 px, rounded up (" + j.MovedPx + ")");
        j = new InputJudge();
        check(j.OnMouse(0x0200, 0, 10, 1, 100) == InputJudge.Move && j.MovedPx == 11, "just past 10 px says 11 (" + j.MovedPx + ")");
        // A burst that stops starts again: 8 px, a 0.6 s pause, 8 px.
        j = new InputJudge();
        j.OnMouse(0x0200, 0, 8, 0, 100);
        check(j.OnMouse(0x0200, 0, 8, 0, 700) == InputJudge.None, "two 8 px nudges 0.6 s apart don't count");
        check(j.OnMouse(0x0200, 0, 8, 0, 900) == InputJudge.Move, "8 px more within the burst counts");
        // The tick count wrapping mid-burst.
        j = new InputJudge();
        j.OnMouse(0x0200, 0, 6, 0, uint.MaxValue - 3);
        check(j.OnMouse(0x0200, 0, 6, 0, 4) == InputJudge.Move, "a burst across the tick count's wrap still counts");
        // Injected moves never add to a burst.
        j = new InputJudge();
        j.OnMouse(0x0200, 0, 6, 0, 100);
        j.OnMouse(0x0200, 0x01, 300, 0, 104);
        check(j.OnMouse(0x0200, 0, 2, 0, 108) == InputJudge.None, "the robot's moves don't add to the person's");

        // The hooks for real: injected mouse moves of 1 px and back
        // (SendInput, as the robot's own input is), the pointer then put
        // back exactly where it was (SetCursorPos, which the hooks never
        // see). No key, click or wheel is sent: safe with someone at the
        // PC. Windows drops a move of 0, 0 before the hooks, hence 1 px.
        // Each must reach the hook flagged injected, measure at most a
        // pixel or two (the hook's point and GetCursorPos agree: per-monitor
        // DPI), and never count as the person's.
        testing = true;
        string err = StartHooks();
        const int Sent = 6;
        int sent = 0;
        if (err == null) {
            Thread.Sleep(100);
            Native.POINT start;
            Native.GetCursorPos(out start);
            var size = Marshal.SizeOf(typeof(Native.INPUT));
            for (int i = 0; i < Sent; i++) {
                var move = new Native.INPUT[1];
                move[0].type = 0;
                move[0].u.mi = new Native.MOUSEINPUT { dx = i % 2 == 0 ? 1 : -1, dwFlags = 0x0001 }; // MOUSEEVENTF_MOVE
                sent += (int)Native.SendInput(1, move, size);
                Thread.Sleep(30);
            }
            Native.POINT end;
            Native.GetCursorPos(out end);
            // Pointer acceleration can round 1 px away: back exactly, unless
            // the person has moved it meanwhile.
            if (Math.Abs(end.X - start.X) + Math.Abs(end.Y - start.Y) <= Sent) Native.SetCursorPos(start.X, start.Y);
            Thread.Sleep(300);
            if (listenMs > 0) Thread.Sleep(listenMs);
            StopHooks();
        }
        testing = false;
        check(err == null, "hooks in (" + err + ")");
        check(sent == Sent, "the " + Sent + " moves sent (" + sent + ")");
        check(injectedMoves >= sent, "the hook saw the injected moves (" + injectedMoves + " of " + sent + ")");
        check(injectedFarthest <= 2, "each measured at most 2 px (" + injectedFarthest + ")");
        check(injectedCounted == 0, "none counted as the person's (" + injectedCounted + ")");

        // The person's own input seen meanwhile (it fails nothing): what
        // would have stopped a run.
        var real = new Dictionary<string, object> {
            { "keys", realSeen[InputJudge.Key] }, { "buttons", realSeen[InputJudge.Button] },
            { "wheel", realSeen[InputJudge.Wheel] }, { "moves", realSeen[InputJudge.Move] },
        };
        Print(new Dictionary<string, object> {
            { "ok", fails.Count == 0 }, { "fails", fails },
            { "injectedSent", sent }, { "injectedSeen", injectedMoves }, { "injectedCounted", injectedCounted },
            { "listenedMs", listenMs }, { "real", real }, { "wouldStop", firstReal },
        });
        return fails.Count == 0 ? 0 : 1;
    }
}
