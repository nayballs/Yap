//! **Windows Credential Manager, one way in.** Every secret Yap keeps there
//! is read, written and deleted here: the account session (`auth.rs`), the
//! calendars' (`calendar/vault.rs`), the ChatGPT sign-in's
//! (`ai/chatgpt/vault.rs`) and the connectors' (`integrations/connectors/vault.rs`).
//! Each keeps its own names and format: a generic credential named
//! `<user>.<service>`, Local persistence, the value as UTF-16 (keyring-core
//! on windows-native-keyring-store, as before 8 Nov 2026), so an installed
//! Yap reads what it kept exactly as it did.
//!
//! **Why everything takes turns.** Credential Manager loses a write when
//! another thread or process writes other names at the same moment: a value
//! just written reads back as missing or as the one before (8 Nov 2026,
//! `tests::real`: three processes writing names of their own at once lost
//! up to half of their 60 writes each; one alone, none). Two token refreshes
//! at once (a connector's and ChatGPT's), the account session beside a
//! calendar's token, or the installed Yap beside Yap - Dev could each drop a
//! sign-in. So:
//!
//! - every read, write and delete takes a [`Turn`]: one at a time in this
//!   process (a gate) and across every Yap on this Windows sign-in (the
//!   named mutex `Local\YapCredentials`: the installed Yap, Yap - Dev, a test
//!   build). Both are waited for up to [`WAIT`] in all, then Yap goes ahead
//!   and says so at warn (never a name). A turn is held only around
//!   Credential Manager's own calls, never across the network; a thread
//!   already holding one takes it again at no cost. Reads too: with the
//!   writes in turns, a read beside another Yap's write still came back
//!   wrong now and then (about 1 in 4,500);
//! - a secret kept in several parts (the connectors', ChatGPT's) is read and
//!   written whole within one turn ([`turn`]), so a read never sees half a
//!   write;
//! - every write is read back and compared, in memory only; one that differs
//!   (or is refused) is written once more, then it's an [`Error`], so the
//!   caller's "couldn't save" runs instead of a sign-in silently gone. A
//!   delete is checked the same way.
//!
//! Nothing here logs a name or a value: outcomes only. Off Windows there's
//! no credential store wired up yet: secrets are kept in memory for one run.

use std::cell::Cell;
use std::marker::PhantomData;
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

/// How long a write waits for its turn (this process's, then other Yaps')
/// before going ahead anyway.
pub const WAIT: Duration = Duration::from_secs(5);

/// The named mutex every Yap on this Windows sign-in takes turns on (the
/// session namespace: another person signed in to the PC has their own).
#[cfg_attr(not(windows), allow(dead_code))]
const MUTEX_NAME: &str = "Local\\YapCredentials";

/// A credential Credential Manager wouldn't keep (or give back). Its
/// message never carries a name or a value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// Windows refused the call (or the store isn't there).
    Refused,
    /// The write went through but read back differently, twice.
    NotKept,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Error::Refused => "Credential Manager refused it",
            Error::NotKept => "Credential Manager didn't keep it",
        })
    }
}

impl std::error::Error for Error {}

// ---- the turn ----------------------------------------------------------------------------

/// This process's gate: one turn at a time, waited for with a bound.
struct Gate {
    busy: Mutex<bool>,
    freed: Condvar,
}

/// A turn at a [`Gate`], let go when dropped.
struct GateHeld<'a>(&'a Gate);

impl Gate {
    const fn new() -> Gate {
        Gate { busy: Mutex::new(false), freed: Condvar::new() }
    }

    /// The gate's turn, or `None` when it stayed taken for `wait`.
    fn take(&self, wait: Duration) -> Option<GateHeld<'_>> {
        let busy = self.busy.lock().unwrap_or_else(|p| p.into_inner());
        let (mut busy, _) = self.freed.wait_timeout_while(busy, wait, |b| *b).unwrap_or_else(|p| p.into_inner());
        if *busy {
            return None;
        }
        *busy = true;
        Some(GateHeld(self))
    }
}

