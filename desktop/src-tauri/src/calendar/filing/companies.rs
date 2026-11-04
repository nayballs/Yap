//! **Folder rules by company** (2026-10-31; Wispr Flow's "Sync meetings
//! with example.com to Clients?"): "Meetings with people from example.com
//! go in Clients" — every meeting note whose calendar invite has someone
//! from that company on it goes to that folder: each new one as the
//! calendar makes it or a recording ties it to its meeting
//! (`filing::folder_for_meeting`, `filing::file_linked`), an imported
//! transcript with attendee emails ([`super::file_imported`]), and the past
//! ones when the person says Yes.
//!
//! **The company** is an attendee's email domain ([`domain_of`]), never
//! the person's own ([`super::super::own_domains`]: the calendars'
//! addresses, the addresses its meetings list as you, the Yap account) and
//! never a free-mail or shared one ([`SHARED`], one list, here). A meeting
//! with people from several companies goes by the one with most people on
//! it, then the first listed ([`domains`]).
//!
//! **When rules disagree**: a series rule ("Always file Weekly sync in
//! Design") beats a company rule, and the person's own move beats both
//! (`filing::may_file`: a rule only moves a note that sits where Yap put
//! it).
//!
//! **The offer** ([`after_move`]): the person files a meeting with people
//! from example.com in a folder of their own by hand (the Meetings view's
//! "File this meeting in…", the Notes view's Move to): "File every meeting
//! with example.com in Clients? (3 past ones too)" — **Yes** (the rule, and
//! the past ones moved, with Undo), **Just this one**, **Never for
//! example.com**. Once per domain and folder, never after Never, at most
//! one a day ([`may_offer`]), through the notice policy (`notice::place_tip`,
//! kind [`KIND`]: a suggestion, under Settings → General → Notifications'
//! switch): a toast in the main window, else a card on the Yap bar.
//!
//! **Kept** in `meeting_folders.json` beside the series rules: a rule is an
//! entry of `rules` under `company:<domain>` ([`rule_key`]), which an older
//! Yap keeps (it keeps every entry, and renames or drops it with its
//! folder) and never matches (no series is called that). The offers'
//! answers (`companyOffers`) and the meeting notes' domains (`noteDomains`,
//! by note id and its `createdTs`: notes.json's fields an older Yap would
//! drop, so not there) are fields an older Yap drops when it saves the
//! file: an offer may then come once more, and the past meetings it knew
//! the companies of are found again only while their invites are in the
//! calendar. A deleted note's domains go with it; Reset app clears them
//! (the rules stay, like the folders).
//!
//! Never a network call: only the calendar Yap already synced. Logs counts
//! and ids, never a domain or a folder.

use std::collections::{BTreeSet, HashMap};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};

use super::{save, with_rules, Rule, Rules, SAVER};
use crate::notes::Note;

/// The notice kind (`notice::rule`, `kinds::category`: a suggestion) and
/// the bar card's id.
pub const KIND: &str = "company-folder";
/// The main window's offer: `{ id, domain, folder, past, title, body }`.
const EVENT: &str = "yap-company-folder-offer";
/// The company rules changed (Settings' list follows).
pub const EVENT_CHANGED: &str = "yap-company-folders";
/// One offer a day.
pub const OFFER_GAP_SECS: u64 = 24 * 3600;
/// A company rule's key in `rules`.
pub const RULE_PREFIX: &str = "company:";
/// How long the bar's offer stays up unanswered.
const CARD_MS: u64 = 30_000;

/// Free-mail and shared domains: an address there says nothing about whose
/// company it is. One list, in one place. (Families with a domain per
/// country — hotmail.fr, yahoo.co.uk — are [`FAMILIES`].)
pub const SHARED: &[&str] = &[
    "gmail.com",
    "googlemail.com",
    "outlook.com",
    "hotmail.com",
    "live.com",
    "msn.com",
    "passport.com",
    "icloud.com",
    "me.com",
    "mac.com",
    "yahoo.com",
    "ymail.com",
    "rocketmail.com",
    "aol.com",
    "aim.com",
    "proton.me",
    "protonmail.com",
    "protonmail.ch",
    "pm.me",
    "gmx.com",
    "gmx.net",
    "web.de",
    "t-online.de",
    "mail.com",
    "email.com",
    "zoho.com",
    "zohomail.com",
    "yandex.com",
    "mail.ru",
    "fastmail.com",
    "fastmail.fm",
    "hey.com",
    "tutanota.com",
    "tuta.io",
    "qq.com",
    "163.com",
    "126.com",
    "naver.com",
    "btinternet.com",
    "sky.com",
    "virginmedia.com",
    "talktalk.net",
    "orange.fr",
    "free.fr",
    "laposte.net",
    "libero.it",
    "comcast.net",
    "verizon.net",
    "att.net",
    "duck.com",
];

/// Free-mail families with a domain per country: hotmail.fr, yahoo.co.uk,
/// outlook.de, live.nl, gmx.at…
const FAMILIES: &[&str] = &["hotmail", "outlook", "live", "yahoo", "gmx", "yandex", "aol"];

