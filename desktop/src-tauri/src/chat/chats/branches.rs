//! **Branches** (15 Nov 2026, the Claude app's ‹ 1/2 ›): a conversation's
//! other versions from a message on. **Retry** under an answer sets the
//! answer (and everything after it) aside and the same question is answered
//! again; **Edit** on the person's message sets that message and everything
//! after it aside and the new words are asked in its place. What was set
//! aside stays one switch away: ‹ 1/2 › on the answer (a retry) or on the
//! person's message (an edit), as many as were made.
//!
//! **What an older Yap keeps.** The branch shown is the conversation's
//! `messages`, as ever: everything that reads a conversation (the history
//! a model gets, summaries of long chats, memory learning, search by
//! meaning, the phone's view, an older Yap) reads the branch shown and only
//! it. The others are in `branches` ([`Fork`]s along the branch shown, each
//! with its other [`Branch`]es, theirs nested in them), which an older Yap's
//! save drops. So they are also copied to `chat_branches.json` after every
//! save ([`mirror_to_disk`]), a file an older Yap never touches, and put back
//! at the next start ([`restore_from_disk`]) on every fork the messages still
//! fit (the shown branch's first message still there, at its time). Lost for
//! good only when an older Yap's own changes cut under a fork (a Code chat
//! rewound past it): the branch shown is then all there is, as before.
//!
//! Pure here, but for the copy on disk: the store (`chats.rs`) saves.

use serde::{Deserialize, Serialize};

use super::{ChatMessage, Conversation};

/// The most versions one message keeps (its fork's branches).
pub const MAX_BRANCHES: usize = 50;

/// **Where a conversation's branches part**: `messages[..at]` are all of
/// theirs alike; from `at` on, the branch shown is the conversation's own
/// `messages`, and the others are kept here.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Fork {
    pub at: usize,
    /// The number of the branch shown (1, 2…: the order they were made in).
    pub shown: u32,
    /// The time of the shown branch's first message (`messages[at].ts`; 0
    /// while it has none yet): how the copy put back after an older Yap's
    /// save knows the messages still fit.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub ts: u64,
    #[serde(default)]
    pub others: Vec<Branch>,
}

/// One branch not shown: its messages from its fork on, its own forks
/// further on, and the summary of a long chat written while it was shown
/// when that covers more than the part they share (`compact.rs`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Branch {
    pub n: u32,
    #[serde(default)]
    pub messages: Vec<ChatMessage>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub forks: Vec<Fork>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<super::ChatSummary>,
}

/// **A fork as the page shows it** (`chat_get`): the message at `at` says
/// ‹ shown / count ›. `since`: the time of the newest other version made
/// before the one shown (its first message's, unix seconds; 0: none) —
/// what that answer's turn made after it (a visual, `chat/made.rs`) is the
/// shown one's, what came before it another version's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ForkView {
    pub at: usize,
    pub shown: u32,
    pub count: u32,
    #[serde(default)]
    pub since: u64,
}

fn is_zero(n: &u64) -> bool {
    *n == 0
}

impl Fork {
    /// Every branch's number, the shown one's too, in order.
    fn numbers(&self) -> Vec<u32> {
        let mut all: Vec<u32> = self.others.iter().map(|b| b.n).chain([self.shown]).collect();
        all.sort_unstable();
        all
    }
}

/// Does the conversation's summary cover more than `messages[..at]` (or was
/// it written when there were more)? It goes with the branch then, as a
/// Rewind drops it (`chats::rewind`).
fn summary_past(conv: &Conversation, at: usize) -> bool {
    conv.summary.as_ref().is_some_and(|s| s.upto > at || s.after > at)
}

/// The branch shown from `at` on, taken out of `conv` as branch `n`: its
/// messages, its forks further on, its summary.
fn take_tail(conv: &mut Conversation, at: usize, n: u32) -> Branch {
    let messages = conv.messages.split_off(at.min(conv.messages.len()));
    let (forks, keep): (Vec<Fork>, Vec<Fork>) = std::mem::take(&mut conv.branches).into_iter().partition(|f| f.at > at);
    conv.branches = keep;
    let summary = if summary_past(conv, at) { conv.summary.take() } else { None };
    Branch { n, messages, forks, summary }
}

/// Branch `b` shown from where it parts on.
fn put_tail(conv: &mut Conversation, b: Branch) {
    conv.messages.extend(b.messages);
    conv.branches.extend(b.forks);
    conv.branches.sort_by_key(|f| f.at);
    if b.summary.is_some() {
        conv.summary = b.summary;
    }
}

