//! **Microphone management** (2026-10-25; Wispr Flow's, by behaviour):
//! which microphone dictation and meetings listen to while headsets come
//! and go.
//!
//! - **An order, not one pick.** Settings → Dictation → Microphone keeps the
//!   person's microphones in order (`config.mic_order`, [`MicChoice`]): the
//!   first one that's connected is used, then Windows' default. Empty (the
//!   default) is Windows' default alone — the one microphone picked before
//!   stays first (`config::migrate_once` v4). Each is kept by its Core
//!   Audio **endpoint id**, which stays the same when the device is plugged
//!   in again, with its name for one picked before ids were kept (or plugged
//!   into another USB port: a new id, the same name).
//! - **Windows' list, live.** The active capture endpoints ([`present`]) are
//!   read once and then again whenever Core Audio says something changed
//!   (`watch.rs`: `IMMNotificationClient`). A change goes to the dictation's
//!   mic (`mic.rs`: a mic higher in the order is used from the next
//!   dictation; the one it records from gone → the dictation ends,
//!   `pipeline/unplugged.rs`), to Settings (`yap-mics-changed`), to a
//!   meeting recording (it reads [`generation`]: its mic gone → the next one,
//!   `meeting.rs`), and, for a microphone that just arrived, to the offer
//!   (`offer.rs`: "<mic> connected · Use it for dictation?", or "Using <mic>
//!   for dictation" when the order puts it to use).
//!
//! The rules — which microphone ([`candidates`], [`in_use`]), what an
//! arrival deserves ([`on_arrival`]), how often to ask ([`Gate`]) — are pure
//! and unit-tested. Test runs can swap Windows' list for made-up devices
//! (`e2e_mics`, `e2e_mic_plug`, `e2e_mic_unplug`, `YAP_E2E_MICS`): `mic.rs`
//! then "opens" them as a tone, and unplugs them as Windows would.

pub mod offer;
#[cfg(windows)]
mod watch;

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

/// Settings' event: Windows' microphones changed (the page asks
/// `list_mics` again).
pub const EVENT_CHANGED: &str = "yap-mics-changed";

/// A microphone in the person's order (`config.mic_order`) or answered
/// "Not now" to (`config.mic_not_now`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MicChoice {
    /// Its Core Audio endpoint id ("{0.0.1.00000000}.{…}"); empty for one
    /// kept by name only (picked before 2026-10-25).
    pub id: String,
    /// Its name as Windows gives it ("Headset Microphone (Contoso H2)").
    pub name: String,
}

/// A capture endpoint Windows has active now.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Endpoint {
    pub id: String,
    pub name: String,
    /// A Bluetooth headset's (`mic::is_bluetooth`): never kept warm.
    pub bluetooth: bool,
    /// Windows' default input (the console role).
    pub default: bool,
    /// A software device (enumerator "ROOT": a streaming app's or a voice
    /// changer's), which comes and goes with its app: never offered.
    #[serde(rename = "virtual")]
    pub is_virtual: bool,
}

/// Where a stream opens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// This endpoint (opened by its name: cpal's).
    Mic(Endpoint),
    /// Windows' default input, whichever it is — when Windows' list
    /// couldn't be read (as Yap always did).
    Default,
}

impl Target {
    pub fn endpoint(&self) -> Option<&Endpoint> {
        match self {
            Target::Mic(e) => Some(e),
            Target::Default => None,
        }
    }
}

// ---- the rules (pure) -----------------------------------------------------------------------

/// Does `choice` name `ep`? By its endpoint id; else — kept without an id,
/// or with an id no endpoint has now (plugged into another port) — by its
/// name.
pub fn matches(choice: &MicChoice, ep: &Endpoint, present: &[Endpoint]) -> bool {
    if !choice.id.is_empty() && choice.id == ep.id {
        return true;
    }
    let id_here = !choice.id.is_empty() && present.iter().any(|p| p.id == choice.id);
    !id_here && !choice.name.trim().is_empty() && choice.name == ep.name
}

/// The connected endpoint `choice` names, if any.
pub fn find<'a>(choice: &MicChoice, present: &'a [Endpoint]) -> Option<&'a Endpoint> {
    present
        .iter()
        .find(|e| !choice.id.is_empty() && e.id == choice.id)
        .or_else(|| present.iter().find(|e| matches(choice, e, present)))
}

