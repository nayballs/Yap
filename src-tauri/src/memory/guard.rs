//! What never goes into Yap's memory, whoever wrote it (the model, an
//! import, the person): **secrets** — passwords, keys and tokens, one-time
//! codes, card and account numbers, street addresses and postcodes, phone
//! numbers and email addresses — and, while "Include sensitive topics" is
//! off, **sensitive topics** in what Yap learns by itself (health, religion,
//! politics, sexuality, ethnicity and immigration, criminal records, union
//! membership). The model is told the same; these are the backstop. Pure.

use std::sync::LazyLock;

use regex::Regex;

fn re(s: &str) -> Regex {
    Regex::new(s).expect("static regex")
}

/// Why a text can't be kept: it holds a secret (`None` = fine). The reason
/// is for the log and the person ("a password"), never the secret.
pub fn secret_in(text: &str) -> Option<&'static str> {
    static PATTERNS: LazyLock<Vec<(Regex, &'static str)>> = LazyLock::new(|| {
        vec![
            // API keys and tokens by their shape.
            (re(r"\b(?:sk|pk|rk)-[A-Za-z0-9_-]{16,}"), "a key"),
            (re(r"\b(?:gsk|xai|ghp|gho|ghu|ghs|github_pat|glpat|xox[abpr])[-_][A-Za-z0-9_-]{12,}"), "a key"),
            (re(r"\bAIza[0-9A-Za-z_-]{30,}"), "a key"),
            (re(r"\bAKIA[0-9A-Z]{16}\b"), "a key"),
            (re(r"\beyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}"), "a token"),
            // A long run of letters and digits mixed, with no spaces: a token.
            (re(r"\b(?:[A-Za-z]+[0-9]+|[0-9]+[A-Za-z]+)[A-Za-z0-9_\-+/=]{18,}"), "a token"),
            // Anything about a password, a PIN or a key ("Saved password
            // hint: tulip99" came through an import, Qwen2.5-1.5B,
            // 2026-10-22): memory never needs one.
            (
                re(r"(?i)\b(?:passwords?|passcodes?|passwd|pass\s*word|pin(?:\s*code|\s*number)?\s*(?:is|was|=|:)|passphrases?|api[\s_-]*keys?|secret\s*keys?|access\s*tokens?|auth\s*tokens?|security\s*(?:answer|question)|recovery\s*(?:code|phrase)|seed\s*phrase|credentials?|login\s*details)\b"),
                "a password or key",
            ),
            (
                re(r"(?i)\b(?:code|otp|one[\s-]*time|verification|2fa|two[\s-]*factor)\b[^.\n]{0,24}\b\d{4,8}\b"),
                "a code",
            ),
            // Card numbers are checked with Luhn below; account details here.
            (re(r"(?i)\b(?:account|acct)\s*(?:number|no\.?|#)\s*(?:is|:)?\s*\d"), "an account number"),
            (re(r"\b\d{2}-\d{2}-\d{2}\b"), "a sort code"),
            (re(r"\b[A-Z]{2}\d{2}(?:\s?[A-Z0-9]{4}){3,7}\b"), "an IBAN"),
            (re(r"(?i)\b(?:routing|sort)\s*(?:number|code)\b"), "a bank number"),
            (re(r"\b\d{3}-\d{2}-\d{4}\b"), "an ID number"),
            (re(r"(?i)\b[A-CEGHJ-PR-TW-Z]{2}\s?\d{2}\s?\d{2}\s?\d{2}\s?[A-D]\b"), "an ID number"),
            (re(r"(?i)\b(?:passport|national insurance|social security|driving licen[cs]e|driver'?s licen[cs]e)\s*(?:number|no\.?)?\s*(?:is|:)?\s*[A-Z0-9]{5,}"), "an ID number"),
            // Addresses: a house number and a street, a UK postcode, a US ZIP+4.
            (
                re(r"(?i)\b\d{1,5}[a-z]?,?\s+(?:[a-z][a-z'-]+\s+){0,3}(?:street|st|road|rd|avenue|ave|lane|ln|drive|dr|close|way|court|ct|place|pl|crescent|cres|terrace|grove|gardens|gdns|boulevard|blvd|hill|square|sq|mews|row|walk|parade|highway|hwy)\b\.?"),
                "an address",
            ),
            (re(r"(?i)\b[A-PR-UWYZ][A-HK-Y]?\d[A-Z\d]?\s*\d[ABD-HJLNP-UW-Z]{2}\b"), "a postcode"),
            (re(r"\b\d{5}-\d{4}\b"), "a postcode"),
            // Contact details.
            (re(r"(?i)\b[A-Z0-9._%+-]+@[A-Z0-9.-]+\.[A-Z]{2,}\b"), "an email address"),
            (re(r"(?:\+\d{1,3}[\s.-]?)?\(?\d{2,5}\)?[\s.-]?\d{3,4}[\s.-]?\d{3,4}\b"), "a phone number"),
        ]
    });
    for (p, why) in PATTERNS.iter() {
        if p.is_match(text) {
            // A phone-shaped run of digits that's a year range or a count
            // ("2019-2024", "10000 steps") isn't a phone number.
            if *why == "a phone number" && !looks_like_phone(text, p) {
                continue;
            }
            return Some(why);
        }
    }
    if has_card_number(text) {
        return Some("a card number");
    }
    None
}

