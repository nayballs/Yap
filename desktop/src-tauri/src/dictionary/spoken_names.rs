//! **Names a speech model spells far from how they're written** (2026-10-25):
//! "Shivon" for "Siobhan", "Neev" for "Niamh", "Seersha" for "Saoirse". A
//! speech model writes what it hears the way English spells it, and these
//! names (Irish, Scottish and Welsh ones most of all) aren't spelled the
//! way they sound: no letter-based distance finds them — by its letters and
//! their Soundex/sound keys, "Spain" is nearer "Siobhan" than "Shivon" is.
//!
//! So each such name has a few **respellings of how it's said** ([`said_as`],
//! a built-in list), and memory's names and your own (About you) carry them
//! as more ways they're heard (`DictionaryEntry::also_heard`,
//! `memory/vocab.rs`). The names pass (`fuzzy::apply_names`) then matches a
//! heard word against those by its usual score and its usual guards: never
//! where what was heard is an ordinary word, the respellings never in the
//! Whisper prompt (it gets the written name). Left out on purpose: a
//! respelling that's a common name of its own ("Owen" for "Eoin", "Rory"
//! for "Ruairí", "Killian" for "Cillian", "Reece" for "Rhys") — it would
//! turn another person's name into this one. Measured on everyday
//! sentences with every name: `memory/vocab.rs` `no_everyday_sentence_changes`.

/// A name (lower case, accents off) → how it's said, spelled as an English
/// speech model would write it.
const SAID_AS: &[(&str, &[&str])] = &[
    // Irish
    ("siobhan", &["shivawn", "shivaun", "shevaun", "shivon", "shevonne"]),
    ("niamh", &["neev", "neeve"]),
    ("saoirse", &["seersha", "sursha", "seersa", "searsha"]),
    ("aoife", &["eefa", "eefah"]),
    ("caoimhe", &["keeva", "kweeva", "keevah"]),
    ("sinead", &["shinade", "shinaid", "shinayd"]),
    ("grainne", &["grawnya", "granya", "grawnia"]),
    ("tadhg", &["taig", "tige", "teige"]),
    ("ciara", &["keera", "keerah"]),
    ("oisin", &["usheen", "osheen", "oshean"]),
    ("padraig", &["pawdrig", "pordrig", "paudrig"]),
    ("dearbhla", &["dervla", "dervala"]),
    ("ailbhe", &["alva", "alvah"]),
    ("clodagh", &["cloda", "clodah"]),
    ("roisin", &["rosheen", "roesheen"]),
    ("aisling", &["ashling"]),
    ("caoilfhionn", &["keelin", "keeleen"]),
    ("sadhbh", &["sive", "syve"]),
    ("fionnuala", &["finoola", "finnoola"]),
    ("muireann", &["mirran", "mirrin"]),
    ("sorcha", &["sorka", "surka"]),
    ("treasa", &["trassa"]),
    ("eimear", &["eemer", "emer"]),
    ("blathnaid", &["blawnid", "blaunid"]),
    ("cathal", &["cahal", "kahal"]),
    ("donnchadh", &["donaka", "dunaka"]),
    ("diarmuid", &["dermot", "deermid"]),
    ("feidhlim", &["faylim", "felim"]),
    ("mairead", &["moraid", "maraid"]),
    ("orlaith", &["orla", "orlah"]),
    ("una", &["oona"]),
    // Scottish Gaelic
    ("eilidh", &["ailey", "aylee", "ailie"]),
    ("mairi", &["mahri", "maree"]),
    ("ealasaid", &["elasatch", "yalasatch"]),
    // Welsh (not Siân: "Shan" is a letter from "Shane", "Sharn" from
    // "Sharon" — other people's names)
    ("dafydd", &["davith", "davvith"]),
    ("ffion", &["fee on", "feeon"]),
    ("cerys", &["kerris", "kerys"]),
    ("angharad", &["anharad", "an harad"]),
    ("llinos", &["hlinos", "thlinos"]),
    ("gwenllian", &["gwenthlian", "gwenhlian"]),
    ("siwan", &["shoo an", "shuwan"]),
    // Others a speech model writes the English way
    ("joaquin", &["wakeen", "walkeen", "hwakeen"]),
    ("xavier", &["zavier", "zayvier"]),
    ("wojciech", &["voytek", "voyteck", "voy check"]),
    ("grzegorz", &["gzhegosh", "zhegosh"]),
    ("nguyen", &["nwin", "ngwen"]),
    ("thanh", &["tahn", "tan"]),
    ("xiaoming", &["shaoming", "shao ming"]),
    ("zhou", &["joe"]),
];

/// Letters, lower case, accents off ("Siobhán" → "siobhan").
fn fold(name: &str) -> String {
    name.chars()
        .flat_map(char::to_lowercase)
        .filter_map(|c| {
            let plain = match c {
                'á' | 'à' | 'â' | 'ä' | 'ã' | 'å' => 'a',
                'é' | 'è' | 'ê' | 'ë' => 'e',
                'í' | 'ì' | 'î' | 'ï' => 'i',
                'ó' | 'ò' | 'ô' | 'ö' | 'õ' => 'o',
                'ú' | 'ù' | 'û' | 'ü' => 'u',
                'ŵ' => 'w',
                'ŷ' => 'y',
                c => c,
            };
            plain.is_alphanumeric().then_some(plain)
        })
        .collect()
}

/// Respellings that are a name (or an everyday word) of their own: only
/// for the person's own name ("Dermot" said by a Diarmuid is him), never
/// for someone memory remembers (another person's "Dermot" isn't them).
const OWN_NAME_ONLY: &[&str] = &["joe", "tan", "emer", "dermot", "orla", "felim"];

/// How `name` (one word) is said, when a speech model would spell it far
/// from how it's written; else none. `own`: the person's own name (About
/// you), which may take a respelling that's a name of its own. Pure.
pub fn said_as(name: &str, own: bool) -> Vec<&'static str> {
    let key = fold(name);
    SAID_AS
        .iter()
        .find(|(n, _)| *n == key)
        .map_or(&[][..], |(_, said)| *said)
        .iter()
        .copied()
        .filter(|s| own || !OWN_NAME_ONLY.contains(s))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_spelled_far_from_how_it_is_said_has_its_respellings() {
        assert!(said_as("Siobhan", false).contains(&"shivon"));
        assert!(said_as("Siobhán", false).contains(&"shivawn"));
        assert!(said_as("NIAMH", false).contains(&"neev"));
        assert!(said_as("Rochelle", true).is_empty());
        assert!(said_as("", true).is_empty());
        assert!(said_as("Zhou", false).is_empty());
        assert_eq!(said_as("Zhou", true), ["joe"]);
        assert!(!said_as("Diarmuid", false).contains(&"dermot"));
        // None is a common name of its own, nor another listed name.
        for (name, said) in SAID_AS {
            for s in *said {
                assert!(!["owen", "rory", "killian", "reece", "reese", "maeve", "ava", "katrina", "shona"].contains(s), "{name}: {s}");
                assert!(SAID_AS.iter().all(|(n, _)| n != s), "{name}: {s} is a listed name");
            }
        }
    }
}