/// The fork at `at` knows the shown branch's first message's time.
fn stamp_fork(conv: &mut Conversation, at: usize) {
    let ts = conv.messages.get(at).map(|m| m.ts).unwrap_or(0);
    if let Some(f) = conv.branches.iter_mut().find(|f| f.at == at) {
        f.ts = ts;
    }
}

/// **A new branch at `at`**: what's shown from there on set aside, kept as
/// one of the fork's branches, and nothing shown after `messages[..at]` —
/// the next message (the edited one, or the answer again) starts the new
/// branch, shown. The number of the branch set aside: [`undo`] goes back to
/// it. Pure.
pub fn branch_off(conv: &mut Conversation, at: usize) -> Result<u32, String> {
    if at >= conv.messages.len() {
        return Err("That message isn't there any more.".into());
    }
    if !conv.branches.iter().any(|f| f.at == at) {
        conv.branches.push(Fork { at, shown: 1, ts: 0, others: Vec::new() });
        conv.branches.sort_by_key(|f| f.at);
    }
    let fork = conv.branches.iter().find(|f| f.at == at).expect("made above");
    if fork.others.len() + 1 >= MAX_BRANCHES {
        return Err(format!("That message has {MAX_BRANCHES} versions already: that's as many as Yap keeps."));
    }
    let set_aside = fork.shown;
    let tail = take_tail(conv, at, set_aside);
    let fork = conv.branches.iter_mut().find(|f| f.at == at).expect("only later forks were taken");
    let next = fork.numbers().last().copied().unwrap_or(1) + 1;
    fork.others.push(tail);
    fork.shown = next;
    fork.ts = 0;
    Ok(set_aside)
}

/// **Back to branch `to`** of the fork at `at`, the branch shown dropped (a
/// retry or an edit that never answered, or Yap closed mid-way: [`heal`]).
/// A fork left with no other branch goes. The messages dropped, for their
/// screenshots. Pure.
pub fn undo(conv: &mut Conversation, at: usize, to: u32) -> Vec<ChatMessage> {
    let Some(fork) = conv.branches.iter().find(|f| f.at == at) else { return Vec::new() };
    if !fork.others.iter().any(|b| b.n == to) {
        return Vec::new();
    }
    let dropped = take_tail(conv, at, 0);
    let fork = conv.branches.iter_mut().find(|f| f.at == at).expect("only later forks were taken");
    let k = fork.others.iter().position(|b| b.n == to).expect("checked above");
    let back = fork.others.remove(k);
    fork.shown = to;
    let alone = fork.others.is_empty();
    put_tail(conv, back);
    if alone {
        conv.branches.retain(|f| f.at != at);
    } else {
        stamp_fork(conv, at);
    }
    owned_messages(dropped)
}

/// **Show the branch in place `place`** (1…count, as ‹ › counts) of the
/// fork at `at`: the one shown now set aside, that one in its place. False
/// when it's shown already. Pure.
pub fn switch(conv: &mut Conversation, at: usize, place: u32) -> Result<bool, String> {
    let fork = conv.branches.iter().find(|f| f.at == at).ok_or("That message has no other versions any more.")?;
    let numbers = fork.numbers();
    let to = *numbers.get((place as usize).wrapping_sub(1)).ok_or("That version isn't there any more.")?;
    if to == fork.shown {
        return Ok(false);
    }
    let shown = fork.shown;
    let tail = take_tail(conv, at, shown);
    let fork = conv.branches.iter_mut().find(|f| f.at == at).expect("only later forks were taken");
    let k = fork.others.iter().position(|b| b.n == to).expect("a number of its own");
    let chosen = fork.others.remove(k);
    fork.others.push(tail);
    fork.others.sort_by_key(|b| b.n);
    fork.shown = to;
    put_tail(conv, chosen);
    stamp_fork(conv, at);
    Ok(true)
}

/// **The fork at `at` once its new branch has its first message**: it knows
/// its time (the copy's check). Pure.
pub fn settle(conv: &mut Conversation, at: usize) {
    stamp_fork(conv, at);
}