/// An address's domain, lowercased: "Sam Lee <Sam@Example.com>",
/// "mailto:sam@example.com" and "sam@example.com." → "example.com". `None`
/// for anything that isn't an address. Pure.
pub fn domain_of(address: &str) -> Option<String> {
    let a = address.trim();
    let a = match (a.rfind('<'), a.rfind('>')) {
        (Some(l), Some(r)) if l < r => &a[l + 1..r],
        _ => a,
    };
    let a = a.trim();
    let a = if a.len() >= 7 && a[..7].eq_ignore_ascii_case("mailto:") { &a[7..] } else { a };
    let (local, domain) = a.rsplit_once('@')?;
    let domain = domain.trim().trim_end_matches('.').to_lowercase();
    let labels: Vec<&str> = domain.split('.').collect();
    let ok = !local.trim().is_empty()
        && !local.contains(char::is_whitespace)
        && labels.len() >= 2
        && labels.iter().all(|l| !l.is_empty() && !l.starts_with('-') && !l.ends_with('-') && l.chars().all(|c| c.is_alphanumeric() || c == '-'));
    ok.then_some(domain)
}

/// A free-mail or shared domain ([`SHARED`], [`FAMILIES`]), or a calendar's
/// own (rooms, groups: `….calendar.google.com`). Pure.
pub fn is_shared(domain: &str) -> bool {
    let d = domain.trim().to_lowercase();
    if SHARED.contains(&d.as_str()) || d == "calendar.google.com" || d.ends_with(".calendar.google.com") {
        return true;
    }
    // "hotmail.fr", "yahoo.co.uk", "outlook.com.au": the family, then a
    // country's one or two short labels.
    FAMILIES.iter().any(|f| {
        d.strip_prefix(f).and_then(|r| r.strip_prefix('.')).is_some_and(|rest| {
            let parts: Vec<&str> = rest.split('.').collect();
            (1..=2).contains(&parts.len()) && parts.iter().all(|p| (2..=3).contains(&p.len()) && p.chars().all(|c| c.is_ascii_alphabetic()))
        })
    })
}

/// `domain` is one of `own` (the person's), or a part of one (uk.example.org
/// of example.org). Pure.
pub fn is_own(domain: &str, own: &[String]) -> bool {
    own.iter().any(|o| {
        let o = o.trim().to_lowercase();
        !o.is_empty() && (domain == o || domain.strip_suffix(o.as_str()).is_some_and(|rest| rest.ends_with('.')))
    })
}

/// The companies in `emails` (attendees' addresses): their domains, free
/// mail left out, the one with most people first, then as listed. The
/// person's own are still in (they're taken out where it's used: [`external`],
/// as Yap may learn them later). Pure.
pub fn domains<S: AsRef<str>>(emails: &[S]) -> Vec<String> {
    let mut counts: HashMap<String, (usize, usize)> = HashMap::new();
    for (i, e) in emails.iter().enumerate() {
        if let Some(d) = domain_of(e.as_ref()).filter(|d| !is_shared(d)) {
            counts.entry(d).or_insert((0, i)).0 += 1;
        }
    }
    let mut out: Vec<(String, (usize, usize))> = counts.into_iter().collect();
    out.sort_by(|a, b| b.1 .0.cmp(&a.1 .0).then(a.1 .1.cmp(&b.1 .1)));
    out.into_iter().map(|(d, _)| d).collect()
}

/// `domains` without the person's own (`own`), in order. Pure.
pub fn external(domains: &[String], own: &[String]) -> Vec<String> {
    domains.iter().filter(|d| !is_shared(d) && !is_own(d, own)).cloned().collect()
}

/// A company rule's key in `rules`.
pub fn rule_key(domain: &str) -> String {
    format!("{RULE_PREFIX}{}", domain.trim().to_lowercase())
}

/// The company of a `rules` key, if it's a company rule's.
pub fn domain_of_key(key: &str) -> Option<&str> {
    key.strip_prefix(RULE_PREFIX).filter(|d| !d.is_empty())
}

fn asked_key(domain: &str, folder: &str) -> String {
    format!("{}|{}", domain.trim().to_lowercase(), folder.trim().to_lowercase())
}

/// The offers' answers and when the last went up (`companyOffers` in
/// meeting_folders.json).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Offers {
    /// When the last offer showed (unix seconds): one a day.
    #[serde(default)]
    pub last_ts: u64,
    /// "domain|folder" (lowercase) asked about: not again.
    #[serde(default)]
    pub asked: BTreeSet<String>,
    /// "Never for example.com": no offer for it again, whatever the folder.
    #[serde(default)]
    pub never: BTreeSet<String>,
}

impl Offers {
    pub fn is_empty(&self) -> bool {
        self.last_ts == 0 && self.asked.is_empty() && self.never.is_empty()
    }
}

/// A meeting note's companies (`noteDomains` in meeting_folders.json, by
/// note id): its attendees' domains as [`domains`] orders them, and the
/// note's `createdTs` (an id comes round again: a new note never takes an
/// old one's).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Seen {
    pub created_ts: u64,
    pub domains: Vec<String>,
}

/// The folder a note with companies `domains` (the person's own already out)
/// goes to by a company rule: the first company with one whose folder
/// still exists (`exists` gives the folder as the list spells it), with
/// its domain. Pure over the rules.
pub fn rule_for(rules: &Rules, domains: &[String], exists: impl Fn(&str) -> Option<String>) -> Option<(String, String)> {
    domains.iter().find_map(|d| {
        let rule = rules.rules.get(&rule_key(d))?;
        exists(&rule.folder).map(|f| (d.clone(), f))
    })
}