/// `ep`'s place in `order` (0 = first), if it's there.
pub fn rank_of(order: &[MicChoice], ep: &Endpoint, present: &[Endpoint]) -> Option<usize> {
    order.iter().position(|c| matches(c, ep, present))
}

/// Where a stream opens, best first: each microphone in `order` that's
/// connected, then Windows' default. `away`: endpoints whose stream was just
/// lost that Windows still lists for a moment (left out, so a default that
/// was unplugged isn't opened again while Windows picks another). `present`
/// `None`: Windows' list couldn't be read — its default, whatever it is.
pub fn candidates(order: &[MicChoice], present: Option<&[Endpoint]>, away: &[String]) -> Vec<Target> {
    let Some(present) = present else {
        return vec![Target::Default];
    };
    let usable = |e: &&Endpoint| !away.contains(&e.id);
    let mut out: Vec<Target> = Vec::new();
    let push = |e: &Endpoint, out: &mut Vec<Target>| {
        if !out.iter().any(|t| t.endpoint().is_some_and(|o| o.id == e.id)) {
            out.push(Target::Mic(e.clone()));
        }
    };
    for c in order {
        if let Some(e) = find(c, present).filter(usable) {
            push(e, &mut out);
        }
    }
    // Windows' default; if that one is away, the next it would pick.
    let mut left = present.iter().filter(usable);
    if let Some(e) = present.iter().filter(usable).find(|e| e.default).or_else(|| left.next()) {
        push(e, &mut out);
    }
    out
}

/// The microphone a dictation starting now would use.
pub fn in_use(order: &[MicChoice], present: &[Endpoint], away: &[String]) -> Option<Endpoint> {
    candidates(order, Some(present), away).into_iter().find_map(|t| match t {
        Target::Mic(e) => Some(e),
        Target::Default => None,
    })
}

/// What a microphone that just connected deserves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arrival {
    /// New to Yap: "<mic> connected · Use it for dictation?"
    Ask,
    /// The order put it to use (it's higher than the one in use): "Using
    /// <mic> for dictation".
    Using,
    /// Nothing to say: lower in the order, answered "Not now" before, a
    /// software device, or Windows' default that Yap follows anyway.
    Quiet,
}

/// What to tell the person about `ep`, just connected, given their `order`,
/// the microphones they said "Not now" to and Windows' list now (`ep` in it).
pub fn on_arrival(ep: &Endpoint, order: &[MicChoice], not_now: &[MicChoice], present: &[Endpoint]) -> Arrival {
    if ep.is_virtual {
        return Arrival::Quiet;
    }
    let ranked = rank_of(order, ep, present).is_some();
    if in_use(order, present, &[]).is_some_and(|u| u.id == ep.id) {
        // In use now: by the order (say so), or as Windows' new default,
        // which is what the person has Yap follow (nothing to say).
        return if ranked { Arrival::Using } else { Arrival::Quiet };
    }
    if ranked || not_now.iter().any(|c| matches(c, ep, present)) {
        return Arrival::Quiet;
    }
    Arrival::Ask
}

/// Asking about a new microphone at most once per [`ASK_GAP_MS`] (a flaky
/// cable or a dock full of devices asks once), and about each at most once
/// a run (one left to fade isn't asked about again until Yap restarts; a
/// "Not now" never again).
pub const ASK_GAP_MS: u64 = 10 * 60_000;

#[derive(Debug, Default)]
pub struct Gate {
    last: Option<u64>,
    asked: Vec<String>,
}

impl Gate {
    /// May Yap ask about `id` at `now` (ms)? Recorded if so.
    pub fn admit(&mut self, id: &str, now: u64) -> bool {
        if self.asked.iter().any(|a| a == id) {
            return false;
        }
        if self.last.is_some_and(|at| now.saturating_sub(at) < ASK_GAP_MS) {
            return false;
        }
        self.last = Some(now);
        self.asked.push(id.to_string());
        true
    }
}