impl Drop for GateHeld<'_> {
    fn drop(&mut self) {
        *self.0.busy.lock().unwrap_or_else(|p| p.into_inner()) = false;
        self.0.freed.notify_one();
    }
}

static GATE: Gate = Gate::new();

thread_local! {
    /// Turns this thread holds (nested ones are free).
    static HELD: Cell<usize> = const { Cell::new(0) };
}

/// A turn at Credential Manager: this process's and every Yap's, let go when
/// dropped. Stays on the thread that took it (Windows' mutex belongs to it).
pub struct Turn {
    // Let go in this order: other Yaps' first, then this process's.
    across: Option<across::Held>,
    _gate: Option<GateHeld<'static>>,
    _thread: PhantomData<*const ()>,
}

/// Take a turn (waiting up to [`WAIT`]). Hold it around a secret's whole
/// read or write when it's kept in several parts.
pub fn turn() -> Turn {
    if HELD.with(|h| h.get()) > 0 {
        HELD.with(|h| h.set(h.get() + 1));
        return Turn { across: None, _gate: None, _thread: PhantomData };
    }
    let start = Instant::now();
    let gate = GATE.take(WAIT);
    if gate.is_none() {
        tracing::warn!(waited_ms = WAIT.as_millis() as u64, "credentials: another write in this Yap held its turn too long; going ahead");
    }
    let across = across::take(WAIT.saturating_sub(start.elapsed()));
    HELD.with(|h| h.set(1));
    Turn { across, _gate: gate, _thread: PhantomData }
}

impl Drop for Turn {
    fn drop(&mut self) {
        HELD.with(|h| h.set(h.get().saturating_sub(1)));
        self.across.take();
    }
}

impl Turn {
    /// The secret under `service` / `user`, if there is one.
    pub fn read(&self, service: &str, user: &str) -> Result<Option<String>, Error> {
        store::System.get(service, user)
    }

    /// Keep `secret` under `service` / `user` (replacing what was there),
    /// read back to be sure.
    pub fn write(&self, service: &str, user: &str, secret: &str) -> Result<(), Error> {
        write_checked(&store::System, service, user, secret)
    }

    /// Forget what's under `service` / `user` (fine if there's nothing).
    pub fn delete(&self, service: &str, user: &str) -> Result<(), Error> {
        delete_checked(&store::System, service, user)
    }
}

/// [`Turn::read`] in a turn of its own (a read beside another Yap's write
/// can come back wrong too).
pub fn read(service: &str, user: &str) -> Result<Option<String>, Error> {
    turn().read(service, user)
}

/// [`Turn::write`] in a turn of its own.
pub fn write(service: &str, user: &str, secret: &str) -> Result<(), Error> {
    turn().write(service, user, secret)
}

/// [`Turn::delete`] in a turn of its own.
pub fn delete(service: &str, user: &str) -> Result<(), Error> {
    turn().delete(service, user)
}

// ---- checked writes ----------------------------------------------------------------------

/// Credential Manager's three calls, as plain as they come (the real one, or
/// a test's).
trait Raw {
    fn get(&self, service: &str, user: &str) -> Result<Option<String>, Error>;
    fn set(&self, service: &str, user: &str, secret: &str) -> Result<(), Error>;
    /// Nothing there is fine.
    fn remove(&self, service: &str, user: &str) -> Result<(), Error>;
}

/// Write, read back, compare; once more if it differs.
fn write_checked(raw: &dyn Raw, service: &str, user: &str, secret: &str) -> Result<(), Error> {
    let mut last = Error::NotKept;
    for attempt in 0..2 {
        match raw.set(service, user, secret) {
            Ok(()) => match raw.get(service, user) {
                Ok(Some(back)) if back == secret => {
                    if attempt > 0 {
                        tracing::info!("credentials: a write was kept on its second try");
                    }
                    return Ok(());
                }
                Ok(back) => {
                    last = Error::NotKept;
                    tracing::warn!(attempt, found = back.is_some(), "credentials: a write read back differently");
                }
                Err(e) => {
                    last = e;
                    tracing::warn!(attempt, "credentials: a write couldn't be read back");
                }
            },
            Err(e) => {
                last = e;
                tracing::warn!(attempt, "credentials: a write was refused");
            }
        }
    }
    tracing::error!(outcome = %last, "credentials: a write wasn't kept after two tries");
    Err(last)
}