/// Whether the offer for `domain` and `folder` may go up at `now`: no rule
/// for the company yet, not Never, not asked for this folder, and none
/// shown in the last day. Pure.
pub fn may_offer(rules: &Rules, domain: &str, folder: &str, now: u64) -> Result<(), &'static str> {
    let o = &rules.company_offers;
    if rules.rules.contains_key(&rule_key(domain)) {
        return Err("the company has a rule");
    }
    if o.never.contains(&domain.to_lowercase()) {
        return Err("never for the company");
    }
    if o.asked.contains(&asked_key(domain, folder)) {
        return Err("asked already");
    }
    if o.last_ts != 0 && now < o.last_ts.saturating_add(OFFER_GAP_SECS) {
        return Err("one a day");
    }
    Ok(())
}

/// The note's companies, the person's own still in: what Yap kept when it
/// was tied to its meeting, else its invite while the calendar still has it
/// (`calendar_emails`, by its event's key).
pub fn domains_of_note(rules: &Rules, note: &Note, calendar_emails: impl Fn(&str) -> Option<Vec<String>>) -> Vec<String> {
    if let Some(seen) = rules.note_domains.get(&note.id.to_string()).filter(|s| s.created_ts == note.created_ts) {
        return seen.domains.clone();
    }
    note.event.as_ref().and_then(|e| calendar_emails(&e.key)).map(|emails| domains(&emails)).unwrap_or_default()
}

/// The past meeting notes a Yes for `domain` moves into `folder`, newest
/// first: with someone from the company, not `except` (the one the person
/// filed), sitting where Yap put them (`filing::may_file`: never one the
/// person moved, or one in a folder of their own or its series'), and not
/// of a series with a rule of its own (`series_ruled`: it beats this).
/// Pure.
pub fn past_for(
    notes: &[Note],
    domain: &str,
    folder: &str,
    except: u64,
    companies: impl Fn(&Note) -> Vec<String>,
    series_ruled: impl Fn(&Note) -> bool,
) -> Vec<u64> {
    let mut found: Vec<&Note> = notes
        .iter()
        .filter(|n| n.id != except && super::may_file(n, folder))
        .filter(|n| companies(n).iter().any(|d| d == domain))
        .filter(|n| !series_ruled(n))
        .collect();
    found.sort_by_key(|n| std::cmp::Reverse(n.event.as_ref().map_or(n.created_ts, |e| e.start)));
    found.iter().map(|n| n.id).collect()
}

/// A note a Yes moved: where it was, so Undo puts it back.
#[derive(Debug, Clone, PartialEq)]
pub struct Moved {
    pub id: u64,
    pub created_ts: u64,
    pub from: String,
    pub chosen: bool,
}

/// Move notes `ids` into `folder` (filed: a rule never moves them again),
/// leaving their edit times alone. Returns what moved.
pub fn move_into(ids: &[u64], folder: &str) -> Vec<Moved> {
    let mut moved = Vec::new();
    for &id in ids {
        let done = crate::notes::modify_quietly(id, |n| {
            if n.folder.trim().eq_ignore_ascii_case(folder) {
                return Ok((None, false));
            }
            let m = Moved { id, created_ts: n.created_ts, from: n.folder.clone(), chosen: n.folder_chosen };
            n.folder = folder.to_string();
            n.folder_chosen = true;
            Ok((Some(m), true))
        });
        if let Ok(Some(m)) = done {
            moved.push(m);
        }
    }
    moved
}

/// Undo [`move_into`]: each note still in `folder` (and still the same
/// note) goes back where it was, as it was. Returns the ids put back.
pub fn put_back(moved: &[Moved], folder: &str) -> Vec<u64> {
    let mut back = Vec::new();
    for m in moved {
        let done = crate::notes::modify_quietly(m.id, |n| {
            if n.created_ts != m.created_ts || !n.folder.trim().eq_ignore_ascii_case(folder) {
                return Ok((false, false));
            }
            n.folder = m.from.clone();
            n.folder_chosen = m.chosen;
            Ok((true, true))
        });
        if done == Ok(true) {
            back.push(m.id);
        }
    }
    back
}

// ---- what the notes' companies are -------------------------------------------------------------

/// The person's own domains (the calendars' and the account's), and the
/// calendar's invites by key: read before the rules' lock, never under it.
fn own() -> Vec<String> {
    super::super::own_domains()
}

/// Keep note `note_id`'s companies, from its attendees' `emails` (it was
/// just tied to its meeting, or imported with them).
pub fn remember(note_id: u64, emails: &[String]) {
    let found = domains(emails);
    if found.is_empty() {
        return;
    }
    let Some(created_ts) = crate::notes::get(note_id).map(|n| n.created_ts) else { return };
    let seen = Seen { created_ts, domains: found };
    with_rules(|r| {
        let key = note_id.to_string();
        if r.note_domains.get(&key) != Some(&seen) {
            r.note_domains.insert(key, seen);
            save(r);
        }
    });
}

/// A note deleted: its companies go with it.
pub fn forget_note(note_id: u64) {
    with_rules(|r| {
        if r.note_domains.remove(&note_id.to_string()).is_some() {
            save(r);
        }
    });
}

/// Reset app: every note went, and their companies with them (the rules
/// and the offers' answers stay, like the folders).
pub fn forget_notes() {
    with_rules(|r| {
        if !r.note_domains.is_empty() {
            r.note_domains.clear();
            save(r);
        }
    });
}

