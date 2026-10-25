//! **One entry, several phrases** (2026-10-25): a correction can be heard
//! more than one way — "Kubernetes" ← "cooper netties", "cube an eighties" —
//! and keeps them all in one entry (`DictionaryEntry::also_heard` beside
//! `from`; `DictionaryEntry::heard` lists them).
//!
//! - **The passes** take every way it's heard: the exact pass
//!   (`config::apply_dictionary`, the longest first), the near-miss pass
//!   (`fuzzy.rs`: each one, and the written form, a spelling to match);
//!   AI cleanup is told `"cooper netties" or "cube an eighties" →
//!   "Kubernetes"` (`llm::dictionary_suffix`). The Whisper prompt gets the
//!   written form only (`config::dictionary_prompt`).
//! - **The one-time change** ([`migrate`], from `config::load`, by the
//!   dictionary's own version `dictionary_version`, apart from the config's
//!   `config_version`): corrections written exactly the same way, with the
//!   same near-miss switch, become one entry where the first of them was —
//!   its star if any had one. Nothing is lost: every way each was heard is
//!   kept, in the order they were added. Snippets never merge.
//! - **New ways in** go to the entry already writing that word: a word
//!   offered from your corrections (`learn.rs` [`add_heard`]) and an
//!   import's rows (`views/dictionary/dictionaryFile.js`).
//!
//! An older Yap reading config.json ignores `alsoHeard` (it keeps `from`).

use crate::config::{DictionaryEntry, YapConfig};

/// The dictionary's current version (`YapConfig::dictionary_version`).
pub const VERSION: u32 = 1;

/// The one-time change for a config whose dictionary is older than
/// [`VERSION`]. Returns whether anything changed (the caller saves).
pub fn migrate(cfg: &mut YapConfig) -> bool {
    if cfg.dictionary_version >= VERSION {
        return false;
    }
    let before = cfg.dictionary.len();
    cfg.dictionary = merge(std::mem::take(&mut cfg.dictionary));
    cfg.dictionary_version = VERSION;
    let merged = before - cfg.dictionary.len();
    if merged > 0 {
        tracing::info!(merged, entries = cfg.dictionary.len(), "Dictionary: corrections written the same way are one entry now");
    }
    true
}

/// Corrections written the same way (`to`, exactly) with the same
/// near-miss switch, as one entry with every way they're heard (pure). The
/// entry stays where the first of them was; a star on any is kept.
/// Snippets and half-written rows are left as they are.
pub fn merge(entries: Vec<DictionaryEntry>) -> Vec<DictionaryEntry> {
    let mut out: Vec<DictionaryEntry> = Vec::with_capacity(entries.len());
    for e in entries {
        let mergeable = !e.snippet && !e.to.trim().is_empty() && !e.from.trim().is_empty();
        let into = mergeable
            .then(|| {
                out.iter_mut().find(|o| {
                    !o.snippet && !o.from.trim().is_empty() && o.fuzzy == e.fuzzy && o.to.trim() == e.to.trim()
                })
            })
            .flatten();
        match into {
            Some(o) => {
                o.starred |= e.starred;
                for h in e.heard() {
                    push_heard(o, h);
                }
            }
            None => out.push(e),
        }
    }
    out
}

/// Add `heard` to `entry`'s ways of being heard, unless it's one of them
/// already (any case). Returns whether it was added.
pub fn push_heard(entry: &mut DictionaryEntry, heard: &str) -> bool {
    let heard = heard.trim();
    if heard.is_empty() || entry.heard().iter().any(|h| h.to_lowercase() == heard.to_lowercase()) {
        return false;
    }
    if entry.from.trim().is_empty() {
        entry.from = heard.to_string();
    } else {
        entry.also_heard.push(heard.to_string());
    }
    true
}

