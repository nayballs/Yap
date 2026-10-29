//! **Did the app read the paste?** (2026-10-29; Wispr Flow's delayed
//! clipboard rendering, by behaviour.) A dictation goes on the clipboard
//! with its text *promised*, not written: `SetClipboardData(CF_UNICODETEXT,
//! NULL)` from a window of Yap's own (a message-only window on a thread of
//! its own, [`Offer::put`]). Windows asks that window for the text
//! (`WM_RENDERFORMAT`) the moment an app reads it, and that moment says the
//! paste reached an app:
//!
//! - **Who read it.** The window that has the clipboard open
//!   (`GetOpenClipboardWindow`), its process against the target's
//!   ([`classify`]): the target window's own, its focused window's, its
//!   child windows' (a WebView2 app's browser process, a UWP app inside its
//!   frame), or the same program; Yap itself; another app; or nobody can
//!   say (an app that opens the clipboard without a window — Windows
//!   Terminal does, on a thread of its own).
//! - **Who gets it** ([`answer`]). Before the paste keys go, only the
//!   target and Yap: anyone else reading then isn't pasting (a clipboard
//!   manager reacting to the change, which the privacy formats should have
//!   kept away anyway) and gets nothing, so the text stays promised and the
//!   target's read still shows. From the keys on, everyone: a paste is never
//!   broken by a wrong guess at who's reading. A remote desktop or VM
//!   (`text_injector::syncs_clipboard`) is open to all from the start: its
//!   clipboard sync may run in a process of its own.
//! - **What it says** ([`seen`], [`combine`]). The first read that got the
//!   text decides: the target's (or, after the keys, one nobody can name)
//!   → it landed; someone else's → no telling (as before this check); none
//!   within [`Timing::read_within`] → the paste failed, the text stays on
//!   the clipboard and the person is told ("Couldn't paste · it's copied,
//!   press Ctrl+V"). A Win32 edit control's own before/after check
//!   (`selection::PasteCheck`) still decides there, and "nothing that takes
//!   text had the focus" is still "Select a text box first".
//! - **When the person's clipboard comes back**: [`Timing::restore_after`]
//!   after the read (half a second; longer for apps that read again later,
//!   Wispr Flow's figures), and only while the clipboard still holds Yap's
//!   text (nobody copied anything since). With no read in time it never
//!   comes back (the dictation stays copied), so **an app that reads late
//!   gets the dictation, never the old clipboard**: the promise is kept for
//!   [`LATE_MS`] more and a late read is noted ([`Report::late`]), then the
//!   text is written out so it outlasts Yap.
//!
//! When the window or the promise can't be made, the text goes on the
//! clipboard as before (`clip::put_text_verified`) and nothing is judged
//! by reads.

use std::time::Duration;

/// Who opened the clipboard to read Yap's text.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Reader {
    /// The app the paste is for (its process, a process of one of its
    /// windows, or the same program).
    Target,
    /// Yap itself (the robot's check, the next paste's look at the clipboard).
    Yap,
    /// Another app: a clipboard manager, a sync tool, a remote session's helper.
    Other,
    /// It opened the clipboard without a window (Windows Terminal does), so
    /// it can't be named — or there's no target to tell it from.
    Unknown,
}

impl Reader {
    pub fn name(self) -> &'static str {
        match self {
            Reader::Target => "target",
            Reader::Yap => "yap",
            Reader::Other => "other",
            Reader::Unknown => "unknown",
        }
    }
}

/// Where the paste is when a read comes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// The text is on the clipboard, the paste keys not sent yet.
    Waiting,
    /// The keys went (or a console's own Paste was posted).
    Sent,
    /// The paste is over and the text stays for the person to paste (it
    /// wasn't pasted, or "Restore clipboard" is off).
    Left,
}

/// What Yap's clipboard window does with a read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Answer {
    Render,
    Refuse,
}

/// What Yap's clipboard window does when `reader` asks for the text in
/// `phase` (`open`: a remote desktop or VM, open to all). Pure.
pub fn answer(reader: Reader, phase: Phase, open: bool) -> Answer {
    match (reader, phase) {
        (Reader::Target | Reader::Yap, _) => Answer::Render,
        (_, Phase::Sent | Phase::Left) => Answer::Render,
        (_, Phase::Waiting) if open => Answer::Render,
        (_, Phase::Waiting) => Answer::Refuse,
    }
}

/// What the first read that got the text says about the paste.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Seen {
    /// The target read it — `sure` false: a reader nobody can name, after
    /// the keys (Windows Terminal's), taken for the target's.
    Target { sure: bool },
    /// Someone else got the text first, or the target read it before the
    /// keys (not the paste): whether the paste read it too can't be told
    /// (the text was no longer only promised).
    Taken,
    /// Nobody (yet).
    Nothing,
}

/// The read that got the text (by `reader` in `phase`; `open`: a remote
/// desktop or VM), as [`Seen`]. The target's after the keys counts, and
/// before them only a remote desktop's or VM's (its clipboard sync reads
/// as soon as the clipboard changes, and the remote app pastes that); one
/// nobody can name counts after the keys. Pure.
pub fn seen(reader: Reader, phase: Phase, open: bool) -> Seen {
    match (reader, phase) {
        (Reader::Target, Phase::Sent | Phase::Left) => Seen::Target { sure: true },
        (Reader::Target, Phase::Waiting) if open => Seen::Target { sure: true },
        (Reader::Unknown, Phase::Sent | Phase::Left) => Seen::Target { sure: false },
        _ => Seen::Taken,
    }
}

/// What became of a paste.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// It landed, or nothing says it didn't.
    Pasted,
    /// A Win32 edit control stayed exactly as it was: type the text instead.
    Type,
    /// Nothing that takes text had the focus, before the keys or after.
    NoTextBox,
    /// Nobody read the text in time: it stays on the clipboard.
    NotRead,
}

impl Verdict {
    pub fn name(self) -> &'static str {
        match self {
            Verdict::Pasted => "pasted",
            Verdict::Type => "typed",
            Verdict::NoTextBox => "noTextBox",
            Verdict::NotRead => "notRead",
        }
    }
}