/// Companies kept for notes that are gone (or whose id a new note took).
fn prune(r: &mut Rules, notes: &[Note]) -> bool {
    let live: HashMap<String, u64> = notes.iter().map(|n| (n.id.to_string(), n.created_ts)).collect();
    let before = r.note_domains.len();
    r.note_domains.retain(|id, seen| live.get(id) == Some(&seen.created_ts));
    r.note_domains.len() != before
}

/// The folder a meeting with attendees `emails` goes to by a company rule
/// (the folder as the list spells it), with its company.
pub fn folder_for(emails: &[String]) -> Option<(String, String)> {
    let companies = external(&domains(emails), &own());
    if companies.is_empty() {
        return None;
    }
    let rules = with_rules(|r| r.clone());
    rule_for(&rules, &companies, crate::notes::find_folder)
}

/// The past notes a Yes for `domain` would move into `folder` now.
fn past_now(rules: &Rules, notes: &[Note], domain: &str, folder: &str, except: u64, own: &[String]) -> Vec<u64> {
    let emails = |key: &str| super::super::attendee_emails(key);
    past_for(
        notes,
        domain,
        folder,
        except,
        |n| external(&domains_of_note(rules, n, emails), own),
        |n| super::series_of_note(n).is_some_and(|s| rules.rules.contains_key(&s)),
    )
}

// ---- the offer -------------------------------------------------------------------------------

/// The offer on screen.
#[derive(Debug, Clone)]
struct Pending {
    id: u64,
    domain: String,
    folder: String,
    note_id: u64,
    on_bar: bool,
}

/// What the last Yes did, for its Undo.
#[derive(Debug, Clone)]
struct Undo {
    id: u64,
    domain: String,
    folder: String,
    /// The company's rule before (none: the Yes made it).
    before: Option<Rule>,
    moved: Vec<Moved>,
}

static PENDING: Mutex<Option<Pending>> = Mutex::new(None);
static UNDO: Mutex<Option<Undo>> = Mutex::new(None);
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// "File every meeting with example.com in Clients? (3 past ones too)" and
/// its line. Pure.
pub fn offer_text(domain: &str, folder: &str, past: usize) -> (String, String) {
    let too = match past {
        0 => String::new(),
        1 => " (1 past one too)".to_string(),
        n => format!(" ({n} past ones too)"),
    };
    (
        format!("File every meeting with {domain} in {folder}?{too}"),
        format!("Yap goes by who's on the invite. New meetings with {domain} go there as they're made."),
    )
}

/// The person filed meeting note `note_id` in `folder` by hand (the
/// Meetings view, the Notes view): offer a rule for its company, off this
/// thread (it looks at the notes and the windows).
pub fn after_move(app: &AppHandle, note_id: u64, folder: &str) {
    let (app, folder) = (app.clone(), folder.to_string());
    std::thread::spawn(move || {
        if let Err(why) = offer(&app, note_id, &folder) {
            tracing::debug!(note_id, why, "company folders: not offered");
        }
    });
}

fn offer(app: &AppHandle, note_id: u64, folder: &str) -> Result<(), &'static str> {
    let folder = crate::notes::find_folder(folder).ok_or("no such folder")?;
    if crate::notes::is_protected_folder(&folder) {
        return Err("Meetings or Personal");
    }
    let note = crate::notes::get(note_id).ok_or("no such note")?;
    if !note.folder.trim().eq_ignore_ascii_case(&folder) {
        return Err("moved again");
    }
    let own = own();
    let rules = with_rules(|r| r.clone());
    let companies = external(&domains_of_note(&rules, &note, super::super::attendee_emails), &own);
    if companies.is_empty() {
        return Err("no company on the invite");
    }
    let now = now_secs();
    // The first company it may be asked for (one with a rule, or Never, or
    // asked about already, gives way to the next on the invite).
    let mut why = "no company to ask about";
    let mut domain = None;
    for d in &companies {
        match may_offer(&rules, d, &folder, now) {
            Ok(()) => {
                domain = Some(d.clone());
                break;
            }
            Err("one a day") => return Err("one a day"),
            Err(w) => why = w,
        }
    }
    let domain = domain.ok_or(why)?;
    let past = past_now(&rules, &crate::notes::all(), &domain, &folder, note_id, &own).len();
    let place = crate::notices::notice::place_tip(app, KIND);
    if place == crate::notices::notice::Place::Nowhere {
        return Err("nowhere to show it");
    }
    let in_window = place == crate::notices::notice::Place::Window;
    let id = NEXT_ID.fetch_add(1, Ordering::SeqCst);
    let (title, body) = offer_text(&domain, &folder, past);
    *PENDING.lock().unwrap_or_else(|p| p.into_inner()) =
        Some(Pending { id, domain: domain.clone(), folder: folder.clone(), note_id, on_bar: !in_window });
    let shown = if in_window {
        app.emit(EVENT, json!({ "id": id, "domain": domain, "folder": folder, "past": past, "title": title, "body": body })).is_ok()
    } else {
        use crate::bar::{Card, CardAction};
        let card = Card {
            id: KIND.into(),
            icon: "notes",
            title,
            body,
            primary: Some(CardAction::new("yes", "Yes")),
            secondary: Some(CardAction::new("one", "Just this one")),
            link: Some(CardAction::new("never", format!("Never for {domain}"))),
            timeout_ms: Some(CARD_MS),
            expire_action: Some("expire".into()),
            close_action: Some("one".into()),
            ..Default::default()
        };
        crate::bar::show_card(
            app,
            card,
            Box::new(move |app, action| {
                if let Err(e) = respond(app, id, action) {
                    tracing::warn!("company folders: {e}");
                }
            }),
        )
    };
    let shown = crate::notices::notice::tip_shown(KIND, place, shown);
    if !shown {
        let mut p = PENDING.lock().unwrap_or_else(|p| p.into_inner());
        if p.as_ref().is_some_and(|p| p.id == id) {
            *p = None;
        }
        return Err("not shown");
    }
    with_rules(|r| {
        r.company_offers.last_ts = now;
        save(r);
    });
    tracing::info!(note_id, past, in_window, "company folders: offered");
    Ok(())
}