/// The telling part of a microphone's name: "Headset Microphone (Contoso
/// H2)" → "Contoso H2"; a classic Bluetooth headset's "Headset (Contoso
/// Buds Hands-Free AG Audio)" → "Contoso Buds".
pub fn short_name(name: &str) -> String {
    let name = name.trim();
    let inner = name
        .strip_suffix(')')
        .and_then(|n| n.split_once(" ("))
        .map(|(_, inner)| inner.trim())
        .filter(|i| !i.is_empty())
        .unwrap_or(name);
    let mut out = inner;
    for suffix in [" Hands-Free AG Audio", " Hands-Free Audio", " Hands-Free"] {
        if let Some(s) = out.strip_suffix(suffix) {
            out = s.trim_end();
            break;
        }
    }
    if out.is_empty() {
        "Your microphone".to_string()
    } else {
        out.to_string()
    }
}

// ---- Windows' list ---------------------------------------------------------------------------

static APP: OnceLock<AppHandle> = OnceLock::new();
/// Windows' active capture endpoints as last read (`None`: not read yet, or
/// Core Audio couldn't be asked).
static LIST: Mutex<Option<Vec<Endpoint>>> = Mutex::new(None);
/// One read at a time, so two never report the same change twice.
static READING: Mutex<()> = Mutex::new(());
/// Bumped with every change: a meeting recording's capture thread watches it.
static GENERATION: AtomicU64 = AtomicU64::new(0);

type Listener = Box<dyn Fn() + Send + Sync>;
static LISTENERS: Mutex<Vec<Listener>> = Mutex::new(Vec::new());

fn list() -> std::sync::MutexGuard<'static, Option<Vec<Endpoint>>> {
    LIST.lock().unwrap_or_else(|p| p.into_inner())
}

/// Windows' microphones now (the default flagged): from the last read, or
/// read now the first time. `None`: Core Audio couldn't be asked.
pub fn present() -> Option<Vec<Endpoint>> {
    #[cfg(debug_assertions)]
    if let Some(fake) = fake::list() {
        return Some(fake);
    }
    if let Some(l) = list().clone() {
        return Some(l);
    }
    refresh()
}

/// The app, once started (for those that tell the person from a thread
/// with no handle of its own: a test run's meeting player).
#[cfg(debug_assertions)]
pub(crate) fn app() -> Option<&'static AppHandle> {
    APP.get()
}

/// The count of changes so far (a meeting's capture compares it).
pub fn generation() -> u64 {
    GENERATION.load(Ordering::SeqCst)
}

/// Call `f` (quick: it sends a message) after every change of the list.
pub fn on_change(f: impl Fn() + Send + Sync + 'static) {
    LISTENERS.lock().unwrap_or_else(|p| p.into_inner()).push(Box::new(f));
}

/// Read Windows' list again now (a stream that just failed asks whether its
/// device is still there); a change is told as the watcher tells one.
pub fn refresh() -> Option<Vec<Endpoint>> {
    #[cfg(debug_assertions)]
    if let Some(fake) = fake::list() {
        return Some(fake);
    }
    let (old, new) = {
        let _one = READING.lock().unwrap_or_else(|p| p.into_inner());
        let new = read_windows();
        let mut l = list();
        let old = l.clone();
        if new.is_some() {
            l.clone_from(&new);
        }
        (old, new)
    };
    // (A rename or a new default is a change too: Yap may follow Windows'.)
    if let (Some(old), Some(new)) = (&old, &new) {
        if old != new {
            changed(old, new, true);
        }
    }
    new.or(old)
}

/// The list went from `old` to `new`: tell the dictation's mic, a meeting,
/// Settings and — `offers` — the offer about each microphone that arrived.
/// No lock held.
fn changed(old: &[Endpoint], new: &[Endpoint], offers: bool) {
    let arrived: Vec<Endpoint> = new.iter().filter(|e| !old.iter().any(|o| o.id == e.id)).cloned().collect();
    let left = old.iter().filter(|o| !new.iter().any(|e| e.id == o.id)).count();
    GENERATION.fetch_add(1, Ordering::SeqCst);
    tracing::info!(
        microphones = new.len(),
        arrived = arrived.len(),
        left,
        "Microphones changed"
    );
    for e in &arrived {
        tracing::debug!(name = %e.name, bluetooth = e.bluetooth, "Microphone connected");
    }
    for f in LISTENERS.lock().unwrap_or_else(|p| p.into_inner()).iter() {
        f();
    }
    if let Some(app) = APP.get() {
        let _ = app.emit(EVENT_CHANGED, ());
        if offers {
            for e in arrived {
                offer::arrived(app, e);
            }
        }
    }
}