/// Delete, check it's gone; once more if it isn't.
fn delete_checked(raw: &dyn Raw, service: &str, user: &str) -> Result<(), Error> {
    let mut last = Error::NotKept;
    for attempt in 0..2 {
        match raw.remove(service, user).and_then(|()| raw.get(service, user)) {
            Ok(None) => return Ok(()),
            Ok(Some(_)) => {
                last = Error::NotKept;
                tracing::warn!(attempt, "credentials: a deleted credential was still there");
            }
            Err(e) => {
                last = e;
                tracing::warn!(attempt, "credentials: a delete was refused");
            }
        }
    }
    tracing::error!(outcome = %last, "credentials: a delete didn't take after two tries");
    Err(last)
}

// ---- the store ---------------------------------------------------------------------------

#[cfg(windows)]
mod store {
    use super::Error;
    use std::collections::HashMap;
    use std::sync::Once;

    /// Windows Credential Manager.
    pub struct System;

    fn entry(service: &str, user: &str) -> Result<keyring_core::Entry, keyring_core::Error> {
        static STORE: Once = Once::new();
        STORE.call_once(|| {
            if keyring_core::get_default_store().is_none() {
                match windows_native_keyring_store::Store::new() {
                    Ok(store) => keyring_core::set_default_store(store),
                    Err(e) => tracing::error!(kind = kind(&e), "credentials: the credential store is unavailable"),
                }
            }
        });
        // Local: it belongs to this PC (the default, Enterprise, would roam
        // it with a domain user's profile).
        keyring_core::Entry::new_with_modifiers(service, user, &HashMap::from([("persistence", "Local")]))
    }

    /// keyring-core's error, by kind only (some carry a name).
    fn kind(e: &keyring_core::Error) -> &'static str {
        use keyring_core::Error as E;
        match e {
            E::PlatformFailure(_) => "platform failure",
            E::NoStorageAccess(_) => "no storage access",
            E::NoEntry => "no entry",
            E::BadEncoding(_) => "bad encoding",
            E::BadDataFormat(..) => "bad data format",
            E::BadStoreFormat(_) => "bad store format",
            E::TooLong(..) => "too long",
            E::Invalid(..) => "invalid",
            E::Ambiguous(_) => "ambiguous",
            E::NoDefaultStore => "no default store",
            E::NotSupportedByStore(_) => "not supported",
            _ => "other",
        }
    }

    impl super::Raw for System {
        fn get(&self, service: &str, user: &str) -> Result<Option<String>, Error> {
            match entry(service, user).and_then(|e| e.get_password()) {
                Ok(secret) => Ok(Some(secret)),
                Err(keyring_core::Error::NoEntry) => Ok(None),
                Err(e) => {
                    tracing::debug!(kind = kind(&e), "credentials: a read failed");
                    Err(Error::Refused)
                }
            }
        }

        fn set(&self, service: &str, user: &str, secret: &str) -> Result<(), Error> {
            entry(service, user).and_then(|e| e.set_password(secret)).map_err(|e| {
                tracing::debug!(kind = kind(&e), "credentials: a write failed");
                Error::Refused
            })
        }

        fn remove(&self, service: &str, user: &str) -> Result<(), Error> {
            match entry(service, user).and_then(|e| e.delete_credential()) {
                Ok(()) | Err(keyring_core::Error::NoEntry) => Ok(()),
                Err(e) => {
                    tracing::debug!(kind = kind(&e), "credentials: a delete failed");
                    Err(Error::Refused)
                }
            }
        }
    }

    /// Unchecked, unlocked writes: only the stress test's "before".
    #[cfg(test)]
    pub fn raw_set(service: &str, user: &str, secret: &str) -> Result<(), Error> {
        super::Raw::set(&System, service, user, secret)
    }
}