/// What a Yes did: `{ domain, folder, moved, undo }`.
fn yes(app: &AppHandle, p: &Pending) -> Result<Value, String> {
    let folder = crate::notes::find_folder(&p.folder).ok_or("That folder isn't there any more.")?;
    let own = own();
    let notes = crate::notes::all();
    let (before, rules) = with_rules(|r| {
        prune(r, &notes);
        let rule = Rule { folder: folder.clone(), title: p.domain.clone(), created_ts: now_secs() };
        let before = r.rules.insert(rule_key(&p.domain), rule);
        r.company_offers.asked.insert(asked_key(&p.domain, &folder));
        save(r);
        (before, r.clone())
    });
    SAVER.check()?;
    // The calendar's invites are read outside the rules' lock.
    let ids = past_now(&rules, &notes, &p.domain, &folder, p.note_id, &own);
    let moved = move_into(&ids, &folder);
    let ids: Vec<u64> = moved.iter().map(|m| m.id).collect();
    super::notes_changed(app, &ids);
    let _ = app.emit(EVENT_CHANGED, ());
    let undo = NEXT_ID.fetch_add(1, Ordering::SeqCst);
    *UNDO.lock().unwrap_or_else(|p| p.into_inner()) =
        Some(Undo { id: undo, domain: p.domain.clone(), folder: folder.clone(), before, moved: moved.clone() });
    tracing::info!(note_id = p.note_id, moved = moved.len(), "company folders: a company's meetings filed in a folder");
    crate::notes::saved()?;
    Ok(json!({ "domain": p.domain, "folder": folder, "moved": moved.len(), "undo": undo }))
}

/// An answer to offer `id`: "yes", "one" (Just this one, or ✕), "never"
/// (Never for the company), "expire" (unanswered: nothing kept). A Yes
/// returns `{ domain, folder, moved, undo }`; the rest null.
fn respond(app: &AppHandle, id: u64, answer: &str) -> Result<Value, String> {
    let taken = {
        let mut p = PENDING.lock().unwrap_or_else(|p| p.into_inner());
        match p.as_ref() {
            Some(open) if open.id == id => p.take(),
            _ => None,
        }
    };
    let Some(p) = taken else {
        return match answer {
            "yes" => Err("That suggestion is no longer open".into()),
            _ => Ok(Value::Null),
        };
    };
    tracing::info!(answer, "company folders: answered");
    match answer {
        "yes" => {
            let done = yes(app, &p)?;
            if p.on_bar {
                let moved = done["moved"].as_u64().unwrap_or(0);
                let undo = done["undo"].as_u64().unwrap_or(0);
                let card = crate::bar::Card {
                    id: format!("{KIND}-filed"),
                    icon: "notes",
                    title: format!("Meetings with {} go in {}", p.domain, p.folder),
                    body: filed_line(moved as usize, &p.folder),
                    primary: Some(crate::bar::CardAction::new("undo", "Undo")),
                    timeout_ms: Some(8_000),
                    ..Default::default()
                };
                crate::bar::show_card(
                    app,
                    card,
                    Box::new(move |app, action| {
                        if action == "undo" {
                            if let Err(e) = undo_now(app, undo) {
                                tracing::warn!("company folders: {e}");
                            }
                        }
                    }),
                );
            }
            Ok(done)
        }
        "one" | "never" => {
            with_rules(|r| {
                if answer == "never" {
                    r.company_offers.never.insert(p.domain.to_lowercase());
                } else {
                    r.company_offers.asked.insert(asked_key(&p.domain, &p.folder));
                }
                save(r);
            });
            SAVER.check()?;
            Ok(Value::Null)
        }
        _ => Ok(Value::Null),
    }
}

/// "Moved 3 past meetings there." Pure.
pub fn filed_line(moved: usize, folder: &str) -> String {
    match moved {
        0 => format!("New ones go to {folder} as they're made."),
        1 => format!("Moved 1 past meeting to {folder} too."),
        n => format!("Moved {n} past meetings to {folder} too."),
    }
}

/// Undo the Yes numbered `undo`: the moved notes back where they were, the
/// rule gone. Returns how many went back.
fn undo_now(app: &AppHandle, undo: u64) -> Result<usize, String> {
    let taken = {
        let mut u = UNDO.lock().unwrap_or_else(|p| p.into_inner());
        match u.as_ref() {
            Some(open) if open.id == undo => u.take(),
            _ => None,
        }
    };
    let u = taken.ok_or("Nothing to undo")?;
    let back = put_back(&u.moved, &u.folder);
    with_rules(|r| {
        let key = rule_key(&u.domain);
        // Only the rule the Yes made (not one changed since).
        if r.rules.get(&key).is_some_and(|rule| rule.folder.eq_ignore_ascii_case(&u.folder)) {
            match &u.before {
                Some(rule) => r.rules.insert(key, rule.clone()),
                None => r.rules.remove(&key),
            };
            save(r);
        }
    });
    SAVER.check()?;
    super::notes_changed(app, &back);
    let _ = app.emit(EVENT_CHANGED, ());
    tracing::info!(back = back.len(), "company folders: undone");
    crate::notes::saved()?;
    Ok(back.len())
}