/// **Cut at `at`** (a Code chat's Rewind, `chats::rewind`): the forks from
/// `at` on go, every branch of theirs with them — they hang off what's cut.
/// Their messages, for their screenshots. Pure.
pub fn cut_at(conv: &mut Conversation, at: usize) -> Vec<ChatMessage> {
    let (gone, keep): (Vec<Fork>, Vec<Fork>) = std::mem::take(&mut conv.branches).into_iter().partition(|f| f.at >= at);
    conv.branches = keep;
    gone.into_iter().flat_map(|f| f.others).flat_map(owned_messages).collect()
}

/// A branch's messages and those of every branch in it, owned.
fn owned_messages(b: Branch) -> Vec<ChatMessage> {
    let mut out = b.messages;
    for f in b.forks {
        for other in f.others {
            out.extend(owned_messages(other));
        }
    }
    out
}

/// **Every message of the branches not shown** (`forks`, and theirs):
/// their screenshots are kept for them, and go with them. Pure.
pub fn other_messages(forks: &[Fork]) -> Vec<&ChatMessage> {
    let mut out = Vec::new();
    collect(forks, &mut out);
    out
}

fn collect<'a>(forks: &'a [Fork], out: &mut Vec<&'a ChatMessage>) {
    for f in forks {
        for b in &f.others {
            out.extend(b.messages.iter());
            collect(&b.forks, out);
        }
    }
}

/// [`other_messages`], to change: a copy put back after an older Yap's
/// save (an answer's 👍 / 👎, `chat/feedback.rs`). Pure.
pub fn other_messages_mut(forks: &mut [Fork]) -> Vec<&mut ChatMessage> {
    let mut out = Vec::new();
    collect_mut(forks, &mut out);
    out
}

fn collect_mut<'a>(forks: &'a mut [Fork], out: &mut Vec<&'a mut ChatMessage>) {
    for f in forks {
        for b in f.others.iter_mut() {
            out.extend(b.messages.iter_mut());
            collect_mut(&mut b.forks, out);
        }
    }
}

/// **A conversation's forks put right**: none past its messages; one whose
/// shown branch is empty (Yap closed while a retry was being answered)
/// shows its newest other branch again; one with no other branch goes.
/// Whether anything changed. Pure.
pub fn heal(conv: &mut Conversation) -> bool {
    let mut changed = false;
    loop {
        let len = conv.messages.len();
        let before = conv.branches.len();
        conv.branches.retain(|f| f.at <= len && !f.others.is_empty());
        changed |= conv.branches.len() != before;
        let Some((at, to)) = conv.branches.iter().find(|f| f.at == len).map(|f| (f.at, f.others.iter().map(|b| b.n).max().unwrap_or(1))) else { break };
        undo(conv, at, to);
        changed = true;
    }
    changed
}

/// **The page's ‹ k/n ›**: each fork along the branch shown, the shown
/// branch's place among its fork's. Pure.
pub fn views(conv: &Conversation) -> Vec<ForkView> {
    conv.branches
        .iter()
        .filter(|f| f.at < conv.messages.len() && !f.others.is_empty())
        .map(|f| {
            let numbers = f.numbers();
            let place = numbers.iter().position(|n| *n == f.shown).unwrap_or(0) + 1;
            let shown_ts = conv.messages[f.at].ts;
            let since = f.others.iter().filter_map(|b| b.messages.first().map(|m| m.ts)).filter(|ts| *ts < shown_ts).max().unwrap_or(0);
            ForkView { at: f.at, shown: place as u32, count: numbers.len() as u32, since }
        })
        .collect()
}

// ---- kept for an older Yap's save ---------------------------------------------------------------

/// **`chat_branches.json`**: every conversation's forks, by its id — the
/// copy an older Yap never touches.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Mirror {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    conversations: Vec<Kept>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Kept {
    id: u64,
    forks: Vec<Fork>,
}

/// The forks `convs` keep, as the copy has them. Pure.
fn mirror_of(convs: &[Conversation]) -> Mirror {
    let conversations = convs.iter().filter(|c| !c.branches.is_empty()).map(|c| Kept { id: c.id, forks: c.branches.clone() }).collect();
    Mirror { version: 1, conversations }
}

/// Does fork `f` still fit `conv`'s messages: its shown branch's first
/// message there at its time (or, mid-retry, none yet at the end)? Pure.
fn fits(conv: &Conversation, f: &Fork) -> bool {
    match conv.messages.get(f.at) {
        Some(m) => f.ts == 0 || m.ts == f.ts,
        None => f.at == conv.messages.len() && f.ts == 0,
    }
}