/// No credential store wired up off Windows yet: secrets last one run.
#[cfg(not(windows))]
mod store {
    use super::Error;
    use std::collections::HashMap;
    use std::sync::Mutex;

    pub struct System;

    static SECRETS: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);

    impl super::Raw for System {
        fn get(&self, service: &str, user: &str) -> Result<Option<String>, Error> {
            Ok(SECRETS.lock().unwrap_or_else(|p| p.into_inner()).as_ref().and_then(|m| m.get(&format!("{user}.{service}")).cloned()))
        }
        fn set(&self, service: &str, user: &str, secret: &str) -> Result<(), Error> {
            SECRETS.lock().unwrap_or_else(|p| p.into_inner()).get_or_insert_with(HashMap::new).insert(format!("{user}.{service}"), secret.into());
            Ok(())
        }
        fn remove(&self, service: &str, user: &str) -> Result<(), Error> {
            if let Some(map) = SECRETS.lock().unwrap_or_else(|p| p.into_inner()).as_mut() {
                map.remove(&format!("{user}.{service}"));
            }
            Ok(())
        }
    }
}

// ---- across Yaps -------------------------------------------------------------------------

/// Every Yap on this Windows sign-in: a named mutex in the session namespace.
#[cfg(windows)]
mod across {
    use std::sync::OnceLock;
    use std::time::Duration;
    use windows::core::HSTRING;
    use windows::Win32::Foundation::{HANDLE, WAIT_ABANDONED, WAIT_OBJECT_0};
    use windows::Win32::System::Threading::{CreateMutexW, ReleaseMutex, WaitForSingleObject};

    /// A named mutex, opened (or made) once.
    pub struct Named(usize);

    /// Its turn, let go when dropped (on the thread that took it).
    pub struct Held(usize);

    impl Named {
        pub fn open(name: &str) -> Option<Named> {
            // SAFETY: plain Win32 call; the name is a valid wide string.
            match unsafe { CreateMutexW(None, false, &HSTRING::from(name)) } {
                Ok(h) => Some(Named(h.0 as usize)),
                Err(_) => {
                    tracing::warn!("credentials: couldn't open the turn other Yaps share; going ahead without it");
                    None
                }
            }
        }

        /// Its turn, or `None` when another holder kept it for `wait`.
        pub fn take(&self, wait: Duration) -> Option<Held> {
            let ms = wait.as_millis().min(u32::MAX as u128 - 1) as u32;
            // SAFETY: the handle stays open for the process (or the test) and
            // is waited on by this thread only.
            let got = unsafe { WaitForSingleObject(HANDLE(self.0 as *mut _), ms) };
            if got == WAIT_OBJECT_0 {
                Some(Held(self.0))
            } else if got == WAIT_ABANDONED {
                // A Yap that held it ended mid-turn: the turn is ours now.
                tracing::info!("credentials: another Yap ended during its turn; taking it");
                Some(Held(self.0))
            } else {
                None
            }
        }
    }

    impl Drop for Named {
        fn drop(&mut self) {
            // SAFETY: the handle CreateMutexW gave this value.
            unsafe {
                let _ = windows::Win32::Foundation::CloseHandle(HANDLE(self.0 as *mut _));
            }
        }
    }

    impl Drop for Held {
        fn drop(&mut self) {
            // SAFETY: taken on this thread (a `Turn` never leaves it).
            unsafe {
                let _ = ReleaseMutex(HANDLE(self.0 as *mut _));
            }
        }
    }

