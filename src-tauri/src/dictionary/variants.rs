//! **One word, several ways it's heard** (2026-10-25): "Kubernetes" ←
//! "cooper netties", "cube an eighties".
//!
//! - **On disk it's rows** — one `{from, to, fuzzy, snippet, starred}` per
//!   way a word is heard, the shape config.json has always had — so an
//!   older Yap sharing `%APPDATA%\yap\config.json` (the installed nightly
//!   beside Yap - Dev) reads every one and writes every one back: nothing
//!   to migrate, nothing it can drop. ([`rows`]: an entry that ever arrives
//!   with `alsoHeard` is written out as rows of its own, `config::save`.)
//! - **The Dictionary view groups them**: the rows that write the same word
//!   exactly (corrections) are one entry with several ways it's heard, its
//!   star and ≈ from its first row (`views/dictionary/dictionaryList.js`
//!   `groupRows`), and go back as rows, every row with the entry's star and
//!   ≈ (`entryRows`). Import and export work on the same rows.
//! - **The passes** take every row as ever (the exact pass, the near-miss
//!   pass); the Whisper prompt has the word once (`config::dictionary_prompt`
//!   keeps one of each written form); AI cleanup is told `"cooper netties"
//!   or "cube an eighties" → "Kubernetes"` (`llm::dictionary_suffix`
//!   groups them too).
//! - **New ways in** are new rows for the same word ([`add_heard`]): a word
//!   offered from your corrections that the dictionary writes already.
//! - `DictionaryEntry::also_heard` is for memory only: memory's names and
//!   your own carry how they're said (`spoken_names.rs`), never saved.

use crate::config::DictionaryEntry;

/// `entries` as config.json keeps them: an entry with `also_heard` as a
/// row per way it's heard (its flags on each, in place); the rest as they
/// are. Pure.
pub fn rows(entries: &[DictionaryEntry]) -> Vec<DictionaryEntry> {
    let mut out = Vec::with_capacity(entries.len());
    for e in entries {
        if e.also_heard.is_empty() {
            out.push(e.clone());
            continue;
        }
        let row = |from: &str| DictionaryEntry { from: from.to_string(), also_heard: Vec::new(), ..e.clone() };
        if e.snippet {
            out.push(row(&e.from));
            continue;
        }
        let heard = e.heard();
        if heard.is_empty() {
            out.push(row(&e.from));
        }
        for h in heard {
            out.push(row(h));
        }
    }
    out
}