/// A paste's outcome from everything Yap checks. Pure.
///
/// - `edit`: a Win32 edit control's before/after check
///   (`selection::PasteCheck`), when there was one. Changed → it landed;
///   unchanged for a second → type it — whatever the read says: the field
///   is the better witness. A check that couldn't tell leaves it to the read.
/// - `seen`: the read ([`seen`]); `offered`: the text was promised (else
///   there's no read to go by, and a paste is taken to have landed, as
///   before).
/// - `no_text_box`: UI Automation said plainly that nothing taking text had
///   the focus, before the keys and after them — "Select a text box first",
///   unless the target itself, named by its window, read the text (it took
///   it somewhere). A read nobody can name never overrides it: Edge's
///   browser reads the clipboard on Ctrl+V with no window even when
///   nothing on the page takes text (the robot, 2026-10-29).
pub fn combine(edit: Option<crate::pasting::selection::Landed>, seen: Seen, offered: bool, no_text_box: bool) -> Verdict {
    use crate::pasting::selection::Landed;
    match edit {
        Some(Landed::Yes) => return Verdict::Pasted,
        Some(Landed::No) => return Verdict::Type,
        Some(Landed::Unknown) | None => {}
    }
    match seen {
        Seen::Target { sure: true } => Verdict::Pasted,
        _ if no_text_box => Verdict::NoTextBox,
        Seen::Target { sure: false } | Seen::Taken => Verdict::Pasted,
        Seen::Nothing if offered => Verdict::NotRead,
        Seen::Nothing => Verdict::Pasted,
    }
}

/// How long a paste has to be read, and how long after the read the
/// person's clipboard comes back.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Timing {
    pub read_within: Duration,
    pub restore_after: Duration,
}

/// Apps that read a paste slowly, or again later: Outlook (both), Word,
/// WhatsApp (Wispr Flow's list).
const SLOW_READERS: &[&str] = &["outlook.exe", "olk.exe", "winword.exe", "whatsapp.exe", "whatsapp.root.exe"];

/// Remote viewers: the read crosses the network (Wispr Flow's list, and
/// Yap's own clipboard-sync apps).
const REMOTE_READERS: &[&str] = &["rdcman.exe"];

/// How long `exe`'s paste gets ([`Timing`]): a second to be read and half
/// a second after it before the clipboard comes back; 3 s and 3 s for
/// [`SLOW_READERS`]; 3 s and 5 s for a remote viewer. Pure.
pub fn timing(exe: Option<&str>) -> Timing {
    let is = |list: &[&str]| exe.is_some_and(|e| list.iter().any(|s| s.eq_ignore_ascii_case(e.trim())));
    if is(REMOTE_READERS) || crate::pasting::text_injector::syncs_clipboard(exe) {
        Timing { read_within: Duration::from_secs(3), restore_after: Duration::from_secs(5) }
    } else if is(SLOW_READERS) {
        Timing { read_within: Duration::from_secs(3), restore_after: Duration::from_secs(3) }
    } else {
        Timing { read_within: Duration::from_millis(1_000), restore_after: Duration::from_millis(500) }
    }
}

/// How long the text stays promised after a paste nobody read: a late read
/// gets the dictation (not the old clipboard) and is noted, then the text
/// is written out so it outlasts Yap.
pub const LATE_MS: u64 = 5_000;

/// A restore due this soon after the paste is waited for; a later one (a
/// slow app's) happens in the background, so the dictation isn't held up.
pub const WAIT_FOR_RESTORE_MS: u64 = 600;

/// Who the target is, for [`classify`]: its windows (the top-level one and
/// the one with the focus) and processes (theirs, and its child windows'),
/// its program, and Yap's own process.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Targets {
    pub windows: Vec<isize>,
    pub pids: Vec<u32>,
    pub exe: Option<String>,
    pub own: Vec<u32>,
}

/// Programs that run many unrelated things (scripts, hosts): a reader of
/// the same name isn't the target for that alone.
const HOSTS: &[&str] = &[
    "python.exe",
    "pythonw.exe",
    "java.exe",
    "javaw.exe",
    "node.exe",
    "electron.exe",
    "powershell.exe",
    "pwsh.exe",
    "cmd.exe",
    "conhost.exe",
    "rundll32.exe",
    "dllhost.exe",
    "svchost.exe",
    "msedgewebview2.exe",
    "autohotkey.exe",
    "autohotkey64.exe",
    "wscript.exe",
    "cscript.exe",
    "mshta.exe",
];

/// Who a reader is: the window that had the clipboard open (`opener`, 0 =
/// none) and its process (`pid`), against `targets`; `exe` names the
/// reader's program, asked only when it decides. Pure.
pub fn classify(targets: &Targets, opener: isize, pid: Option<u32>, exe: impl FnOnce() -> Option<String>) -> Reader {
    let Some(pid) = pid.filter(|_| opener != 0) else {
        return Reader::Unknown;
    };
    if targets.windows.contains(&opener) {
        return Reader::Target;
    }
    if targets.own.contains(&pid) {
        return Reader::Yap;
    }
    if targets.windows.is_empty() && targets.pids.is_empty() {
        return Reader::Unknown;
    }
    if targets.pids.contains(&pid) {
        return Reader::Target;
    }
    let same_program = targets.exe.as_deref().is_some_and(|want| {
        !HOSTS.iter().any(|h| h.eq_ignore_ascii_case(want))
            && exe().is_some_and(|got| got.eq_ignore_ascii_case(want))
    });
    if same_program {
        Reader::Target
    } else {
        Reader::Other
    }
}

/// The last paste, as the robot's report sees it (`realtest_clipboard
/// "lastPaste"`): what read it, when, and what Yap made of it. Never the text.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    /// The text was promised (else written plainly: no read to go by).
    pub offered: bool,
    /// The first app's read that got it, if any (Yap's own look — the
    /// robot's check, the next paste's snapshot — isn't reported).
    pub reader: Option<Reader>,
    /// That read was named (not taken for the target's).
    pub sure: bool,
    /// When, after the paste keys (0: before them).
    pub read_ms: Option<u64>,
    /// Reads refused (others' before the keys).
    pub refused: u32,
    /// The first read came after Yap had judged the paste (it was left on
    /// the clipboard): late.
    pub late: bool,
    /// Its read came from the target (late or not).
    pub late_target: bool,
    /// What Yap made of it ([`Verdict::name`]).
    pub verdict: Option<&'static str>,
    /// The person's clipboard came back (`None`: not asked to, or not yet).
    pub restored: Option<bool>,
    /// Where the focus was put at paste time (`selection::Refocus`).
    pub refocus: Option<&'static str>,
}

#[cfg(windows)]
pub use win::{take_pending, targets_of, Offer};

#[cfg(windows)]
pub(crate) use win::note;

#[cfg(all(windows, debug_assertions))]
pub(crate) use win::report;

#[cfg(all(windows, test))]
pub(crate) use win::start_owner;

#[cfg(windows)]
mod win {
    use std::sync::{Condvar, Mutex, MutexGuard, OnceLock};
    use std::time::{Duration, Instant};