/// Start with the app: Windows' list read, and watched (a test run with
/// made-up devices, `YAP_E2E_MICS`, watches none).
pub fn start(app: &AppHandle) {
    let _ = APP.set(app.clone());
    #[cfg(debug_assertions)]
    if crate::shell::e2e::active() {
        if let Some(devices) = std::env::var("YAP_E2E_MICS").ok().and_then(|v| serde_json::from_str::<Vec<Endpoint>>(&v).ok()) {
            tracing::info!(count = devices.len(), "e2e: made-up microphones");
            fake::set(Some(devices));
        }
    }
    #[cfg(windows)]
    watch::spawn();
}

/// Windows' active capture endpoints, the default flagged. `None`: Core
/// Audio couldn't be asked.
fn read_windows() -> Option<Vec<Endpoint>> {
    #[cfg(windows)]
    {
        let facts = win::capture_endpoints()?;
        Some(
            facts
                .into_iter()
                .map(|f| Endpoint {
                    bluetooth: crate::dictation::mic::is_bluetooth(&f.enumerator, &f.path, &f.name),
                    is_virtual: f.enumerator.trim().eq_ignore_ascii_case("ROOT"),
                    id: f.id,
                    name: f.name,
                    default: f.default,
                })
                .collect(),
        )
    }
    #[cfg(not(windows))]
    None
}