    /// The turn every Yap shares, or `None` (waited out, or no mutex).
    pub fn take(wait: Duration) -> Option<Held> {
        static SHARED: OnceLock<Option<Named>> = OnceLock::new();
        let named = SHARED.get_or_init(|| Named::open(super::MUTEX_NAME)).as_ref()?;
        let held = named.take(wait);
        if held.is_none() {
            tracing::warn!(waited_ms = wait.as_millis() as u64, "credentials: another Yap held its turn too long; going ahead");
        }
        held
    }
}

/// Off Windows: no other Yaps to take turns with yet.
#[cfg(not(windows))]
mod across {
    use std::time::Duration;

    pub struct Held;

    pub fn take(_wait: Duration) -> Option<Held> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::HashMap;

    /// A store that loses the first `lose` writes (keeps what was there) and
    /// refuses the first `refuse` calls to `set`.
    #[derive(Default)]
    struct Fake {
        map: RefCell<HashMap<String, String>>,
        lose: Cell<usize>,
        refuse: Cell<usize>,
        stuck: Cell<usize>,
        sets: Cell<usize>,
    }

    impl Raw for Fake {
        fn get(&self, service: &str, user: &str) -> Result<Option<String>, Error> {
            Ok(self.map.borrow().get(&format!("{user}.{service}")).cloned())
        }
        fn set(&self, service: &str, user: &str, secret: &str) -> Result<(), Error> {
            self.sets.set(self.sets.get() + 1);
            if self.refuse.get() > 0 {
                self.refuse.set(self.refuse.get() - 1);
                return Err(Error::Refused);
            }
            if self.lose.get() > 0 {
                self.lose.set(self.lose.get() - 1);
                return Ok(());
            }
            self.map.borrow_mut().insert(format!("{user}.{service}"), secret.into());
            Ok(())
        }
        fn remove(&self, service: &str, user: &str) -> Result<(), Error> {
            if self.stuck.get() > 0 {
                self.stuck.set(self.stuck.get() - 1);
                return Ok(());
            }
            self.map.borrow_mut().remove(&format!("{user}.{service}"));
            Ok(())
        }
    }

    #[test]
    fn a_write_is_read_back_and_a_lost_one_written_again() {
        let fake = Fake::default();
        write_checked(&fake, "svc", "a", "one").unwrap();
        assert_eq!((fake.get("svc", "a").unwrap().as_deref(), fake.sets.get()), (Some("one"), 1), "kept first time: one write");
        // Lost once (the old value still there): written again, kept.
        fake.lose.set(1);
        write_checked(&fake, "svc", "a", "two").unwrap();
        assert_eq!((fake.get("svc", "a").unwrap().as_deref(), fake.sets.get()), (Some("two"), 3));
        // Refused once: the same.
        fake.refuse.set(1);
        write_checked(&fake, "svc", "a", "three").unwrap();
        assert_eq!(fake.get("svc", "a").unwrap().as_deref(), Some("three"));
    }

    #[test]
    fn a_write_lost_twice_is_an_error_not_a_silent_loss() {
        let fake = Fake::default();
        fake.lose.set(2);
        assert_eq!(write_checked(&fake, "svc", "b", "secret"), Err(Error::NotKept));
        assert_eq!(fake.sets.get(), 2, "one more try, no more");
        fake.refuse.set(2);
        assert_eq!(write_checked(&fake, "svc", "b", "secret"), Err(Error::Refused));
        assert!(!Error::NotKept.to_string().contains("secret") && !Error::Refused.to_string().contains("svc"));
    }

    #[test]
    fn a_delete_is_checked_too() {
        let fake = Fake::default();
        write_checked(&fake, "svc", "c", "x").unwrap();
        fake.stuck.set(1);
        delete_checked(&fake, "svc", "c").unwrap();
        assert_eq!(fake.get("svc", "c").unwrap(), None);
        write_checked(&fake, "svc", "c", "x").unwrap();
        fake.stuck.set(2);
        assert_eq!(delete_checked(&fake, "svc", "c"), Err(Error::NotKept));
        delete_checked(&fake, "svc", "nothing-here").unwrap();
    }