/// Test mode only (`notice::e2e_notice_reset`, before every test): the
/// day's offer forgotten, like the notice policy's other cooldowns.
#[cfg(debug_assertions)]
pub fn e2e_reset() {
    with_rules(|r| {
        if r.company_offers.last_ts != 0 {
            r.company_offers.last_ts = 0;
            save(r);
        }
    });
}

// ---- commands ------------------------------------------------------------------------------

/// The main window's answer to its offer (`yap-company-folder-offer`):
/// `answer` = "yes" | "one" | "never" | "expire".
#[tauri::command]
pub fn company_folder_respond(app: AppHandle, id: u64, answer: String) -> Result<Value, String> {
    respond(&app, id, &answer)
}

/// Undo a Yes (its `undo` number): returns how many notes went back.
#[tauri::command]
pub fn company_folder_undo(app: AppHandle, undo: u64) -> Result<usize, String> {
    undo_now(&app, undo)
}

/// The company rules, by company: `[{ domain, folder, createdTs }]`.
#[tauri::command]
pub fn company_folders() -> Vec<Value> {
    with_rules(|r| list(r))
}

/// The company rules in `r`, by company. Pure.
pub fn list(r: &Rules) -> Vec<Value> {
    r.rules
        .iter()
        .filter_map(|(k, rule)| {
            let domain = domain_of_key(k)?;
            (!rule.folder.trim().is_empty()).then(|| json!({ "domain": domain, "folder": rule.folder, "createdTs": rule.created_ts }))
        })
        .collect()
}