/// Core Audio's facts about capture endpoints (Windows).
#[cfg(windows)]
pub(crate) mod win {
    use windows::core::GUID;
    use windows::Win32::Foundation::PROPERTYKEY;
    use windows::Win32::Media::Audio::{
        eCapture, eConsole, IMMDevice, IMMDeviceEnumerator, MMDeviceEnumerator, DEVICE_STATE_ACTIVE,
    };
    use windows::Win32::System::Com::StructuredStorage::{PropVariantClear, PROPVARIANT};
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_ALL, COINIT_MULTITHREADED, STGM_READ,
    };
    use windows::Win32::System::Variant::{VT_LPWSTR, VT_UI4};

    /// PKEY_Device_FriendlyName: "Microphone (Contoso USB Mic)", the name cpal gives.
    const FRIENDLY_NAME: PROPERTYKEY = PROPERTYKEY {
        fmtid: GUID::from_u128(0xa45c254e_df1c_4efd_8020_67d146a850e0),
        pid: 14,
    };
    /// PKEY_Device_EnumeratorName: "USB", "HDAUDIO", "BTHENUM", "BTHHFENUM", "ROOT"…
    const ENUMERATOR_NAME: PROPERTYKEY = PROPERTYKEY {
        fmtid: GUID::from_u128(0xa45c254e_df1c_4efd_8020_67d146a850e0),
        pid: 24,
    };
    /// The endpoint's device path: "{1}.USB\VID_…", "{1}.BTHENUM\…".
    const DEVICE_PATH: PROPERTYKEY = PROPERTYKEY {
        fmtid: GUID::from_u128(0xb3f8fa53_0004_438e_9003_51a46e139bfc),
        pid: 2,
    };
    /// PKEY_AudioEndpoint_FormFactor (Microphone 4, Headset 5…), for the log.
    const FORM_FACTOR: PROPERTYKEY = PROPERTYKEY {
        fmtid: GUID::from_u128(0x1da5d803_d492_4edd_8c23_e0c0ffee7f0e),
        pid: 0,
    };

    /// What Windows says about a capture endpoint.
    #[derive(Debug, Default, Clone)]
    pub(crate) struct Facts {
        pub id: String,
        pub name: String,
        pub enumerator: String,
        pub path: String,
        pub form_factor: Option<u32>,
        pub default: bool,
    }

    /// COM on this thread, undone on drop when this did it (cpal may have
    /// set the thread up its own way already: RPC_E_CHANGED_MODE, and COM
    /// works either way).
    pub(crate) struct Com(bool);
    impl Drop for Com {
        fn drop(&mut self) {
            if self.0 {
                // SAFETY: balances this thread's successful CoInitializeEx.
                unsafe { CoUninitialize() };
            }
        }
    }

    pub(crate) fn com() -> Com {
        // SAFETY: plain COM initialisation of this thread.
        Com(unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok())
    }

    pub(crate) fn enumerator() -> Option<IMMDeviceEnumerator> {
        // SAFETY: a plain COM call; the pointer is released by its wrapper.
        unsafe { CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_ALL).ok() }
    }

    fn text(value: &PROPVARIANT) -> Option<String> {
        // SAFETY: read as the type the PROPVARIANT says it holds.
        unsafe {
            let inner = &value.Anonymous.Anonymous;
            if inner.vt == VT_LPWSTR && !inner.Anonymous.pwszVal.is_null() {
                inner.Anonymous.pwszVal.to_string().ok()
            } else {
                None
            }
        }
    }

    fn id_of(device: &IMMDevice) -> Option<String> {
        // SAFETY: the id string is COM-allocated and freed once, after
        // copying it.
        unsafe {
            let id = device.GetId().ok()?;
            let text = id.to_string().ok();
            CoTaskMemFree(Some(id.0 as *const _));
            text
        }
    }

    pub(crate) fn facts(device: &IMMDevice) -> Option<Facts> {
        // SAFETY: COM calls on this thread; each PROPVARIANT is read, then
        // cleared once.
        unsafe {
            let store = device.OpenPropertyStore(STGM_READ).ok()?;
            let read = |key: &PROPERTYKEY| -> (Option<String>, Option<u32>) {
                match store.GetValue(key) {
                    Ok(mut v) => {
                        let s = text(&v);
                        let inner = &v.Anonymous.Anonymous;
                        let n = (inner.vt == VT_UI4).then(|| inner.Anonymous.ulVal);
                        let _ = PropVariantClear(&mut v);
                        (s, n)
                    }
                    Err(_) => (None, None),
                }
            };
            Some(Facts {
                id: id_of(device).unwrap_or_default(),
                name: read(&FRIENDLY_NAME).0.unwrap_or_default(),
                enumerator: read(&ENUMERATOR_NAME).0.unwrap_or_default(),
                path: read(&DEVICE_PATH).0.unwrap_or_default(),
                form_factor: read(&FORM_FACTOR).1,
                default: false,
            })
        }
    }

    /// Every active capture endpoint's facts, Windows' default (console)
    /// one first and flagged. `None`: Core Audio couldn't be asked.
    pub(crate) fn capture_endpoints() -> Option<Vec<Facts>> {
        let _com = com();
        let enumerator = enumerator()?;
        // SAFETY: plain COM calls on this thread; every pointer comes from
        // COM and is released by its wrapper.
        unsafe {
            let default_id = enumerator.GetDefaultAudioEndpoint(eCapture, eConsole).ok().and_then(|d| id_of(&d));
            let all = enumerator.EnumAudioEndpoints(eCapture, DEVICE_STATE_ACTIVE).ok()?;
            let mut out: Vec<Facts> = Vec::new();
            for i in 0..all.GetCount().unwrap_or(0) {
                if let Some(mut f) = all.Item(i).ok().and_then(|d| facts(&d)) {
                    if f.id.is_empty() || out.iter().any(|o| o.id == f.id) {
                        continue;
                    }
                    f.default = default_id.as_deref() == Some(f.id.as_str());
                    out.push(f);
                }
            }
            out.sort_by_key(|f| !f.default);
            Some(out)
        }
    }
}

// ---- commands -------------------------------------------------------------------------------

/// Settings and the first-run setup: Windows' microphones now (the default
/// flagged), for the order's rows and "Add a microphone".
#[tauri::command]
pub async fn list_mics() -> Vec<Endpoint> {
    tauri::async_runtime::spawn_blocking(|| present().unwrap_or_default()).await.unwrap_or_default()
}

// ---- test mode: made-up microphones -----------------------------------------------------------