    use tracing::debug;
    use windows::core::w;
    use windows::Win32::Foundation::{GlobalFree, SetLastError, HANDLE, HINSTANCE, HWND, LPARAM, LRESULT, WIN32_ERROR, WPARAM};
    use windows::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, GetClipboardOwner, GetClipboardSequenceNumber, GetOpenClipboardWindow,
        IsClipboardFormatAvailable, OpenClipboard, SetClipboardData,
    };
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DispatchMessageW, EnumChildWindows, GetGUIThreadInfo, GetMessageW,
        GetWindowThreadProcessId, KillTimer, PostMessageW, RegisterClassExW, SetTimer, GUITHREADINFO, HWND_MESSAGE, MSG,
        WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP, WM_DESTROYCLIPBOARD, WM_RENDERALLFORMATS, WM_RENDERFORMAT, WM_TIMER,
        WNDCLASSEXW,
    };

    use super::{answer, classify, seen, Answer, Phase, Reader, Report, Seen, Targets};
    use crate::pasting::text_injector::clip;

    const CF_UNICODETEXT: u32 = 13;
    /// The paste thread asks: write the text out after `lparam` ms (offer `wparam`).
    const WM_LEAVE: u32 = WM_APP + 0x31;
    /// …and: put the person's clipboard back after `lparam` ms.
    const WM_RESTORE_LATER: u32 = WM_APP + 0x32;
    const LATE_TIMER: usize = 1;
    const RESTORE_TIMER: usize = 2;
    /// At most this many child windows are looked at for their processes.
    const MAX_CHILDREN: usize = 4_000;

    /// A promise on the clipboard and what's become of it.
    struct Live {
        id: u64,
        owner: isize,
        /// UTF-16 with its terminating NUL, as CF_UNICODETEXT holds it.
        text: Vec<u8>,
        targets: Targets,
        phase: Phase,
        open: bool,
        /// Robot mode's stand-in for an app that never reads a paste.
        refuse_all: bool,
        /// The read that got the text: who, in which phase, when.
        first: Option<(Reader, Phase, Instant)>,
        keys_at: Option<Instant>,
        refused: u32,
        rendered: bool,
        /// Someone else took the clipboard (or Yap put the person's back).
        gone: bool,
    }

    /// A restore waiting for its time (a slow app's paste).
    struct Pending {
        offer: u64,
        snapshot: clip::Snapshot,
        private: bool,
    }

    struct State {
        next: u64,
        offer: Option<Live>,
        /// The offer whose text is written out when its late wait ends.
        late: Option<u64>,
        pending: Option<Pending>,
        last: Report,
    }

    static STATE: Mutex<State> = Mutex::new(State {
        next: 1,
        offer: None,
        late: None,
        pending: None,
        last: Report {
            offered: false,
            reader: None,
            sure: false,
            read_ms: None,
            refused: 0,
            late: false,
            late_target: false,
            verdict: None,
            restored: None,
            refocus: None,
        },
    });
    static CHANGED: Condvar = Condvar::new();

    fn state() -> MutexGuard<'static, State> {
        STATE.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Yap's clipboard window, made once on a thread of its own (`None`
    /// when Windows wouldn't make it: pastes then go as before).
    pub fn owner() -> Option<isize> {
        static OWNER: OnceLock<Option<isize>> = OnceLock::new();
        *OWNER.get_or_init(|| start(None))
    }

    /// Tests: a clipboard window of their own, on `desktop`.
    #[cfg(test)]
    pub fn start_owner(desktop: isize) -> Option<isize> {
        start(Some(desktop))
    }

    #[link(name = "user32")]
    extern "system" {
        fn SetThreadDesktop(desktop: isize) -> i32;
    }

    fn instance() -> HINSTANCE {
        unsafe { GetModuleHandleW(None).map(|m| HINSTANCE(m.0)).unwrap_or_default() }
    }

    fn start(desktop: Option<isize>) -> Option<isize> {
        let (tx, rx) = std::sync::mpsc::channel::<Option<isize>>();
        let spawned = std::thread::Builder::new().name("clipboard-owner".into()).spawn(move || unsafe {
            if let Some(d) = desktop {
                SetThreadDesktop(d);
            }
            let class = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(wndproc),
                hInstance: instance(),
                lpszClassName: w!("YapPasteOwner"),
                ..Default::default()
            };
            // (A second registration — a test's own window — fails harmlessly.)
            RegisterClassExW(&class);
            let made = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("YapPasteOwner"),
                w!("Yap paste"),
                WINDOW_STYLE(0),
                0,
                0,
                0,
                0,
                Some(HWND_MESSAGE),
                None,
                Some(instance()),
                None,
            );
            let Ok(hwnd) = made else {
                let _ = tx.send(None);
                return;
            };
            let _ = tx.send(Some(hwnd.0 as isize));
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                DispatchMessageW(&msg);
            }
        });
        if spawned.is_err() {
            return None;
        }
        let made = rx.recv_timeout(Duration::from_secs(2)).ok().flatten();
        if made.is_none() {
            tracing::warn!("Couldn't make Yap's clipboard window; pastes go on the clipboard as before");
        }
        made
    }

    fn hwnd(h: isize) -> HWND {
        HWND(h as *mut core::ffi::c_void)
    }

    unsafe extern "system" fn wndproc(window: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        match msg {
            WM_RENDERFORMAT => {
                on_render(window, wparam.0 as u32);
                LRESULT(0)
            }
            // The window is going (Yap quitting): the text written out, so
            // a dictation left on the clipboard outlasts it.
            WM_RENDERALLFORMATS => {
                write_out(window, None);
                LRESULT(0)
            }
            WM_DESTROYCLIPBOARD => {
                on_taken(window);
                LRESULT(0)
            }
            WM_LEAVE => {
                let id = wparam.0 as u64;
                let ms = lparam.0 as u32;
                if ms == 0 {
                    write_out(window, Some(id));
                } else {
                    state().late = Some(id);
                    SetTimer(Some(window), LATE_TIMER, ms, None);
                }
                LRESULT(0)
            }
            WM_RESTORE_LATER => {
                SetTimer(Some(window), RESTORE_TIMER, lparam.0 as u32, None);
                LRESULT(0)
            }
            WM_TIMER => {
                let _ = KillTimer(Some(window), wparam.0);
                match wparam.0 {
                    LATE_TIMER => {
                        let id = state().late.take();
                        if let Some(id) = id {
                            write_out(window, Some(id));
                        }
                    }
                    RESTORE_TIMER => restore_pending(window),
                    _ => {}
                }
                LRESULT(0)
            }
            _ => DefWindowProcW(window, msg, wparam, lparam),
        }
    }

    /// A program's file name ("slack.exe") by process id.
    fn exe_of(pid: u32) -> Option<String> {
        use windows::Win32::Foundation::CloseHandle;
        use windows::Win32::System::Threading::{
            OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
        };
        unsafe {
            let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
            let mut buf = [0u16; 260];
            let mut size = buf.len() as u32;
            let ok = QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, windows::core::PWSTR(buf.as_mut_ptr()), &mut size);
            let _ = CloseHandle(process);
            ok.ok()?;
            let full = String::from_utf16_lossy(&buf[..size as usize]);
            full.rsplit(['\\', '/']).next().map(str::to_string)
        }
    }

    fn window_pid(h: HWND) -> u32 {
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(h, Some(&mut pid)) };
        pid
    }

    /// Someone opened the clipboard and asked for the text: give it or not
    /// ([`answer`]), and note the first read that got it.
    fn on_render(window: HWND, format: u32) {
        if format != CF_UNICODETEXT {
            return;
        }
        // (No window: an app that opened the clipboard without one.)
        let opener = unsafe { GetOpenClipboardWindow() }.unwrap_or_default();
        let pid = (!opener.is_invalid()).then(|| window_pid(opener)).filter(|&p| p != 0);
        let (id, text, reader, phase) = {
            let mut st = state();
            let Some(live) = st.offer.as_mut().filter(|l| !l.gone && l.owner == window.0 as isize) else {
                return;
            };
            // Yap's own reads open the clipboard with no window: told by
            // Yap holding it open (one opener at a time).
            let reader = if clip::yap_holds_it() {
                Reader::Yap
            } else {
                classify(&live.targets, opener.0 as isize, pid, || pid.and_then(exe_of))
            };
            let mut verdict = answer(reader, live.phase, live.open);
            if live.refuse_all && reader != Reader::Yap && live.phase != Phase::Left {
                verdict = Answer::Refuse;
            }
            if verdict == Answer::Refuse {
                live.refused += 1;
                let refused = live.refused;
                st.last.refused = refused;
                debug!(reader = reader.name(), "Kept the clipboard's text from a read before the paste keys");
                return;
            }
            (live.id, live.text.clone(), reader, live.phase)
        };
        // The clipboard is the reader's, open: the data goes in as is.
        let rendered = unsafe { set_data(&text) };
        let mut st = state();
        let first = match st.offer.as_mut().filter(|l| l.id == id) {
            Some(live) if rendered => {
                live.rendered = true;
                if live.first.is_some() {
                    None
                } else {
                    live.first = Some((reader, phase, Instant::now()));
                    Some((live.keys_at, live.open))
                }
            }
            _ => None,
        };
        // The report is about the apps' reads, not Yap's own look (the
        // robot's check, the next paste's snapshot).
        if let Some((keys_at, open)) = first.filter(|_| reader != Reader::Yap) {
            let said = seen(reader, phase, open);
            st.last.reader = Some(reader);
            st.last.sure = said == Seen::Target { sure: true };
            st.last.read_ms = Some(keys_at.map_or(0, |k| k.elapsed().as_millis() as u64));
            if phase == Phase::Left {
                st.last.late = true;
                st.last.late_target = matches!(said, Seen::Target { .. });
                tracing::info!(reader = reader.name(), "The paste was read late, after Yap had left it on the clipboard");
            }
        }
        drop(st);
        CHANGED.notify_all();
    }

    /// The text as CF_UNICODETEXT's data (the clipboard open by whoever
    /// asked, or by Yap's window). `false` if it didn't take.
    unsafe fn set_data(text: &[u8]) -> bool {
        let Ok(global) = GlobalAlloc(GMEM_MOVEABLE, text.len().max(1)) else { return false };
        let ptr = GlobalLock(global) as *mut u8;
        if ptr.is_null() {
            let _ = GlobalFree(Some(global));
            return false;
        }
        std::ptr::copy_nonoverlapping(text.as_ptr(), ptr, text.len());
        let _ = GlobalUnlock(global);
        if SetClipboardData(CF_UNICODETEXT, Some(HANDLE(global.0))).is_err() {
            let _ = GlobalFree(Some(global));
            return false;
        }
        true
    }

    /// The clipboard emptied by someone (another app's copy, the person's
    /// clipboard put back): the promise is over, and a restore still
    /// waiting would overwrite what's there now.
    fn on_taken(window: HWND) {
        let mut st = state();
        if let Some(live) = st.offer.as_mut().filter(|l| l.owner == window.0 as isize) {
            live.gone = true;
        }
        st.pending = None;
        drop(st);
        CHANGED.notify_all();
    }

    /// The promised text written out for good (offer `id`, or whichever is
    /// current), while the clipboard is still Yap's: it no longer depends
    /// on Yap's window.
    fn write_out(window: HWND, id: Option<u64>) {
        let text = {
            let st = state();
            match st.offer.as_ref() {
                Some(l) if !l.gone && !l.rendered && id.is_none_or(|id| l.id == id) => Some((l.id, l.text.clone())),
                _ => None,
            }
        };
        let Some((id, text)) = text else { return };
        unsafe {
            let mut open = false;
            for i in 0..10u64 {
                if OpenClipboard(Some(window)).is_ok() {
                    open = true;
                    break;
                }
                std::thread::sleep(Duration::from_millis(5 + i * 10));
            }
            if !open {
                debug!("Couldn't open the clipboard to write the dictation out");
                return;
            }
            let ours = GetClipboardOwner().is_ok_and(|o| o == window);
            let wrote = ours && set_data(&text);
            let _ = CloseClipboard();
            if wrote {
                if let Some(live) = state().offer.as_mut().filter(|l| l.id == id) {
                    live.rendered = true;
                }
                debug!("The dictation left on the clipboard, written out");
            }
        }
    }

    /// A slow app's restore, now due: the person's clipboard back, if it
    /// still holds Yap's text.
    fn restore_pending(window: HWND) {
        let pending = {
            let mut st = state();
            let Some(p) = st.pending.take() else { return };
            match st.offer.as_mut() {
                Some(live) if live.id == p.offer && !live.gone => {
                    live.gone = true;
                    Some(p)
                }
                _ => None,
            }
        };
        let Some(p) = pending else { return };
        let ours = unsafe { GetClipboardOwner() }.is_ok_and(|o| o == window);
        if ours {
            clip::restore(p.snapshot, p.private);
            state().last.restored = Some(true);
            debug!("The clipboard put back after a slow app's paste");
        }
    }

    /// The person's clipboard, if a restore is still waiting for a slow
    /// app and nobody has copied anything since: the next paste takes it
    /// over (its own look would only find Yap's text).
    pub fn take_pending() -> Option<clip::Snapshot> {
        let owner = owner()?;
        let mut st = state();
        let p = st.pending.take()?;
        let current = st.offer.as_ref().is_some_and(|l| l.id == p.offer && !l.gone);
        let ours = unsafe { GetClipboardOwner() }.is_ok_and(|o| o.0 as isize == owner);
        (current && ours).then(|| {
            debug!("The next paste takes over the clipboard a slow app's restore was keeping");
            p.snapshot
        })
    }

    /// Who the target is ([`Targets`]): `receiver`'s window and the window
    /// with the focus in its thread, their processes and those of its child
    /// windows (WebView2's browser process, a UWP app inside its frame),
    /// its program; and Yap's own process.
    pub fn targets_of(receiver: Option<isize>) -> Targets {
        let mut t = Targets { own: vec![std::process::id()], ..Targets::default() };
        let Some(top) = receiver.filter(|&h| h != 0) else { return t };
        let top_pid = window_pid(hwnd(top));
        if top_pid == 0 {
            return t;
        }
        t.windows.push(top);
        t.pids.push(top_pid);
        unsafe {
            let thread = GetWindowThreadProcessId(hwnd(top), None);
            let mut info = GUITHREADINFO { cbSize: std::mem::size_of::<GUITHREADINFO>() as u32, ..Default::default() };
            if thread != 0 && GetGUIThreadInfo(thread, &mut info).is_ok() && !info.hwndFocus.is_invalid() {
                t.windows.push(info.hwndFocus.0 as isize);
                t.pids.push(window_pid(info.hwndFocus));
            }
            let mut found: (Vec<u32>, usize) = (Vec::new(), 0);
            let _ = EnumChildWindows(Some(hwnd(top)), Some(child), LPARAM(&mut found as *mut (Vec<u32>, usize) as isize));
            t.pids.extend(found.0);
        }
        t.pids.sort_unstable();
        t.pids.dedup();
        t.pids.retain(|&p| p != 0);
        t.exe = crate::pasting::text_injector::app_name_for(Some(top));
        t
    }

    unsafe extern "system" fn child(h: HWND, lparam: LPARAM) -> windows::core::BOOL {
        let found = &mut *(lparam.0 as *mut (Vec<u32>, usize));
        found.1 += 1;
        let pid = window_pid(h);
        if pid != 0 && !found.0.contains(&pid) {
            found.0.push(pid);
        }
        (found.1 < MAX_CHILDREN).into()
    }

    /// The dictation, promised on the clipboard by Yap's window.
    pub struct Offer {
        id: u64,
        owner: isize,
    }

    impl Offer {
        /// Put `text` on the clipboard as a promise (`private`: with the
        /// keep-it-out-of-history formats, written at once; `open`: any app
        /// may read it before the keys — a remote desktop's sync), and check
        /// it took (the sequence number moved), retrying with a short
        /// backoff as a plain write does. `None`: it didn't (no window, the
        /// clipboard stayed busy): put it there plainly instead.
        pub fn put(text: &str, private: bool, targets: Targets, open: bool) -> Option<Offer> {
            Offer::put_with(owner()?, text, private, targets, open)
        }

        pub(crate) fn put_with(owner: isize, text: &str, private: bool, targets: Targets, open: bool) -> Option<Offer> {
            let bytes: Vec<u8> = text.encode_utf16().chain(std::iter::once(0)).flat_map(|u| u.to_le_bytes()).collect();
            let refuse_all = crate::pasting::text_injector::test_refuses_reads();
            for (attempt, wait_ms) in [0u64, 15, 40, 80].into_iter().enumerate() {
                std::thread::sleep(Duration::from_millis(wait_ms));
                let before = unsafe { GetClipboardSequenceNumber() };
                let id = match Offer::try_put(owner, &bytes, private, &targets, open, refuse_all) {
                    Ok(id) => id,
                    Err(e) => {
                        debug!(attempt, error = %e, "The clipboard promise didn't go on");
                        continue;
                    }
                };
                if unsafe { GetClipboardSequenceNumber() } != before {
                    debug!(attempt, private, "The dictation is promised on the clipboard");
                    return Some(Offer { id, owner });
                }
                debug!(attempt, "The clipboard promise didn't take (sequence number unchanged)");
                if let Some(live) = state().offer.as_mut().filter(|l| l.id == id) {
                    live.gone = true;
                }
            }
            None
        }

        fn try_put(owner: isize, bytes: &[u8], private: bool, targets: &Targets, open: bool, refuse_all: bool) -> Result<u64, String> {
            unsafe {
                // Open as Yap's window: emptying makes it the owner, so the
                // reads come to it.
                OpenClipboard(Some(hwnd(owner))).map_err(|e| format!("OpenClipboard: {e}"))?;
                struct Close;
                impl Drop for Close {
                    fn drop(&mut self) {
                        unsafe {
                            let _ = CloseClipboard();
                        }
                    }
                }
                let _close = Close;
                // (The promise before, if any, is told it's over:
                // WM_DESTROYCLIPBOARD, here and now.)
                EmptyClipboard().map_err(|e| format!("EmptyClipboard: {e}"))?;
                let id = {
                    let mut st = state();
                    let id = st.next;
                    st.next += 1;
                    st.late = None;
                    st.pending = None;
                    // The report starts afresh here, before the clipboard
                    // closes: a clipboard manager's read the moment it does
                    // is this paste's.
                    st.last = Report { offered: true, ..Report::default() };
                    st.offer = Some(Live {
                        id,
                        owner,
                        text: bytes.to_vec(),
                        targets: targets.clone(),
                        phase: Phase::Waiting,
                        open,
                        refuse_all,
                        first: None,
                        keys_at: None,
                        refused: 0,
                        rendered: false,
                        gone: false,
                    });
                    id
                };
                // The promise: no data. SetClipboardData gives no handle back
                // for it, so its "error" carries no code when it took.
                SetLastError(WIN32_ERROR(0));
                if let Err(e) = SetClipboardData(CF_UNICODETEXT, None) {
                    if e.code().is_err() {
                        return Err(format!("SetClipboardData (the promise): {e}"));
                    }
                }
                if IsClipboardFormatAvailable(CF_UNICODETEXT).is_err() {
                    return Err("the promised text isn't listed".into());
                }
                if private {
                    clip::add_privacy_formats();
                }
                Ok(id)
            }
        }

        fn with<T>(&self, f: impl FnOnce(&mut Live) -> T) -> Option<T> {
            state().offer.as_mut().filter(|l| l.id == self.id).map(f)
        }

        /// The paste keys are about to go: reads from now on are the paste's.
        pub fn keys_going(&self) {
            self.with(|l| {
                l.phase = Phase::Sent;
                l.keys_at = Some(Instant::now());
            });
        }

        /// What the first read that got the text says, now.
        pub fn seen(&self) -> Seen {
            self.with(|l| l.first.map_or(Seen::Nothing, |(r, p, _)| seen(r, p, l.open))).unwrap_or(Seen::Nothing)
        }

        /// [`Offer::seen`] once a read got the text, or at `until`.
        pub fn wait(&self, until: Instant) -> Seen {
            let mut st = state();
            loop {
                let (now_seen, gone) = match st.offer.as_ref().filter(|l| l.id == self.id) {
                    Some(l) => (l.first.map_or(Seen::Nothing, |(r, p, _)| seen(r, p, l.open)), l.gone),
                    None => (Seen::Nothing, true),
                };
                if now_seen != Seen::Nothing || gone {
                    return now_seen;
                }
                let now = Instant::now();
                if now >= until {
                    return Seen::Nothing;
                }
                st = CHANGED.wait_timeout(st, until - now).map(|(g, _)| g).unwrap_or_else(|p| p.into_inner().0);
            }
        }

        /// When the read that got the text came, and its reader.
        pub fn first_read(&self) -> Option<(Reader, Instant)> {
            self.with(|l| l.first.map(|(r, _, at)| (r, at))).flatten()
        }

        /// Reads refused so far (others' before the keys).
        pub fn refused(&self) -> u32 {
            self.with(|l| l.refused).unwrap_or(0)
        }

        /// The paste is over and the text stays on the clipboard: anyone may
        /// read it now. Written out for good `grace` from now — a read by
        /// the target meanwhile is noted as late — or at once (`ZERO`).
        pub fn leave(&self, grace: Duration) {
            self.with(|l| l.phase = Phase::Left);
            let ms = grace.as_millis().min(u32::MAX as u128) as isize;
            unsafe {
                let _ = PostMessageW(Some(hwnd(self.owner)), WM_LEAVE, WPARAM(self.id as usize), LPARAM(ms));
            }
        }

        /// Is the clipboard still Yap's text (nobody copied since)? Then the
        /// promise ends here, for the person's clipboard to go back.
        pub fn end_for_restore(&self) -> bool {
            let current = self
                .with(|l| {
                    let was = !l.gone;
                    l.gone = true;
                    was
                })
                .unwrap_or(false);
            current && unsafe { GetClipboardOwner() }.is_ok_and(|o| o.0 as isize == self.owner)
        }

        /// The person's clipboard back `after` from now, in the background
        /// (a slow app's paste: the dictation isn't held up), if it still
        /// holds Yap's text then; a paste before then takes it over.
        pub fn restore_later(&self, snapshot: clip::Snapshot, private: bool, after: Duration) {
            {
                let mut st = state();
                st.pending = Some(Pending { offer: self.id, snapshot, private });
                st.last.restored = None;
            }
            let ms = after.as_millis().min(u32::MAX as u128) as isize;
            unsafe {
                let _ = PostMessageW(Some(hwnd(self.owner)), WM_RESTORE_LATER, WPARAM(0), LPARAM(ms));
            }
        }
    }

    /// The robot's report of the last paste, as it stands.
    #[cfg(debug_assertions)]
    pub(crate) fn report() -> Report {
        state().last.clone()
    }

    /// What Yap made of the last paste, for the report.
    pub(crate) fn note(f: impl FnOnce(&mut Report)) {
        f(&mut state().last);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pasting::selection::Landed;

    #[test]
    fn before_the_keys_only_the_target_and_yap_get_the_text() {
        for reader in [Reader::Target, Reader::Yap] {
            assert_eq!(answer(reader, Phase::Waiting, false), Answer::Render, "{reader:?}");
        }
        for reader in [Reader::Other, Reader::Unknown] {
            assert_eq!(answer(reader, Phase::Waiting, false), Answer::Refuse, "{reader:?}");
            // A remote desktop's sync may be anyone: open to all.
            assert_eq!(answer(reader, Phase::Waiting, true), Answer::Render, "{reader:?}");
        }
    }

    #[test]
    fn from_the_keys_on_everyone_gets_it_so_a_paste_is_never_broken() {
        for reader in [Reader::Target, Reader::Yap, Reader::Other, Reader::Unknown] {
            for phase in [Phase::Sent, Phase::Left] {
                assert_eq!(answer(reader, phase, false), Answer::Render, "{reader:?} {phase:?}");
            }
        }
    }

    #[test]
    fn only_the_targets_read_counts_and_a_nameless_one_only_after_the_keys() {
        assert_eq!(seen(Reader::Target, Phase::Sent, false), Seen::Target { sure: true });
        assert_eq!(seen(Reader::Target, Phase::Left, false), Seen::Target { sure: true });
        assert_eq!(seen(Reader::Unknown, Phase::Sent, false), Seen::Target { sure: false });
        assert_eq!(seen(Reader::Unknown, Phase::Left, false), Seen::Target { sure: false });
        // Before the keys it isn't the paste (a browser looking at the
        // clipboard), unless it's a remote desktop's sync, which pastes that.
        assert_eq!(seen(Reader::Target, Phase::Waiting, false), Seen::Taken);
        assert_eq!(seen(Reader::Target, Phase::Waiting, true), Seen::Target { sure: true });
        // Rendered for someone else first: no telling any more.
        assert_eq!(seen(Reader::Other, Phase::Sent, false), Seen::Taken);
        assert_eq!(seen(Reader::Yap, Phase::Sent, false), Seen::Taken);
        assert_eq!(seen(Reader::Unknown, Phase::Waiting, true), Seen::Taken);
        assert_eq!(seen(Reader::Other, Phase::Waiting, true), Seen::Taken);
    }

    fn t(windows: &[isize], pids: &[u32], exe: Option<&str>) -> Targets {
        Targets {
            windows: windows.to_vec(),
            pids: pids.to_vec(),
            exe: exe.map(str::to_string),
            own: vec![7],
        }
    }

    #[test]
    fn a_reader_is_told_by_its_window_its_process_or_its_program() {
        let edge = t(&[100, 101], &[40, 41], Some("msedge.exe"));
        let never = || -> Option<String> { panic!("the program's name isn't needed") };
        // The focused window itself (an EDIT opening the clipboard).
        assert_eq!(classify(&edge, 101, Some(40), never), Reader::Target);
        // Its process (Chromium's clipboard window), a child window's (WebView2's).
        assert_eq!(classify(&edge, 555, Some(40), never), Reader::Target);
        assert_eq!(classify(&edge, 556, Some(41), never), Reader::Target);
        // Yap's own.
        assert_eq!(classify(&edge, 557, Some(7), never), Reader::Yap);
        // The same program in another process, or another app.
        assert_eq!(classify(&edge, 558, Some(90), || Some("MSEDGE.EXE".into())), Reader::Target);
        assert_eq!(classify(&edge, 559, Some(91), || Some("ditto.exe".into())), Reader::Other);
        // No window: can't be named (Windows Terminal reads that way).
        assert_eq!(classify(&edge, 0, Some(40), never), Reader::Unknown);
        assert_eq!(classify(&edge, 0, None, never), Reader::Unknown);
    }

    #[test]
    fn a_host_program_is_never_the_target_for_its_name_alone() {
        let script = t(&[100], &[40], Some("pythonw.exe"));
        assert_eq!(classify(&script, 500, Some(90), || Some("pythonw.exe".into())), Reader::Other);
        let java = t(&[100], &[40], Some("javaw.exe"));
        assert_eq!(classify(&java, 500, Some(90), || Some("javaw.exe".into())), Reader::Other);
    }

    #[test]
    fn with_no_target_known_nobody_but_yap_can_be_named() {
        let none = Targets { own: vec![7], ..Targets::default() };
        assert_eq!(classify(&none, 500, Some(90), || None), Reader::Unknown);
        assert_eq!(classify(&none, 500, Some(7), || None), Reader::Yap);
    }

    #[test]
    fn an_edit_controls_own_check_comes_first() {
        // The field changed: landed, whatever the read.
        assert_eq!(combine(Some(Landed::Yes), Seen::Nothing, true, false), Verdict::Pasted);
        // Unchanged for a second: typed, as before (even if it read the text).
        assert_eq!(combine(Some(Landed::No), Seen::Target { sure: true }, true, false), Verdict::Type);
        // It couldn't tell: the read decides.
        assert_eq!(combine(Some(Landed::Unknown), Seen::Nothing, true, false), Verdict::NotRead);
        assert_eq!(combine(Some(Landed::Unknown), Seen::Target { sure: true }, true, false), Verdict::Pasted);
    }

    #[test]
    fn the_read_decides_elsewhere() {
        assert_eq!(combine(None, Seen::Target { sure: true }, true, false), Verdict::Pasted);
        assert_eq!(combine(None, Seen::Target { sure: false }, true, false), Verdict::Pasted);
        // Someone else got it first: no telling, so not a failure.
        assert_eq!(combine(None, Seen::Taken, true, false), Verdict::Pasted);
        // Nobody read it in time: it stays copied, and that's said.
        assert_eq!(combine(None, Seen::Nothing, true, false), Verdict::NotRead);
        // No promise (written plainly): nothing to go by, as before.
        assert_eq!(combine(None, Seen::Nothing, false, false), Verdict::Pasted);
    }

    #[test]
    fn select_a_text_box_first_unless_the_target_took_it() {
        assert_eq!(combine(None, Seen::Nothing, true, true), Verdict::NoTextBox);
        assert_eq!(combine(None, Seen::Taken, true, true), Verdict::NoTextBox);
        assert_eq!(combine(None, Seen::Nothing, false, true), Verdict::NoTextBox);
        // The target, named by its window, read it: it took it somewhere.
        assert_eq!(combine(None, Seen::Target { sure: true }, true, true), Verdict::Pasted);
        // A nameless read doesn't say it landed in a box: Edge's browser
        // reads on Ctrl+V with only a button focused.
        assert_eq!(combine(None, Seen::Target { sure: false }, true, true), Verdict::NoTextBox);
        assert_eq!(combine(None, Seen::Target { sure: false }, true, false), Verdict::Pasted);
    }

    /// The real promise on a clipboard of its own: the test process moves
    /// to a new, private window station (each has its own clipboard) with a
    /// desktop for the windows, and checks it did, so the person's
    /// clipboard and Win+V history are never touched. Readers are windows
    /// of the test's own thread: one counts as the target, one doesn't, and
    /// a read with no window can't be named. `#[ignore]`d (it changes the
    /// whole test process's window station):
    /// `cargo test --lib -- --ignored private_clipboard_promise --test-threads=1`.
    #[cfg(windows)]
    #[test]
    #[ignore = "moves the test process to a private window station"]
    fn private_clipboard_promise_round_trip() {
        use std::ffi::c_void;
        use std::time::Instant;

        use windows::core::w;
        use windows::Win32::Foundation::{HGLOBAL, HWND};
        use windows::Win32::System::DataExchange::{CloseClipboard, GetClipboardData, OpenClipboard};
        use windows::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};
        use windows::Win32::UI::WindowsAndMessaging::{CreateWindowExW, HWND_MESSAGE, WINDOW_EX_STYLE, WINDOW_STYLE};

        use crate::pasting::text_injector::clip;

        #[link(name = "user32")]
        extern "system" {
            fn CreateWindowStationW(name: *const u16, flags: u32, access: u32, sa: *mut c_void) -> *mut c_void;
            fn GetProcessWindowStation() -> *mut c_void;
            fn SetProcessWindowStation(station: *mut c_void) -> i32;
            fn CloseWindowStation(station: *mut c_void) -> i32;
            fn CreateDesktopW(name: *const u16, device: *const u16, mode: *mut c_void, flags: u32, access: u32, sa: *mut c_void) -> isize;
            fn SetThreadDesktop(desktop: isize) -> i32;
        }
        #[link(name = "kernel32")]
        extern "system" {
            fn GetLastError() -> u32;
        }
        const WINSTA_ALL_ACCESS: u32 = 0x037F;
        const GENERIC_ALL: u32 = 0x1000_0000;
        const CF_TEXT: u32 = 1;
        const CF_UNICODETEXT: u32 = 13;

        /// Back on the real window station whatever happens.
        struct Back(*mut c_void, *mut c_void);
        impl Drop for Back {
            fn drop(&mut self) {
                unsafe {
                    SetProcessWindowStation(self.0);
                    CloseWindowStation(self.1);
                }
            }
        }

        let real_sequence = clip::sequence();
        let _back = unsafe {
            let original = GetProcessWindowStation();
            let station = CreateWindowStationW(std::ptr::null(), 0, WINSTA_ALL_ACCESS, std::ptr::null_mut());
            assert!(!station.is_null(), "a private window station (error {})", GetLastError());
            assert_ne!(SetProcessWindowStation(station), 0, "moved to it");
            Back(original, station)
        };
        assert_ne!(clip::sequence(), real_sequence, "a clipboard of our own (else stop before touching it)");
        let name: Vec<u16> = "YapPromiseTest\0".encode_utf16().collect();
        let desktop = unsafe {
            CreateDesktopW(name.as_ptr(), std::ptr::null(), std::ptr::null_mut(), 0, GENERIC_ALL, std::ptr::null_mut())
        };
        assert_ne!(desktop, 0, "a desktop on it (error {})", unsafe { GetLastError() });

        // Everything with windows on a thread of its own, on that desktop.
        let run = std::thread::spawn(move || unsafe {
            assert_ne!(SetThreadDesktop(desktop), 0, "this thread on the private desktop");
            let owner = super::start_owner(desktop).expect("Yap's clipboard window");
            let window = || {
                CreateWindowExW(
                    WINDOW_EX_STYLE(0),
                    w!("STATIC"),
                    w!(""),
                    WINDOW_STYLE(0),
                    0,
                    0,
                    0,
                    0,
                    Some(HWND_MESSAGE),
                    None,
                    None,
                    None,
                )
                .expect("a reader's window")
                .0 as isize
            };
            let (target, other) = (window(), window());
            // `format` read with the clipboard opened by `opener` (0: none).
            let read_as = |opener: isize, format: u32| -> Option<String> {
                let h = (opener != 0).then_some(HWND(opener as *mut c_void));
                let mut open = false;
                for _ in 0..50 {
                    if OpenClipboard(h).is_ok() {
                        open = true;
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
                assert!(open, "the clipboard opened to read it");
                let out = GetClipboardData(format).ok().filter(|d| !d.is_invalid()).and_then(|d| {
                    let g = HGLOBAL(d.0);
                    let p = GlobalLock(g) as *const u8;
                    if p.is_null() {
                        return None;
                    }
                    let bytes = std::slice::from_raw_parts(p, GlobalSize(g)).to_vec();
                    let _ = GlobalUnlock(g);
                    Some(if format == CF_UNICODETEXT {
                        let units: Vec<u16> =
                            bytes.as_chunks::<2>().0.iter().map(|&b| u16::from_le_bytes(b)).take_while(|&u| u != 0).collect();
                        String::from_utf16_lossy(&units)
                    } else {
                        String::from_utf8_lossy(&bytes.into_iter().take_while(|&b| b != 0).collect::<Vec<u8>>()).into_owned()
                    })
                });
                let _ = CloseClipboard();
                out
            };
            let read = |opener: isize| read_as(opener, CF_UNICODETEXT);
            let targets = Targets { windows: vec![target], pids: vec![], exe: None, own: vec![] };
            let soon = || Instant::now() + Duration::from_secs(2);

            // 1. Promised, private: a clipboard manager's read before the
            // keys gets nothing (twice: still promised), the target's after
            // them gets the text and counts; the person's clipboard back.
            assert!(clip::put_text_verified("What you had copied", true));
            let before = clip::snapshot();
            let offer = Offer::put_with(owner, "Hello from Yap", true, targets.clone(), false).expect("promised");
            for (name, bytes) in clip::privacy_formats() {
                assert_eq!(bytes.as_deref(), Some(&[0u8, 0, 0, 0][..]), "{name} written at once");
            }
            assert_eq!(read(other), None, "another app before the keys: refused");
            assert_eq!(read(other), None, "and again: the text is still only promised");
            assert_eq!(offer.refused(), 2);
            assert_eq!(offer.seen(), Seen::Nothing);
            offer.keys_going();
            assert_eq!(read(target).as_deref(), Some("Hello from Yap"));
            assert_eq!(offer.wait(soon()), Seen::Target { sure: true });
            assert!(offer.first_read().is_some_and(|(r, _)| r == Reader::Target));
            // Written now: anyone reads it, no second render.
            assert_eq!(read(other).as_deref(), Some("Hello from Yap"));
            assert!(offer.end_for_restore(), "still Yap's text on the clipboard");
            clip::restore(before, true);
            assert_eq!(clip::read_text().as_deref(), Some("What you had copied"));

            // 2. A reader with no window (Windows Terminal's way): refused
            // before the keys, taken for the target's after them.
            let offer = Offer::put_with(owner, "Second", true, targets.clone(), false).expect("promised");
            assert_eq!(read(0), None);
            offer.keys_going();
            assert_eq!(read(0).as_deref(), Some("Second"));
            assert_eq!(offer.seen(), Seen::Target { sure: false });

            // 3. Another app first, after the keys: it gets the text (a paste
            // is never broken), and whether the target read it can't be told.
            let offer = Offer::put_with(owner, "Third", true, targets.clone(), false).expect("promised");
            offer.keys_going();
            assert_eq!(read(other).as_deref(), Some("Third"));
            assert_eq!(offer.seen(), Seen::Taken);

            // 4. An app asking for ANSI text: Windows makes it from the
            // promise, and that read counts.
            let offer = Offer::put_with(owner, "Plain text", true, targets.clone(), false).expect("promised");
            offer.keys_going();
            assert_eq!(read_as(target, CF_TEXT).as_deref(), Some("Plain text"));
            assert_eq!(offer.seen(), Seen::Target { sure: true });

            // 5. Nobody reads it: nothing seen; left, it's written out by
            // Yap's window, and then anyone reads it without asking Yap.
            let offer = Offer::put_with(owner, "Left for you", true, targets.clone(), false).expect("promised");
            offer.keys_going();
            assert_eq!(offer.wait(Instant::now() + Duration::from_millis(150)), Seen::Nothing);
            offer.leave(Duration::ZERO);
            std::thread::sleep(Duration::from_millis(200));
            assert_eq!(read(other).as_deref(), Some("Left for you"));
            assert!(offer.first_read().is_none(), "written out, not read off a promise");

            // 6. Left with a late wait: the target's late read is noted.
            let offer = Offer::put_with(owner, "Late", true, targets.clone(), false).expect("promised");
            offer.keys_going();
            offer.leave(Duration::from_secs(5));
            assert_eq!(read(target).as_deref(), Some("Late"));
            let report = super::report();
            assert!(report.late && report.late_target, "{report:?}");

            // 7. Someone copies something before the restore: theirs stays.
            let offer = Offer::put_with(owner, "Fourth", true, targets.clone(), false).expect("promised");
            offer.keys_going();
            assert_eq!(read(target).as_deref(), Some("Fourth"));
            assert!(clip::put_text_verified("Copied meanwhile", false));
            assert!(!offer.end_for_restore(), "the clipboard isn't Yap's any more");
            assert_eq!(clip::read_text().as_deref(), Some("Copied meanwhile"));

            // 8. A slow app's restore, in the background.
            let before = clip::snapshot();
            let offer = Offer::put_with(owner, "Fifth", true, targets.clone(), false).expect("promised");
            offer.keys_going();
            assert_eq!(read(target).as_deref(), Some("Fifth"));
            offer.restore_later(before, true, Duration::from_millis(150));
            std::thread::sleep(Duration::from_millis(500));
            assert_eq!(clip::read_text().as_deref(), Some("Copied meanwhile"));

            // 9. A remote desktop's paste (plain, open to all): its sync's
            // read before the keys gets the text.
            let offer = Offer::put_with(owner, "Remote", false, targets.clone(), true).expect("promised");
            assert!(clip::privacy_formats().iter().all(|(_, b)| b.is_none()), "plain");
            assert_eq!(read(other).as_deref(), Some("Remote"));
            assert_eq!(offer.seen(), Seen::Taken);
        });
        let result = run.join();
        assert!(result.is_ok(), "the promise's round trip failed (see above)");
    }

    #[test]
    fn slow_apps_and_remote_viewers_get_longer() {
        let normal = timing(Some("chrome.exe"));
        assert_eq!(normal.read_within, Duration::from_millis(1_000));
        assert_eq!(normal.restore_after, Duration::from_millis(500));
        assert_eq!(timing(None), normal);
        let word = timing(Some("WINWORD.EXE"));
        assert_eq!((word.read_within, word.restore_after), (Duration::from_secs(3), Duration::from_secs(3)));
        assert_eq!(timing(Some("olk.exe")), word);
        let remote = timing(Some("mstsc.exe"));
        assert_eq!((remote.read_within, remote.restore_after), (Duration::from_secs(3), Duration::from_secs(5)));
        assert_eq!(timing(Some("rdcman.exe")), remote);
        // A slow restore happens in the background; a normal one is waited for.
        assert!(normal.restore_after.as_millis() as u64 <= WAIT_FOR_RESTORE_MS);
        assert!(word.restore_after.as_millis() as u64 > WAIT_FOR_RESTORE_MS);
    }
}