/// **Put back** the forks an older Yap's save dropped: a conversation with
/// none now, those of the copy's that still fit, put right ([`heal`]). How
/// many conversations. Pure.
fn restore_from(convs: &mut [Conversation], mirror: &Mirror) -> usize {
    let mut put = 0;
    for k in &mirror.conversations {
        let Some(c) = convs.iter_mut().find(|c| c.id == k.id && c.branches.is_empty()) else { continue };
        let fitting: Vec<Fork> = k.forks.iter().filter(|f| fits(c, f)).cloned().collect();
        if fitting.is_empty() {
            continue;
        }
        c.branches = fitting;
        c.branches.sort_by_key(|f| f.at);
        heal(c);
        if !c.branches.is_empty() {
            put += 1;
        }
    }
    put
}

#[cfg(not(test))]
fn mirror_path() -> std::path::PathBuf {
    crate::config::data_dir().join("chat_branches.json")
}

/// What was written last (a save that changes no branch writes nothing).
#[cfg(not(test))]
static LAST: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

/// **The copy brought up to date** with chats.json's save (`chats.rs`
/// `save_to_disk`): written when the branches changed, removed when there
/// are none.
#[cfg(not(test))]
pub fn mirror_to_disk(convs: &[Conversation]) {
    let mirror = mirror_of(convs);
    let json = if mirror.conversations.is_empty() { String::new() } else { serde_json::to_string(&mirror).unwrap_or_default() };
    let mut last = LAST.lock().unwrap_or_else(|p| p.into_inner());
    if last.as_deref() == Some(json.as_str()) {
        return;
    }
    let path = mirror_path();
    let done = if json.is_empty() {
        match std::fs::remove_file(&path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
    } else {
        crate::config::atomic_write(&path, &json)
    };
    match done {
        Ok(()) => *last = Some(json),
        Err(e) => tracing::warn!(kind = ?e.kind(), "Chat: the branches' copy couldn't be saved; tried again with the next save"),
    }
}

/// **At load** (`chats.rs` `load_from_disk`): the branches an older Yap's
/// save dropped, put back from the copy.
#[cfg(not(test))]
pub fn restore_from_disk(convs: &mut [Conversation]) {
    let Ok(raw) = std::fs::read_to_string(mirror_path()) else { return };
    let Ok(mirror) = serde_json::from_str::<Mirror>(&raw) else {
        tracing::warn!("Chat: the branches' copy couldn't be read; left as it is");
        return;
    };
    let put = restore_from(convs, &mirror);
    if put > 0 {
        tracing::info!(conversations = put, "Chat: branches an older Yap's save dropped, put back");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(role: &str, text: &str, ts: u64) -> ChatMessage {
        ChatMessage { role: role.into(), text: text.into(), ts, ..Default::default() }
    }

    /// A conversation of `pairs` questions, each answered.
    fn conv(pairs: &[(&str, &str)]) -> Conversation {
        let mut messages = Vec::new();
        for (i, (q, a)) in pairs.iter().enumerate() {
            messages.push(msg("user", q, 100 + 2 * i as u64));
            messages.push(msg("assistant", a, 101 + 2 * i as u64));
        }
        serde_json::from_value(serde_json::json!({ "id": 7, "title": "t", "createdTs": 1, "updatedTs": 1 })).map(|mut c: Conversation| {
            c.messages = messages;
            c
        }).unwrap()
    }

    /// Each fork's place, as ‹ › says it: (at, shown, count).
    fn places(c: &Conversation) -> Vec<(usize, u32, u32)> {
        views(c).iter().map(|v| (v.at, v.shown, v.count)).collect()
    }

    fn texts(c: &Conversation) -> Vec<&str> {
        c.messages.iter().map(|m| m.text.as_str()).collect()
    }

    #[test]
    fn a_retry_keeps_both_answers_and_switches_between_them() {
        let mut c = conv(&[("Hi", "Hello"), ("Weather?", "Sunny"), ("And tomorrow?", "Rain")]);
        // Retry the second answer: it and everything after are set aside.
        let back = branch_off(&mut c, 3).unwrap();
        assert_eq!(back, 1);
        assert_eq!(texts(&c), ["Hi", "Hello", "Weather?"]);
        c.messages.push(msg("assistant", "Cloudy", 200));
        settle(&mut c, 3);
        assert_eq!(places(&c), [(3, 2, 2)]);
        // What the retried answer's turn made after the first answer is its own (`since`).
        assert_eq!(views(&c)[0].since, 103);
        // ‹ back to the first: the follow-up comes back with it.
        assert!(switch(&mut c, 3, 1).unwrap());
        assert_eq!(texts(&c), ["Hi", "Hello", "Weather?", "Sunny", "And tomorrow?", "Rain"]);
        assert_eq!(places(&c), [(3, 1, 2)]);
        assert_eq!(views(&c)[0].since, 0, "no version came before the first");
        assert!(!switch(&mut c, 3, 1).unwrap(), "shown already");
        assert!(switch(&mut c, 3, 2).unwrap());
        assert_eq!(texts(&c), ["Hi", "Hello", "Weather?", "Cloudy"]);
        assert!(switch(&mut c, 3, 3).is_err(), "no third");
    }

    #[test]
    fn an_edit_branches_at_the_persons_message_and_nested_branches_travel_with_theirs() {
        let mut c = conv(&[("Hi", "Hello"), ("Weather?", "Sunny"), ("And tomorrow?", "Rain")]);
        // Retry the last answer first: a fork at 5.
        branch_off(&mut c, 5).unwrap();
        c.messages.push(msg("assistant", "Snow", 300));
        settle(&mut c, 5);
        // Then edit the second question: the fork at 5 goes with what's set aside.
        let back = branch_off(&mut c, 2).unwrap();
        assert_eq!((back, texts(&c)), (1, vec!["Hi", "Hello"]));
        assert!(c.branches.iter().all(|f| f.at == 2), "the later fork is the old branch's");
        c.messages.push(msg("user", "Weather by the sea?", 400));
        c.messages.push(msg("assistant", "Drizzle", 401));
        settle(&mut c, 2);
        assert_eq!(places(&c), [(2, 2, 2)]);
        // Back to the original question: its own retried answer is shown, ‹ 2/2 › on it.
        switch(&mut c, 2, 1).unwrap();
        assert_eq!(texts(&c), ["Hi", "Hello", "Weather?", "Sunny", "And tomorrow?", "Snow"]);
        assert_eq!(places(&c), [(2, 1, 2), (5, 2, 2)]);
        let every: Vec<&str> = other_messages(&c.branches).iter().map(|m| m.text.as_str()).collect();
        assert_eq!(every, ["Weather by the sea?", "Drizzle", "Rain"]);
    }

    #[test]
    fn a_retry_that_never_answered_goes_back_to_the_answer_it_replaced() {
        let mut c = conv(&[("Hi", "Hello"), ("Weather?", "Sunny")]);
        let back = branch_off(&mut c, 3).unwrap();
        let gone = undo(&mut c, 3, back);
        assert!(gone.is_empty());
        assert_eq!(texts(&c), ["Hi", "Hello", "Weather?", "Sunny"]);
        assert!(c.branches.is_empty(), "no other version left: no fork");
        // A second retry answered, a third not: back to the second, ‹ 2/2 ›.
        branch_off(&mut c, 3).unwrap();
        c.messages.push(msg("assistant", "Cloudy", 200));
        let back = branch_off(&mut c, 3).unwrap();
        assert_eq!(back, 2);
        undo(&mut c, 3, back);
        assert_eq!(texts(&c).last(), Some(&"Cloudy"));
        assert_eq!(places(&c), [(3, 2, 2)]);
        // An edit whose new question was saved but never answered: dropped, for its screenshots.
        let back = branch_off(&mut c, 2).unwrap();
        c.messages.push(msg("user", "Weather by the sea?", 400));
        let gone = undo(&mut c, 2, back);
        assert_eq!(gone.iter().map(|m| m.text.as_str()).collect::<Vec<_>>(), ["Weather by the sea?"]);
        assert_eq!(texts(&c), ["Hi", "Hello", "Weather?", "Cloudy"]);
    }

    #[test]
    fn a_summary_goes_with_the_branch_it_covers() {
        let mut c = conv(&[("a", "b"), ("c", "d"), ("e", "f")]);
        c.summary = Some(crate::chat::chats::ChatSummary { text: "- old".into(), upto: 4, after: 6, ..Default::default() });
        branch_off(&mut c, 1).unwrap();
        assert!(c.summary.is_none(), "it covered the branch set aside");
        c.messages.push(msg("assistant", "B", 50));
        switch(&mut c, 1, 1).unwrap();
        assert_eq!(c.summary.as_ref().map(|s| s.text.as_str()), Some("- old"), "back with its branch");
        // One that covers only what both share stays.
        c.summary = Some(crate::chat::chats::ChatSummary { text: "- shared".into(), upto: 1, after: 1, ..Default::default() });
        switch(&mut c, 1, 2).unwrap();
        assert_eq!(c.summary.as_ref().map(|s| s.text.as_str()), Some("- shared"));
    }

    #[test]
    fn a_rewind_cuts_the_forks_after_it() {
        let mut c = conv(&[("a", "b"), ("c", "d"), ("e", "f")]);
        branch_off(&mut c, 1).unwrap();
        c.messages.push(msg("assistant", "B", 50));
        c.messages.push(msg("user", "c", 51));
        c.messages.push(msg("assistant", "D", 52));
        // Edit "c": a fork at 2. Then a Rewind to before the edited one.
        branch_off(&mut c, 2).unwrap();
        c.messages.push(msg("user", "C2", 60));
        c.messages.truncate(2);
        let gone = cut_at(&mut c, 2);
        assert_eq!(gone.iter().map(|m| m.text.as_str()).collect::<Vec<_>>(), ["c", "D"]);
        assert_eq!(places(&c), [(1, 2, 2)]);
    }

    #[test]
    fn closed_mid_retry_the_answer_comes_back_and_forks_past_the_end_go() {
        let mut c = conv(&[("a", "b"), ("c", "d")]);
        branch_off(&mut c, 3).unwrap();
        assert!(heal(&mut c));
        assert_eq!(texts(&c), ["a", "b", "c", "d"]);
        assert!(c.branches.is_empty());
        // A fork an older Yap's rewind cut under.
        c.branches.push(Fork { at: 9, shown: 2, ts: 5, others: vec![Branch { n: 1, ..Default::default() }] });
        assert!(heal(&mut c));
        assert!(c.branches.is_empty());
    }

    #[test]
    fn an_older_yaps_save_loses_no_branch_that_still_fits() {
        let mut ours = conv(&[("a", "b"), ("c", "d"), ("e", "f")]);
        branch_off(&mut ours, 1).unwrap();
        ours.messages.push(msg("assistant", "B", 50));
        ours.messages.push(msg("user", "c", 51));
        ours.messages.push(msg("assistant", "D", 52));
        settle(&mut ours, 1);
        // Retry "D": a fork at 3.
        branch_off(&mut ours, 3).unwrap();
        ours.messages.push(msg("assistant", "f2", 61));
        settle(&mut ours, 3);
        let mirror: Mirror = serde_json::from_str(&serde_json::to_string(&mirror_of(std::slice::from_ref(&ours))).unwrap()).unwrap();
        // An older Yap reads chats.json, keeps the branch shown, drops the rest, and adds a message.
        let mut theirs: Conversation = serde_json::from_value({
            let mut v = serde_json::to_value(&ours).unwrap();
            v.as_object_mut().unwrap().remove("branches");
            v
        })
        .unwrap();
        assert!(theirs.branches.is_empty());
        theirs.messages.push(msg("user", "more", 70));
        let mut convs = vec![theirs];
        assert_eq!(restore_from(&mut convs, &mirror), 1);
        assert_eq!(places(&convs[0]), places(&ours));
        // One its own rewind cut under (the second fork's message rewritten): only the first comes back.
        let mut cut: Conversation = convs[0].clone();
        cut.branches.clear();
        cut.messages.truncate(3);
        cut.messages.push(msg("assistant", "other", 80));
        let mut convs = vec![cut];
        assert_eq!(restore_from(&mut convs, &mirror), 1);
        assert_eq!(places(&convs[0]), [(1, 2, 2)]);
        // A conversation that has its own branches keeps them (chats.json wins).
        let mut convs = vec![ours.clone()];
        assert_eq!(restore_from(&mut convs, &mirror), 0);
    }

    #[test]
    fn a_fork_keeps_at_most_its_number_of_versions() {
        let mut c = conv(&[("a", "b")]);
        for i in 0..MAX_BRANCHES - 1 {
            branch_off(&mut c, 1).unwrap();
            c.messages.push(msg("assistant", "again", 200 + i as u64));
        }
        assert!(branch_off(&mut c, 1).is_err());
        assert_eq!(places(&c), [(1, MAX_BRANCHES as u32, MAX_BRANCHES as u32)]);
    }
}