#[cfg(debug_assertions)]
pub(crate) mod fake {
    use super::Endpoint;
    use std::sync::Mutex;

    static FAKE: Mutex<Option<Vec<Endpoint>>> = Mutex::new(None);

    fn lock() -> std::sync::MutexGuard<'static, Option<Vec<Endpoint>>> {
        FAKE.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// The made-up list, while a test run uses one.
    pub fn list() -> Option<Vec<Endpoint>> {
        if !crate::shell::e2e::active() {
            return None;
        }
        lock().clone()
    }

    /// Is `id` one of the made-up microphones (connected or not)? A stream
    /// on it is a tone (`mic.rs`).
    pub fn is_fake(id: &str) -> bool {
        id.starts_with("e2e-")
    }

    /// Is the made-up `id` connected?
    pub fn connected(id: &str) -> bool {
        lock().as_ref().is_some_and(|l| l.iter().any(|e| e.id == id))
    }

    /// The default flagged: the one asked for, else the first.
    fn settle(mut l: Vec<Endpoint>) -> Vec<Endpoint> {
        let pick = l.iter().position(|e| e.default).unwrap_or(0);
        for (i, e) in l.iter_mut().enumerate() {
            e.default = i == pick;
            if !is_fake(&e.id) {
                e.id = format!("e2e-{}", e.id);
            }
        }
        l
    }

    /// Use `devices` instead of Windows' (`None`: Windows' again). Returns
    /// the list before and after.
    pub fn set(devices: Option<Vec<Endpoint>>) -> (Vec<Endpoint>, Vec<Endpoint>) {
        let mut l = lock();
        let old = l.clone().unwrap_or_default();
        *l = devices.map(settle);
        (old, l.clone().unwrap_or_default())
    }

    /// `device` plugged in (Windows' default if it says so).
    pub fn plug(device: Endpoint) -> Result<(Vec<Endpoint>, Vec<Endpoint>), String> {
        let mut l = lock();
        let old = l.clone().ok_or("No made-up microphones (e2e_mics first)")?;
        let mut new: Vec<Endpoint> = old.iter().filter(|e| e.id != device.id && e.id != format!("e2e-{}", device.id)).cloned().collect();
        let default = device.default;
        if default {
            new.iter_mut().for_each(|e| e.default = false);
        }
        new.push(device);
        let pick = if default { new.len() - 1 } else { new.iter().position(|e| e.default).unwrap_or(0) };
        for (i, e) in new.iter_mut().enumerate() {
            e.default = i == pick;
        }
        let new = settle(new);
        *l = Some(new.clone());
        Ok((old, new))
    }

    /// `id` unplugged (Windows' default moves to the first left).
    pub fn unplug(id: &str) -> Result<(Vec<Endpoint>, Vec<Endpoint>), String> {
        let mut l = lock();
        let old = l.clone().ok_or("No made-up microphones (e2e_mics first)")?;
        let id = if is_fake(id) { id.to_string() } else { format!("e2e-{id}") };
        if !old.iter().any(|e| e.id == id) {
            return Err(format!("No made-up microphone {id}"));
        }
        let new = settle(old.iter().filter(|e| e.id != id).cloned().collect());
        *l = Some(new.clone());
        Ok((old, new))
    }
}

/// Test mode only: made-up microphones instead of Windows' (`devices`:
/// `{id, name, bluetooth?, default?}`; ids get an "e2e-" prefix; the first
/// is Windows' default unless one says otherwise), or Windows' again
/// (`null`). The dictation's mic "opens" one as a tone; `e2e_mic_plug` and
/// `e2e_mic_unplug` come and go as Windows would say. Forgets the offers'
/// rate limit.
#[cfg(debug_assertions)]
#[tauri::command]
pub fn e2e_mics(devices: Option<Vec<Endpoint>>) -> Result<Vec<Endpoint>, String> {
    if !crate::shell::e2e::active() {
        return Err("Only in e2e test mode".to_string());
    }
    let (old, new) = fake::set(devices);
    offer::e2e_reset();
    tracing::info!(count = new.len(), "e2e: made-up microphones");
    changed(&old, &new, false);
    Ok(new)
}