    #[test]
    fn the_gate_waits_its_bound_then_lets_the_next_one_in() {
        let gate: &'static Gate = Box::leak(Box::new(Gate::new()));
        let held = gate.take(Duration::ZERO).expect("free at first");
        let other = std::thread::spawn(move || {
            let t = Instant::now();
            let got = gate.take(Duration::from_millis(150)).is_some();
            (got, t.elapsed())
        });
        let (got, waited) = other.join().unwrap();
        assert!(!got && waited >= Duration::from_millis(140), "waited its bound, then gave up: {waited:?}");
        // Let go while another waits: it gets in at once.
        let waiter = std::thread::spawn(move || {
            let t = Instant::now();
            let got = gate.take(Duration::from_secs(5)).is_some();
            (got, t.elapsed())
        });
        std::thread::sleep(Duration::from_millis(50));
        drop(held);
        let (got, waited) = waiter.join().unwrap();
        assert!(got && waited < Duration::from_secs(2), "{waited:?}");
    }

    #[test]
    fn a_thread_takes_its_own_turn_again_for_free() {
        let outer = turn();
        let t = Instant::now();
        let inner = turn();
        assert!(inner.across.is_none() && inner._gate.is_none(), "nested: nothing taken twice");
        drop(inner);
        assert!(t.elapsed() < Duration::from_secs(1));
        assert_eq!(HELD.with(|h| h.get()), 1);
        drop(outer);
        assert_eq!(HELD.with(|h| h.get()), 0);
    }

    /// Windows' mutex: another holder (here another thread, as another Yap
    /// would be) keeps the turn → the wait is bounded; let go → it's ours.
    #[cfg(windows)]
    #[test]
    fn other_yaps_turn_is_waited_for_with_a_bound() {
        let name = format!("Local\\YapCredentialsTest-{}", std::process::id());
        let (took, release) = (std::sync::mpsc::channel(), std::sync::mpsc::channel::<()>());
        let n = name.clone();
        let holder = std::thread::spawn(move || {
            let named = across::Named::open(&n).unwrap();
            let held = named.take(Duration::ZERO).expect("free at first");
            took.0.send(()).unwrap();
            release.1.recv().unwrap();
            drop(held);
        });
        took.1.recv().unwrap();
        let named = across::Named::open(&name).unwrap();
        let t = Instant::now();
        assert!(named.take(Duration::from_millis(150)).is_none());
        assert!(t.elapsed() >= Duration::from_millis(140), "{:?}", t.elapsed());
        release.0.send(()).unwrap();
        holder.join().unwrap();
        assert!(named.take(Duration::from_secs(2)).is_some(), "let go: ours");
    }

    /// The real Credential Manager (Windows only), under names of these
    /// tests' own (never Yap's), removed however they end.
    #[cfg(windows)]
    mod real {
        use super::super::*;

        struct Names {
            service: String,
            users: Vec<String>,
        }

        impl Drop for Names {
            fn drop(&mut self) {
                for u in &self.users {
                    let _ = store::System.remove(&self.service, u);
                }
            }
        }

        /// The real-store tests' turn, across test binaries (the connectors'
        /// vault tests take the same lock file).
        fn real_store_turn() -> std::fs::File {
            let path = std::env::temp_dir().join("yap-unit-tests-credential-manager.lock");
            let turn = std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(&path).expect("the lock file");
            turn.lock().expect("a turn at the credential store");
            turn
        }

        const ROUNDS: usize = 60;
        const CHILD: &str = "YAP_CREDENTIALS_STRESS_CHILD";

        /// How one side writes and reads.
        #[derive(Clone, Copy, Debug, PartialEq)]
        enum Way {
            /// Keyring's own calls, no turn, no read-back (Yap before 8 Nov 2026).
            Raw,
            /// This module's writes, read without a turn.
            UnlockedRead,
            /// This module, writes and reads.
            Module,
        }

        impl Way {
            const ALL: [Way; 3] = [Way::Raw, Way::UnlockedRead, Way::Module];