/// A phone-number match with at least 10 digits in it (a year range or a
/// short count isn't one).
fn looks_like_phone(text: &str, p: &Regex) -> bool {
    p.find_iter(text).any(|m| m.as_str().chars().filter(char::is_ascii_digit).count() >= 10)
}

/// A run of 13–19 digits (spaces or dashes between groups) that passes the
/// Luhn check: a payment card number.
fn has_card_number(text: &str) -> bool {
    static RUN: LazyLock<Regex> = LazyLock::new(|| re(r"\b\d(?:[ -]?\d){12,18}\b"));
    RUN.find_iter(text).any(|m| {
        let digits: Vec<u32> = m.as_str().chars().filter_map(|c| c.to_digit(10)).collect();
        (13..=19).contains(&digits.len()) && luhn(&digits)
    })
}

fn luhn(digits: &[u32]) -> bool {
    let sum: u32 = digits
        .iter()
        .rev()
        .enumerate()
        .map(|(i, &d)| {
            if i % 2 == 1 {
                let x = d * 2;
                if x > 9 { x - 9 } else { x }
            } else {
                d
            }
        })
        .sum();
    sum.is_multiple_of(10)
}

/// The sensitive topic a text is about (`None` = none Yap knows of): what
/// automatic learning drops while "Include sensitive topics" is off. `name`:
/// the item's own name, left out first (a colleague called Christian, a
/// friend called Faith). Word stems, case-blind.
pub fn sensitive_in(text: &str, name: &str) -> Option<&'static str> {
    static TOPICS: LazyLock<Vec<(Regex, &'static str)>> = LazyLock::new(|| {
        let topic = |words: &str, what| (re(&format!(r"(?i)\b(?:{words})")), what);
        vec![
            topic(
                r"diagnos|diabet|cancer|tumou?r|chemo|leukaemia|leukemia|depress(?:ion|ed|ive)|anxiety|panic attack|bipolar|schizophren|psychos|adhd\b|autis|asperger|\bocd\b|ptsd|eating disorder|anorexi|bulimi|\bhiv\b|\baids\b|\bstds?\b|\bstis?\b|pregnan|miscarri|\bivf\b|infertil|disabilit|disabled|epilep|seizure|asthma|arthritis|dementia|alzheimer|parkinson|multiple sclerosis|crohn|coeliac|celiac|migraine|chronic (?:pain|illness|fatigue)|long covid|heart (?:condition|disease|attack)|stroke\b|(?:had|having|needs?) (?:an? )?(?:surgery|operation)|surgery on|operation on|medication|prescri(?:bed|ption)|antidepressant|insulin|in therapy|sees a (?:therapist|counsell?or|psychiatrist)|psychiatr|rehab\b|addict|alcoholi|sober\b|sobriety|suicid|self[- ]harm|mental health|terminal(?:ly)? ill|illness|disease|condition (?:called|is)",
                "health",
            ),
            topic(
                r"religio|christian|muslim|islam|jewish|judaism|hindu|buddhis|sikh|atheis|agnostic|church\b|churchgoer|mosque|synagogue|gurdwara|catholic|protestant|anglican|evangelical|mormon|jehovah|pray(?:s|ed|ing|er)?\b|bible|quran|koran|torah|ramadan|kosher|halal",
                "religion",
            ),
            topic(
                r"vot(?:e|es|ed|ing|er) (?:for|labour|tory|conservative|reform|green|lib|democrat|republican)|labour party|conservative party|\btor(?:y|ies)\b|lib dems?|liberal democrat|democrat\b|republican|reform uk|\bsnp\b|plaid cymru|socialist|communist|left[- ]wing|right[- ]wing|political (?:views|party)|party member",
                "politics",
            ),
            topic(
                r"\bgay\b|lesbian|bisexual|\bqueer\b|transgender|\btrans (?:woman|man|person)|non[- ]binary|sexual orientation|sexuality|came out\b|coming out\b",
                "sexuality",
            ),
            topic(
                r"ethnicity|\brace\b is|immigra|asylum|refugee|visa status|undocumented|deport|citizenship status",
                "ethnicity or immigration",
            ),
            topic(
                r"criminal record|convicted|conviction|(?:in|to|out of|served time in) (?:prison|jail)|arrested|probation|on parole|offender",
                "a criminal record",
            ),
            topic(r"trade union|union member|union rep", "union membership"),
        ]
    });
    let without_name = if name.trim().is_empty() {
        text.to_string()
    } else {
        // Every word of the name, whole words, case-blind.
        name.split_whitespace().fold(text.to_string(), |t, w| {
            match Regex::new(&format!(r"(?i)\b{}\b", regex::escape(w))) {
                Ok(r) => r.replace_all(&t, " ").into_owned(),
                Err(_) => t,
            }
        })
    };
    TOPICS.iter().find(|(r, _)| r.is_match(&without_name)).map(|(_, what)| *what)
}