/// Test mode only: a made-up microphone plugged in — told as Windows tells
/// one (the offer included).
#[cfg(debug_assertions)]
#[tauri::command]
pub fn e2e_mic_plug(device: Endpoint) -> Result<Vec<Endpoint>, String> {
    if !crate::shell::e2e::active() {
        return Err("Only in e2e test mode".to_string());
    }
    let (old, new) = fake::plug(device)?;
    tracing::info!("e2e: a microphone is plugged in");
    changed(&old, &new, true);
    Ok(new)
}

/// Test mode only: a made-up microphone unplugged — its stream, if one is
/// open, stops as a real one does (`mic.rs`).
#[cfg(debug_assertions)]
#[tauri::command]
pub fn e2e_mic_unplug(id: String) -> Result<Vec<Endpoint>, String> {
    if !crate::shell::e2e::active() {
        return Err("Only in e2e test mode".to_string());
    }
    let (old, new) = fake::unplug(&id)?;
    tracing::info!("e2e: a microphone is unplugged");
    changed(&old, &new, true);
    Ok(new)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ep(id: &str, name: &str) -> Endpoint {
        Endpoint { id: id.into(), name: name.into(), ..Default::default() }
    }

    fn pick(id: &str, name: &str) -> MicChoice {
        MicChoice { id: id.into(), name: name.into() }
    }

    fn ids_of(t: &[Target]) -> Vec<&str> {
        t.iter().map(|t| t.endpoint().map(|e| e.id.as_str()).unwrap_or("default")).collect()
    }

    /// A laptop's built-in mic (Windows' default), a USB mic and a headset.
    fn desk() -> Vec<Endpoint> {
        vec![
            Endpoint { default: true, ..ep("built-in", "Microphone Array (Northwind Audio)") },
            ep("usb", "Microphone (Fabrikam USB Mic)"),
            ep("headset", "Headset Microphone (Contoso H2)"),
        ]
    }

    fn order() -> Vec<MicChoice> {
        vec![pick("headset", "Headset Microphone (Contoso H2)"), pick("usb", "Microphone (Fabrikam USB Mic)")]
    }

    #[test]
    fn the_first_connected_mic_in_the_order_wins_then_windows_default() {
        let all = desk();
        assert_eq!(ids_of(&candidates(&order(), Some(&all), &[])), vec!["headset", "usb", "built-in"]);
        // The headset unplugged: the USB mic.
        let no_headset: Vec<Endpoint> = all.iter().filter(|e| e.id != "headset").cloned().collect();
        assert_eq!(ids_of(&candidates(&order(), Some(&no_headset), &[])), vec!["usb", "built-in"]);
        assert_eq!(in_use(&order(), &no_headset, &[]).unwrap().id, "usb");
        // Neither: Windows' default.
        let laptop = vec![all[0].clone()];
        assert_eq!(in_use(&order(), &laptop, &[]).unwrap().id, "built-in");
        // No order: Windows' default alone.
        assert_eq!(ids_of(&candidates(&[], Some(&all), &[])), vec!["built-in"]);
    }

    #[test]
    fn no_microphone_at_all_is_nothing_to_open_and_an_unread_list_is_windows_default() {
        assert!(candidates(&order(), Some(&[]), &[]).is_empty());
        assert!(in_use(&order(), &[], &[]).is_none());
        assert_eq!(candidates(&order(), None, &[]), vec![Target::Default]);
    }

    #[test]
    fn a_mic_is_known_by_its_endpoint_id_and_else_by_its_name() {
        let all = desk();
        // Picked before ids were kept: by name.
        let old = vec![pick("", "Microphone (Fabrikam USB Mic)")];
        assert_eq!(in_use(&old, &all, &[]).unwrap().id, "usb");
        // Plugged into another port: a new id, the same name.
        let moved = vec![pick("usb-old-port", "Microphone (Fabrikam USB Mic)")];
        assert_eq!(in_use(&moved, &all, &[]).unwrap().id, "usb");
        // The id wins over a name another device has now.
        let renamed = vec![Endpoint { name: "Microphone (Fabrikam USB Mic)".into(), ..ep("other", "x") }, ep("usb", "Microphone (2- Fabrikam USB Mic)")];
        let by_id = pick("usb", "Microphone (Fabrikam USB Mic)");
        assert_eq!(find(&by_id, &renamed).unwrap().id, "usb");
        // A name nobody has, and an empty choice, match nothing.
        assert!(find(&pick("", "Missing microphone"), &all).is_none());
        assert!(find(&pick("", ""), &all).is_none());
    }

    #[test]
    fn a_mic_just_lost_is_skipped_while_windows_still_lists_it() {
        let all = desk();
        assert_eq!(ids_of(&candidates(&order(), Some(&all), &["headset".into()])), vec!["usb", "built-in"]);
        // Windows' default lost: the next one it would pick.
        assert_eq!(ids_of(&candidates(&[], Some(&all), &["built-in".into()])), vec!["usb"]);
    }

    #[test]
    fn duplicates_in_the_order_count_once() {
        let all = desk();
        let twice = vec![pick("usb", "x"), pick("", "Microphone (Fabrikam USB Mic)"), pick("built-in", "")];
        assert_eq!(ids_of(&candidates(&twice, Some(&all), &[])), vec!["usb", "built-in"]);
    }

    #[test]
    fn a_new_mic_is_asked_about_a_higher_one_is_used_and_said_the_rest_is_quiet() {
        let all = desk();
        let headset = all[2].clone();
        // New to Yap (not in the order), not what Yap uses: ask.
        assert_eq!(on_arrival(&headset, &[], &[], &all), Arrival::Ask);
        // Answered "Not now" before: quiet.
        assert_eq!(on_arrival(&headset, &[], &[pick("headset", "")], &all), Arrival::Quiet);
        // First in the order: used from now on, and said.
        assert_eq!(on_arrival(&headset, &order(), &[], &all), Arrival::Using);
        // In the order, but below the one in use: quiet.
        let usb = all[1].clone();
        assert_eq!(on_arrival(&usb, &order(), &[], &all), Arrival::Quiet);
        // Windows made it its default and Yap follows Windows: quiet.
        let mut now_default = desk();
        now_default.iter_mut().for_each(|e| e.default = e.id == "headset");
        assert_eq!(on_arrival(&now_default[2], &[], &[], &now_default), Arrival::Quiet);
        // A software device that came with its app: quiet.
        let steam = Endpoint { is_virtual: true, ..ep("virtual", "Microphone (Contoso Streaming)") };
        let mut with = all.clone();
        with.push(steam.clone());
        assert_eq!(on_arrival(&steam, &[], &[], &with), Arrival::Quiet);
    }

    #[test]
    fn asking_is_rate_limited() {
        let mut g = Gate::default();
        assert!(g.admit("headset", 1_000));
        // The same one again this run: never.
        assert!(!g.admit("headset", 1_000 + 2 * ASK_GAP_MS));
        // Another within the gap: not yet.
        assert!(!g.admit("usb", 1_000 + ASK_GAP_MS - 1));
        assert!(g.admit("usb", 1_000 + ASK_GAP_MS));
    }

    #[test]
    fn the_short_name_is_the_device() {
        assert_eq!(short_name("Headset Microphone (Contoso H2)"), "Contoso H2");
        assert_eq!(short_name("Microphone (Fabrikam USB Mic)"), "Fabrikam USB Mic");
        assert_eq!(short_name("Headset (Contoso Buds Hands-Free AG Audio)"), "Contoso Buds");
        assert_eq!(short_name("Northwind Stereo Microphone"), "Northwind Stereo Microphone");
        assert_eq!(short_name(""), "Your microphone");
    }

    #[test]
    fn the_order_reads_from_settings_json() {
        let c: Vec<MicChoice> = serde_json::from_str(r#"[{"id":"{0.0.1.00000000}.{a}","name":"Headset"},{"name":"Old pick"}]"#).unwrap();
        assert_eq!(c[0].id, "{0.0.1.00000000}.{a}");
        assert_eq!(c[1], pick("", "Old pick"));
        let e: Endpoint = serde_json::from_str(r#"{"id":"a","name":"A","virtual":true}"#).unwrap();
        assert!(e.is_virtual && !e.default && !e.bluetooth);
    }
}