/// A correction `heard` → `term` into `dict`: another way the entry that
/// writes `term` already (exactly, near-misses on) is heard, else a new
/// entry at the end (near-misses on, as every new correction). Returns
/// whether the dictionary changed.
pub fn add_heard(dict: &mut Vec<DictionaryEntry>, heard: &str, term: &str) -> bool {
    let (heard, term) = (heard.trim(), term.trim());
    if heard.is_empty() || term.is_empty() {
        return false;
    }
    if let Some(e) = dict.iter_mut().find(|e| !e.snippet && e.fuzzy && e.to.trim() == term) {
        return push_heard(e, heard);
    }
    dict.push(DictionaryEntry {
        from: heard.to_string(),
        to: term.to_string(),
        fuzzy: true,
        snippet: false,
        starred: false,
        also_heard: Vec::new(),
    });
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(from: &str, to: &str) -> DictionaryEntry {
        DictionaryEntry { from: from.into(), to: to.into(), fuzzy: true, snippet: false, starred: false, also_heard: vec![] }
    }

    #[test]
    fn corrections_written_the_same_way_become_one_entry() {
        let mut exact = c("jaison", "JSON");
        exact.fuzzy = false;
        let snippet = DictionaryEntry { snippet: true, ..c("my sign off", "Kubernetes") };
        let list = vec![
            c("cooper netties", "Kubernetes"),
            exact.clone(),
            c("graph ana", "Grafana"),
            DictionaryEntry { starred: true, ..c("cube an eighties", "Kubernetes") },
            c("Cooper Netties", "Kubernetes"), // the same way, another case
            c("jason", "Json"),                // another spelling: its own
            DictionaryEntry { fuzzy: false, ..c("jay son", "JSON") },
            c("j son", "JSON"), // near-misses on: not with the exact-only ones
            snippet.clone(),
            c("", "Kubernetes"), // a row being written
        ];
        let out = merge(list);
        let kube = &out[0];
        assert_eq!((kube.from.as_str(), kube.to.as_str()), ("cooper netties", "Kubernetes"));
        assert_eq!(kube.also_heard, ["cube an eighties"]);
        assert!(kube.starred);
        assert_eq!(kube.heard(), ["cooper netties", "cube an eighties"]);
        assert_eq!(out[1].heard(), ["jaison", "jay son"]);
        assert!(!out[1].fuzzy);
        let rest: Vec<(&str, &str)> = out[2..].iter().map(|e| (e.from.as_str(), e.to.as_str())).collect();
        assert_eq!(rest, [("graph ana", "Grafana"), ("jason", "Json"), ("j son", "JSON"), ("my sign off", "Kubernetes"), ("", "Kubernetes")]);
        assert!(out[5].snippet);
        // Again: nothing more to do.
        let again = merge(out.clone());
        assert_eq!(serde_json::to_value(&again).unwrap(), serde_json::to_value(&out).unwrap());
    }

    #[test]
    fn the_migration_runs_once_and_saves_as_before_without_variants() {
        let mut cfg = YapConfig { dictionary: vec![c("a b", "Ab"), c("ay bee", "Ab")], dictionary_version: 0, ..YapConfig::default() };
        assert!(migrate(&mut cfg));
        assert_eq!(cfg.dictionary.len(), 1);
        assert_eq!(cfg.dictionary_version, VERSION);
        assert!(!migrate(&mut cfg));
        // A person's two rows made since stay two (the change ran once).
        cfg.dictionary.push(c("eh bee", "Ab"));
        assert!(!migrate(&mut cfg));
        assert_eq!(cfg.dictionary.len(), 2);
        // A config made now needs nothing.
        assert!(!migrate(&mut YapConfig::default()));
        // On disk: `alsoHeard` only when there are some.
        let json = serde_json::to_value(&cfg.dictionary).unwrap();
        assert_eq!(json[0]["alsoHeard"], serde_json::json!(["ay bee"]));
        assert!(json[1].get("alsoHeard").is_none());
        let back: Vec<DictionaryEntry> = serde_json::from_value(json).unwrap();
        assert_eq!(back[0].heard(), ["a b", "ay bee"]);
        // An older file (no field) reads as before.
        let old: DictionaryEntry = serde_json::from_str(r#"{"from":"x","to":"Y"}"#).unwrap();
        assert!(old.also_heard.is_empty() && old.heard() == ["x"]);
    }

    #[test]
    fn a_new_way_it_is_heard_joins_the_entry_writing_that_word() {
        let mut dict = vec![c("cooper netties", "Kubernetes"), DictionaryEntry { fuzzy: false, ..c("jaison", "JSON") }];
        assert!(add_heard(&mut dict, "cube an eighties", "Kubernetes"));
        assert!(!add_heard(&mut dict, "Cube An Eighties", "Kubernetes"));
        assert_eq!(dict.len(), 2);
        assert_eq!(dict[0].heard(), ["cooper netties", "cube an eighties"]);
        // Only an entry with near-misses on takes it; else a new one.
        assert!(add_heard(&mut dict, "jay son", "JSON"));
        assert_eq!(dict.len(), 3);
        assert_eq!((dict[2].from.as_str(), dict[2].fuzzy), ("jay son", true));
        // Another spelling of the word is another entry.
        assert!(add_heard(&mut dict, "kubernetis", "kubernetes"));
        assert_eq!(dict.len(), 4);
    }

    #[test]
    fn every_way_it_is_heard_is_corrected_and_the_prompt_has_the_word_once() {
        let mut kube = c("cooper netties", "Kubernetes");
        kube.also_heard = vec!["cube an eighties".into(), "cube".into()];
        let dict = vec![kube];
        // Exact: each one, the longest first ("cube" doesn't eat "cube an eighties").
        assert_eq!(
            crate::config::apply_dictionary("cooper netties and cube an eighties, or cube", &dict),
            "Kubernetes and Kubernetes, or Kubernetes"
        );
        // Near-misses of each.
        let mut kube = c("cooper netties", "Kubernetes");
        kube.also_heard = vec!["cube an eighties".into()];
        let two = vec![kube];
        assert_eq!(crate::dictionary::fuzzy::apply_fuzzy("deploy to cooper nettie now", &two), "deploy to Kubernetes now");
        assert_eq!(crate::dictionary::fuzzy::apply_fuzzy("deploy to cube an eightys now", &two), "deploy to Kubernetes now");
        assert_eq!(crate::dictionary::fuzzy::apply_fuzzy("a cube of ice", &two), "a cube of ice");
        // The Whisper prompt: the written form only.
        assert_eq!(crate::config::dictionary_prompt(&dict).as_deref(), Some("Kubernetes"));
        // A snippet's phrase is one phrase: its variants (none in the UI) aren't used.
        let mut snip = DictionaryEntry { snippet: true, ..c("my sign off", "Speak soon,") };
        snip.also_heard = vec!["sign me off".into()];
        assert_eq!(snip.heard(), ["my sign off"]);
    }
}