            fn name(self) -> &'static str {
                match self {
                    Way::Raw => "raw",
                    Way::UnlockedRead => "unlocked-read",
                    Way::Module => "module",
                }
            }
            fn from(name: &str) -> Way {
                Way::ALL.into_iter().find(|w| w.name() == name).expect("a way")
            }
        }

        /// One side of the stress: `ROUNDS` writes of fresh values under its
        /// own name, each read back after a moment; the losses counted (a
        /// write that came back an error counts too).
        fn stress_side(service: &str, user: &str, way: Way) -> usize {
            let mut lost = 0;
            for i in 0..ROUNDS {
                let value = format!("{user}-{i}-{}", "v".repeat(300 + i * 7));
                let ok = match way {
                    Way::Raw => store::raw_set(service, user, &value).is_ok(),
                    Way::UnlockedRead | Way::Module => write(service, user, &value).is_ok(),
                };
                std::thread::sleep(Duration::from_millis(2));
                let back = match way {
                    Way::Raw | Way::UnlockedRead => store::System.get(service, user),
                    Way::Module => read(service, user),
                };
                if !ok || back.ok().flatten().as_deref() != Some(value.as_str()) {
                    lost += 1;
                }
            }
            lost
        }

        /// The stress test's other processes (run only by it).
        #[test]
        #[ignore = "run by the stress test in a process of its own"]
        fn credential_stress_child() {
            let Ok(spec) = std::env::var(CHILD) else { return };
            let mut it = spec.splitn(3, '|');
            let (service, user, way) = (it.next().unwrap(), it.next().unwrap(), Way::from(it.next().unwrap_or("")));
            let lost = stress_side(service, user, way);
            println!("\nSTRESS-LOST={lost}");
        }

        /// **The real Credential Manager under stress**: this process and two
        /// more write and read names of their own at once, `ROUNDS` each — as
        /// Yap did before (keyring's calls, no turn), with this module's
        /// writes but reads without a turn (both counted and printed: a few
        /// in a thousand runs' reads come back wrong), then through this
        /// module (none lost).
        #[test]
        fn many_yaps_writing_at_once_lose_nothing() {
            let _turn = real_store_turn();
            let service = format!("com.yap.test-credentials-{}", std::process::id());
            let users: Vec<String> = (0..3).map(|n| format!("yap-credtest-{n}")).collect();
            let _names = Names { service: service.clone(), users: users.clone() };
            let exe = std::env::current_exe().unwrap();
            let run = |way: Way| -> Vec<usize> {
                let kids: Vec<_> = users[1..]
                    .iter()
                    .map(|u| {
                        std::process::Command::new(&exe)
                            .args(["shell::credentials::tests::real::credential_stress_child", "--exact", "--ignored", "--nocapture", "--test-threads=1"])
                            .env(CHILD, format!("{service}|{u}|{}", way.name()))
                            .stdout(std::process::Stdio::piped())
                            .stderr(std::process::Stdio::null())
                            .spawn()
                            .expect("the stress test's other process")
                    })
                    .collect();
                let mine = stress_side(&service, &users[0], way);
                // Every other process done before anything can fail (so the
                // names are removed after their last write).
                let outs: Vec<String> = kids.into_iter().map(|k| k.wait_with_output().map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default()).collect();
                let mut lost = vec![mine];
                for text in outs {
                    let n = text.split("STRESS-LOST=").nth(1).and_then(|r| r.split_whitespace().next()).and_then(|n| n.parse().ok());
                    lost.push(n.unwrap_or_else(|| panic!("the other process's count: {text:?}")));
                }
                lost
            };
            let lost: Vec<(Way, Vec<usize>)> = Way::ALL.into_iter().map(|w| (w, run(w))).collect();
            println!("credential stress, {ROUNDS} rounds × 3 processes, lost: {lost:?}");
            assert_eq!(lost[2].1, [0, 0, 0], "lost through the module (all: {lost:?})");
        }
    }
}