/// A correction `heard` → `term` into `dict` (a word offered from your
/// corrections): a new row — beside the rows already writing `term`, which
/// the Dictionary view shows as one entry — with their star, near-misses
/// on (as every new correction). Nothing when a row writing `term` is heard
/// that way already. Returns whether the dictionary changed.
pub fn add_heard(dict: &mut Vec<DictionaryEntry>, heard: &str, term: &str) -> bool {
    let (heard, term) = (heard.trim(), term.trim());
    if heard.is_empty() || term.is_empty() {
        return false;
    }
    let same_word = |e: &&DictionaryEntry| !e.snippet && e.to.trim() == term;
    if dict.iter().filter(same_word).any(|e| e.heard().iter().any(|h| h.to_lowercase() == heard.to_lowercase())) {
        return false;
    }
    let starred = dict.iter().find(same_word).is_some_and(|e| e.starred);
    dict.push(DictionaryEntry {
        from: heard.to_string(),
        to: term.to_string(),
        fuzzy: true,
        snippet: false,
        starred,
        also_heard: Vec::new(),
    });
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::YapConfig;

    fn c(from: &str, to: &str) -> DictionaryEntry {
        DictionaryEntry { from: from.into(), to: to.into(), fuzzy: true, snippet: false, starred: false, also_heard: vec![] }
    }

    /// A Yap from before 2026-10-25, as it reads config.json and writes it
    /// back: `{from, to, fuzzy, snippet}` rows, anything else ignored on
    /// read and gone on write.
    mod older {
        use serde::{Deserialize, Serialize};

        #[derive(Serialize, Deserialize)]
        #[serde(rename_all = "camelCase")]
        pub struct Entry {
            pub from: String,
            pub to: String,
            #[serde(default)]
            pub fuzzy: bool,
            #[serde(default)]
            pub snippet: bool,
        }

        #[derive(Serialize, Deserialize)]
        pub struct Config {
            #[serde(default)]
            pub dictionary: Vec<Entry>,
        }
    }

    #[test]
    fn an_older_yap_reads_and_saves_every_way_a_word_is_heard() {
        // The Dictionary view's entry, as it sends it (rows: dictionaryList.js
        // `entryRows`), and one sent grouped all the same (`alsoHeard`).
        let mut grouped = DictionaryEntry { starred: true, ..c("cooper netties", "Kubernetes") };
        grouped.also_heard = vec!["cube an eighties".into(), "kuber nets".into()];
        let cfg = YapConfig {
            dictionary: vec![grouped, c("jaison", "JSON"), c("jay son", "JSON"), DictionaryEntry { snippet: true, ..c("my sign off", "Speak soon,") }],
            ..YapConfig::default()
        };
        // Saved by this Yap: rows, no field an older one doesn't know.
        let saved = serde_json::to_value(YapConfig { dictionary: rows(&cfg.dictionary), ..cfg.clone() }).unwrap();
        assert!(saved["dictionary"].as_array().unwrap().iter().all(|e| e.get("alsoHeard").is_none()));
        // Read and saved again by an older Yap.
        let old: older::Config = serde_json::from_value(saved).unwrap();
        let resaved = serde_json::to_value(&old).unwrap();
        // …and read by this one: every way each word is heard is there.
        let back: YapConfig = serde_json::from_value(resaved).unwrap();
        let pairs: Vec<(&str, &str, bool)> = back.dictionary.iter().map(|e| (e.from.as_str(), e.to.as_str(), e.snippet)).collect();
        assert_eq!(
            pairs,
            [
                ("cooper netties", "Kubernetes", false),
                ("cube an eighties", "Kubernetes", false),
                ("kuber nets", "Kubernetes", false),
                ("jaison", "JSON", false),
                ("jay son", "JSON", false),
                ("my sign off", "Speak soon,", true),
            ]
        );
        // And every one is still corrected.
        assert_eq!(
            crate::config::apply_dictionary("cooper netties, cube an eighties and kuber nets", &back.dictionary),
            "Kubernetes, Kubernetes and Kubernetes"
        );
    }

    #[test]
    fn rows_write_an_entry_out_a_row_a_way_its_flags_on_each() {
        let mut kube = DictionaryEntry { starred: true, fuzzy: false, ..c("cooper netties", "Kubernetes") };
        kube.also_heard = vec!["Cooper Netties".into(), " cube an eighties ".into(), String::new()];
        let out = rows(&[c("jaison", "JSON"), kube]);
        let got: Vec<(&str, bool, bool)> = out.iter().map(|e| (e.from.as_str(), e.starred, e.fuzzy)).collect();
        assert_eq!(got, [("jaison", false, true), ("cooper netties", true, false), ("cube an eighties", true, false)]);
        assert!(out.iter().all(|e| e.also_heard.is_empty()));
        // Saved by config::save as rows too (the JSON has no `alsoHeard`).
        let json = serde_json::to_string(&out).unwrap();
        assert!(!json.contains("alsoHeard"));
        // A snippet is one phrase.
        let mut snip = DictionaryEntry { snippet: true, ..c("my sign off", "Speak soon,") };
        snip.also_heard = vec!["sign me off".into()];
        assert_eq!(rows(&[snip]).len(), 1);
    }

    #[test]
    fn a_new_way_it_is_heard_is_a_new_row_for_the_word() {
        let mut dict = vec![DictionaryEntry { starred: true, ..c("cooper netties", "Kubernetes") }];
        assert!(add_heard(&mut dict, "cube an eighties", "Kubernetes"));
        assert!(!add_heard(&mut dict, "Cube An Eighties", "Kubernetes"));
        assert!(!add_heard(&mut dict, "cooper netties", "Kubernetes"));
        assert_eq!(dict.len(), 2);
        // Beside the word's rows, with its star (one entry in the view).
        assert_eq!((dict[1].from.as_str(), dict[1].to.as_str(), dict[1].starred, dict[1].fuzzy), ("cube an eighties", "Kubernetes", true, true));
        assert!(!add_heard(&mut dict, " ", "Kubernetes"));
    }

    #[test]
    fn every_row_is_corrected_and_the_prompt_has_the_word_once() {
        let dict = vec![c("cooper netties", "Kubernetes"), c("cube an eighties", "Kubernetes"), c("cube", "Kubernetes")];
        // Exact: each one ("cube" after "cube an eighties" doesn't eat it: rows
        // run in order, and that row is first here).
        assert_eq!(crate::config::apply_dictionary("cooper netties and cube an eighties", &dict), "Kubernetes and Kubernetes");
        // Near-misses of each.
        let two = &dict[..2];
        assert_eq!(crate::dictionary::fuzzy::apply_fuzzy("deploy to cooper nettie now", two), "deploy to Kubernetes now");
        assert_eq!(crate::dictionary::fuzzy::apply_fuzzy("deploy to cube an eightys now", two), "deploy to Kubernetes now");
        assert_eq!(crate::dictionary::fuzzy::apply_fuzzy("a cube of ice", two), "a cube of ice");
        // The Whisper prompt: the written form once.
        assert_eq!(crate::config::dictionary_prompt(&dict).as_deref(), Some("Kubernetes"));
        // In memory, an entry may carry more ways (memory's names): the same.
        let mut name = c("", "Siobhan");
        name.also_heard = vec!["shivon".into()];
        assert_eq!(name.heard(), ["shivon"]);
        assert_eq!(crate::dictionary::fuzzy::apply_fuzzy("ask shivon", &[name]), "ask Siobhan");
    }
}