/// Another person's private life — their relationships, family troubles,
/// money, losses (`None` = nothing of that): never kept about someone else
/// by a pass, whatever the setting (it's about the person's own topics).
/// With [`sensitive_in`] (their health…), what a learning pass drops from a
/// person's facts. Pure.
pub fn private_in(text: &str) -> Option<&'static str> {
    static PRIVATE: LazyLock<Regex> = LazyLock::new(|| {
        re(r"(?i)\b(?:divorc|separat(?:ed|ing) from|separation|affair|broke up|break-?up|cheat(?:ed|ing) on|custody|funeral|passed away|died|bereave|grieving|miscarri|pregnan|expecting a baby|ivf|in debt|debts|bankrupt|evict|fired|sacked|laid off|redundan|disciplinary|arrested|in rehab|drinking problem|salary|pay rise|earns)")
    });
    PRIVATE.is_match(text).then_some("another person's private life")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn other_peoples_private_lives_are_not_kept() {
        assert!(private_in("Parents are divorcing").is_some());
        assert!(private_in("Wife is pregnant").is_some());
        assert!(private_in("Was laid off in May").is_some());
        assert!(private_in("Product manager at Acme").is_none());
        assert!(private_in("Lives in York").is_none());
        assert!(private_in("Birthday 3 May").is_none());
    }

    #[test]
    fn secrets_are_never_kept() {
        for s in [
            "My password is hunter2",
            "Groq key gsk_abcdefghijklmnop1234567890",
            "sk-proj-abcdefghijklmnopqrstuvwx",
            "The wifi passcode: tulip99",
            "My PIN is 4821",
            "Bank verification code 482913",
            "Card 4111 1111 1111 1111",
            "Account number 12345678",
            "Sort code 20-45-77",
            "Lives at 12 Oak Street",
            "Lives at 221b Baker St.",
            "Postcode LS1 4AP",
            "Email is nathan@example.com",
            "Mobile +44 7700 900123",
            "Phone 07700 900123",
            "NI number AB 12 34 56 C",
            "SSN 123-45-6789",
            "Token eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0",
            "Recovery phrase: apple banana",
            "Saved password hint: tulip99",
            "Uses the password tulip99 for everything",
        ] {
            assert!(secret_in(s).is_some(), "{s}");
        }
    }

    #[test]
    fn ordinary_facts_are_not_secrets() {
        for s in [
            "Lives in Leeds",
            "Works night shifts at St James's Hospital",
            "Sister, lives in York",
            "Birthday 3 May",
            "Born in 1990",
            "Worked there 2019-2024",
            "Walks 10000 steps a day",
            "Has 2 kids, aged 4 and 7",
            "Dog called Beans",
            "Runs a side business selling candles on Etsy",
            "Uses Windows 11 and an RTX 5070 Ti",
            "Prefers answers in British English",
            "Team of 12 people",
            "Project codename Phoenix",
            "Plays Baldur's Gate 3",
            "Supports Arsenal",
            "Lives on the 3rd floor",
            "Version 0.1.1 shipped",
        ] {
            assert_eq!(secret_in(s), None, "{s}");
        }
    }

    #[test]
    fn card_numbers_need_the_luhn_check() {
        assert!(has_card_number("4111-1111-1111-1111"));
        assert!(!has_card_number("1234 5678 9012 3456")); // fails Luhn
        assert!(!has_card_number("Order 12345"));
    }

    #[test]
    fn sensitive_topics_are_recognised_but_names_are_not_topics() {
        assert_eq!(sensitive_in("Has type 2 diabetes", ""), Some("health"));
        assert_eq!(sensitive_in("Takes antidepressants", ""), Some("health"));
        assert_eq!(sensitive_in("Goes to church on Sundays", ""), Some("religion"));
        assert_eq!(sensitive_in("Voted Labour", ""), Some("politics"));
        assert_eq!(sensitive_in("Is bisexual", ""), Some("sexuality"));
        assert_eq!(sensitive_in("Has a criminal record", ""), Some("a criminal record"));
        assert_eq!(sensitive_in("Is a union rep", ""), Some("union membership"));
        // A name isn't a topic.
        assert_eq!(sensitive_in("Christian is a colleague at Acme", "Christian"), None);
        assert_eq!(sensitive_in("Colleague at Acme", "Faith Hope"), None);
        // Everyday facts.
        for s in ["Works as a nurse", "Lives in Leeds", "Plays football on Sundays", "Training for a 10k race", "Has a dog called Beans", "Likes running"] {
            assert_eq!(sensitive_in(s, ""), None, "{s}");
        }
    }
}