/// Remove the rule for `domain` (Settings): its meetings stay where they are.
#[tauri::command]
pub fn company_folder_remove(app: AppHandle, domain: String) -> Result<(), String> {
    let gone = with_rules(|r| {
        let gone = r.rules.remove(&rule_key(&domain)).is_some();
        if gone {
            save(r);
        }
        gone
    });
    if gone {
        tracing::info!("company folders: a company rule removed");
        let _ = app.emit(EVENT_CHANGED, ());
        let _ = app.emit("yap-notes-changed", ());
    }
    Ok(SAVER.check()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// "sam@" + a free-mail domain, built here: the repo is public, and the
    /// privacy guard keeps real-looking addresses out of it.
    fn at(local: &str, domain: &str) -> String {
        format!("{local}@{domain}")
    }

    #[test]
    fn an_address_gives_its_domain() {
        assert_eq!(domain_of("sam@example.com").as_deref(), Some("example.com"));
        assert_eq!(domain_of("  Sam Lee <Sam@Example.COM>  ").as_deref(), Some("example.com"));
        assert_eq!(domain_of("mailto:priya@eu.example.org").as_deref(), Some("eu.example.org"));
        assert_eq!(domain_of("MAILTO:priya@example.org.").as_deref(), Some("example.org"));
        for not in ["", "sam", "@example.com", "sam@", "sam@localhost", "sam@exa mple.com", "sam lee@example.com", "sam@.example.com", "sam@-x.example.com"] {
            assert_eq!(domain_of(not), None, "{not}");
        }
    }

    #[test]
    fn free_mail_and_shared_domains_say_nothing_about_a_company() {
        for d in ["gmail.com", "googlemail.com", "outlook.com", "hotmail.com", "icloud.com", "yahoo.com", "proton.me", "GMAIL.COM"] {
            assert!(is_shared(d), "{d}");
        }
        // Country domains of the big families, and the calendars' own.
        for d in ["hotmail.fr", "yahoo.co.uk", "outlook.com.au", "live.nl", "gmx.de", "resource.calendar.google.com", "group.calendar.google.com"] {
            assert!(is_shared(d), "{d}");
        }
        // A company's, even one starting like a family.
        for d in ["example.com", "example.org", "liveexample.com", "outlook.example.com", "yahoo.example.org", "hotmail.example"] {
            assert!(!is_shared(d), "{d}");
        }
        assert!(domains(&[at("sam", "gmail.com"), at("ali", "hotmail.co.uk"), at("jo", "icloud.com")]).is_empty());
    }

    #[test]
    fn a_meetings_companies_most_people_first_and_your_own_left_out() {
        let emails = [
            "priya@example.net".to_string(),
            at("ali", "gmail.com"),
            "sam@example.com".to_string(),
            "jo@example.com".to_string(),
            "imogen@example.org".to_string(),
            "lee@uk.example.org".to_string(),
            "not an address".to_string(),
        ];
        // Two from example.com, then the rest as listed; free mail out.
        assert_eq!(domains(&emails), ["example.com", "example.net", "example.org", "uk.example.org"]);
        // Your own company (and a part of it) left out.
        let own = vec!["Example.org".to_string()];
        assert_eq!(external(&domains(&emails), &own), ["example.com", "example.net"]);
        assert!(is_own("example.org", &own));
        assert!(is_own("uk.example.org", &own));
        assert!(!is_own("notexample.org", &own));
        assert!(!is_own("example.org", &[]));
        // A meeting with only colleagues has no company to file by.
        assert!(external(&domains(&["sam@example.org".to_string()]), &own).is_empty());
    }

    fn rules_with(entries: &[(&str, &str)]) -> Rules {
        let mut r = Rules::default();
        for (k, f) in entries {
            r.rules.insert(k.to_string(), Rule { folder: f.to_string(), title: String::new(), created_ts: 1 });
        }
        r
    }

    #[test]
    fn a_company_rule_files_by_the_first_company_with_one() {
        let r = rules_with(&[("company:example.com", "Clients"), ("company:example.net", "Partners"), ("weekly@example.com", "Design")]);
        let exists = |f: &str| ["Clients", "Partners", "Design"].iter().find(|x| x.eq_ignore_ascii_case(f)).map(|x| x.to_string());
        let found = rule_for(&r, &["example.net".into(), "example.com".into()], exists);
        assert_eq!(found, Some(("example.net".into(), "Partners".into())));
        assert_eq!(rule_for(&r, &["example.org".into(), "example.com".into()], exists), Some(("example.com".into(), "Clients".into())));
        // A series id is never taken for a company.
        assert_eq!(rule_for(&r, &["weekly@example.com".into()], exists), None);
        // A folder that's gone: no rule (the next company's, if any).
        let gone = |f: &str| (f == "Partners").then(|| f.to_string());
        assert_eq!(rule_for(&r, &["example.com".into(), "example.net".into()], gone), Some(("example.net".into(), "Partners".into())));
        // The Settings list: company rules only.
        let listed = list(&r);
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0]["domain"], "example.com");
        assert_eq!(listed[0]["folder"], "Clients");
    }

    #[test]
    fn the_offer_comes_once_per_company_and_folder_and_once_a_day() {
        let day = OFFER_GAP_SECS;
        let mut r = Rules::default();
        assert_eq!(may_offer(&r, "example.com", "Clients", 10 * day), Ok(()));
        // One went up: nothing else for a day.
        r.company_offers.last_ts = 10 * day;
        assert_eq!(may_offer(&r, "example.net", "Partners", 10 * day + day - 1), Err("one a day"));
        assert_eq!(may_offer(&r, "example.net", "Partners", 11 * day), Ok(()));
        // Just this one: not again for that folder; another folder may ask.
        r.company_offers.asked.insert(asked_key("example.com", "Clients"));
        assert_eq!(may_offer(&r, "example.com", "clients", 20 * day), Err("asked already"));
        assert_eq!(may_offer(&r, "example.com", "Accounts", 20 * day), Ok(()));
        // Never for the company: whatever the folder.
        r.company_offers.never.insert("example.com".into());
        assert_eq!(may_offer(&r, "Example.com", "Accounts", 20 * day), Err("never for the company"));
        // A rule already.
        r.rules.insert(rule_key("example.net"), Rule { folder: "Partners".into(), title: String::new(), created_ts: 1 });
        assert_eq!(may_offer(&r, "example.net", "Other", 20 * day), Err("the company has a rule"));
        assert_eq!(offer_text("example.com", "Clients", 3).0, "File every meeting with example.com in Clients? (3 past ones too)");
        assert_eq!(offer_text("example.com", "Clients", 1).0, "File every meeting with example.com in Clients? (1 past one too)");
        assert_eq!(offer_text("example.com", "Clients", 0).0, "File every meeting with example.com in Clients?");
    }

    fn meeting(id: u64, folder: &str, key: &str, start: u64) -> Note {
        let mut n: Note = serde_json::from_value(json!({
            "id": id, "title": format!("Meeting {id}"), "createdTs": start, "updatedTs": start, "folder": folder, "noteType": "meeting",
        }))
        .unwrap();
        n.event = Some(crate::notes::NoteEvent {
            key: key.into(),
            title: format!("Meeting {id}"),
            start,
            end: start + 1_800,
            description: String::new(),
            service: String::new(),
        });
        n
    }

    #[test]
    fn the_past_ones_a_yes_moves() {
        let mut r = rules_with(&[("weekly@example.com", "Design")]);
        let seen = |ts: u64, d: &[&str]| Seen { created_ts: ts, domains: d.iter().map(|s| s.to_string()).collect() };
        r.note_domains.insert("1".into(), seen(100, &["example.com"]));
        r.note_domains.insert("2".into(), seen(200, &["example.org", "example.com"]));
        r.note_domains.insert("3".into(), seen(300, &["example.com"]));
        r.note_domains.insert("4".into(), seen(400, &["example.net"]));
        r.note_domains.insert("6".into(), seen(999, &["example.com"])); // another note had id 6
        let mut chosen = meeting(5, "Meetings", "1:one-off-5", 500);
        chosen.folder_chosen = true;
        r.note_domains.insert("5".into(), seen(500, &["example.com"]));
        let notes = vec![
            meeting(1, "Meetings", "1:one-off-1", 100),
            meeting(2, "Personal", "1:one-off-2", 200),
            // Of a series with its own rule: the series' rule wins.
            meeting(3, "Meetings", "1:weekly@example.com:1791800000", 300),
            meeting(4, "Meetings", "1:one-off-4", 400),
            chosen,
            meeting(6, "Meetings", "1:one-off-6", 600),
            // The calendar still has its invite (no domains kept for it).
            meeting(7, "Meetings", "1:one-off-7", 700),
            // The one the person just filed.
            meeting(8, "Clients", "1:one-off-8", 800),
            // In a folder of the person's own.
            meeting(9, "Hiring", "1:one-off-9", 900),
        ];
        r.note_domains.insert("9".into(), seen(900, &["example.com"]));
        let calendar = |key: &str| (key == "1:one-off-7").then(|| vec!["sam@example.com".to_string()]);
        let own = vec!["example.org".to_string()];
        let ids = past_for(
            &notes,
            "example.com",
            "Clients",
            8,
            |n| external(&domains_of_note(&r, n, calendar), &own),
            |n| super::super::series_of_note(n).is_some_and(|s| r.rules.contains_key(&s)),
        );
        // Newest first: 7 (by its invite), 2 and 1. Not 3 (its series' rule),
        // 4 (another company), 5 (the person moved it), 6 (its id's earlier
        // note was the one with example.com), 8 (filed), 9 (their folder).
        assert_eq!(ids, [7, 2, 1]);
        // Pruned: the companies of notes that are gone, or of an id's earlier note.
        let mut pruned = r.clone();
        assert!(prune(&mut pruned, &notes));
        assert!(!pruned.note_domains.contains_key("6"));
        assert!(pruned.note_domains.contains_key("1"));
    }

    /// Yes moves the past ones (filed: not moved by a rule again), Undo puts
    /// each back as it was — not one the person moved again meanwhile.
    #[test]
    fn a_yes_moves_the_past_ones_and_undo_puts_them_back() {
        crate::notes::folder_create("Clients");
        let a = crate::notes::create("Supplier call", "", "calendar", crate::notes::MEETINGS);
        let b = crate::notes::create("Pricing review", "", "calendar", crate::notes::PERSONAL);
        let c = crate::notes::create("Kick-off", "", "calendar", crate::notes::MEETINGS);
        let moved = move_into(&[a.id, b.id, c.id], "Clients");
        assert_eq!(moved.len(), 3);
        for id in [a.id, b.id, c.id] {
            let n = crate::notes::get(id).unwrap();
            assert_eq!(n.folder, "Clients");
            assert!(n.folder_chosen);
        }
        // Their edit times stay: they don't float to the top as edited now.
        assert_eq!(crate::notes::get(a.id).unwrap().updated_ts, a.updated_ts);
        // The person moves one elsewhere before Undo: it stays there.
        crate::notes::update(c.id, None, None, Some(crate::notes::PERSONAL.into()), None).unwrap();
        let back = put_back(&moved, "clients");
        assert_eq!(back, [a.id, b.id]);
        let (a2, b2, c2) = (crate::notes::get(a.id).unwrap(), crate::notes::get(b.id).unwrap(), crate::notes::get(c.id).unwrap());
        assert_eq!((a2.folder.as_str(), a2.folder_chosen), (crate::notes::MEETINGS, false));
        assert_eq!((b2.folder.as_str(), b2.folder_chosen), (crate::notes::PERSONAL, false));
        assert_eq!(c2.folder, crate::notes::PERSONAL);
        assert!(c2.folder_chosen);
        // A note already in the folder isn't "moved" (Undo leaves it).
        assert!(move_into(&[a.id], "Meetings").is_empty());
    }

    #[test]
    fn a_notes_companies_are_kept_and_go_with_it() {
        let n = crate::notes::create("Supplier call", "", "calendar", crate::notes::MEETINGS);
        remember(n.id, &["sam@example.com".into(), at("ali", "gmail.com"), "jo@example.com".into(), "imogen@example.org".into()]);
        let kept = with_rules(|r| r.note_domains.get(&n.id.to_string()).cloned()).unwrap();
        assert_eq!(kept, Seen { created_ts: n.created_ts, domains: vec!["example.com".into(), "example.org".into()] });
        // Nothing to keep: nothing kept.
        let m = crate::notes::create("Catch-up", "", "calendar", crate::notes::MEETINGS);
        remember(m.id, &[at("ali", "gmail.com")]);
        assert!(with_rules(|r| !r.note_domains.contains_key(&m.id.to_string())));
        forget_note(n.id);
        assert!(with_rules(|r| !r.note_domains.contains_key(&n.id.to_string())));
        // On disk beside the series rules, as fields an older Yap drops.
        remember(n.id, &["sam@example.com".into()]);
        with_rules(|r| {
            r.rules.insert(rule_key("example.com"), Rule { folder: "Clients".into(), title: "example.com".into(), created_ts: 1 });
            r.company_offers.never.insert("example.net".into());
        });
        let json = with_rules(|r| serde_json::to_value(&*r).unwrap());
        assert_eq!(json["rules"]["company:example.com"]["folder"], "Clients");
        assert_eq!(json["companyOffers"]["never"][0], "example.net");
        assert_eq!(json["noteDomains"][n.id.to_string()]["domains"][0], "example.com");
        // An older Yap reads the file as its own (`{rules}` only) and keeps
        // the company rule among them.
        #[derive(serde::Deserialize, serde::Serialize)]
        struct Older {
            rules: std::collections::BTreeMap<String, Rule>,
        }
        let older: Older = serde_json::from_value(json).unwrap();
        let saved = serde_json::to_value(&older).unwrap();
        let back: Rules = serde_json::from_value(saved).unwrap();
        assert_eq!(back.rules[&rule_key("example.com")].folder, "Clients");
        assert!(back.company_offers.is_empty() && back.note_domains.is_empty());
        forget_notes();
        assert!(with_rules(|r| r.note_domains.is_empty()));
        with_rules(|r| *r = Rules::default());
    }
}
