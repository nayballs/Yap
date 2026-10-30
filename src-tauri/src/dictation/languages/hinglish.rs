//! **Hinglish**: Hindi written in Latin letters, the way it's typed in chats
//! ("kal ki meeting teen baje hai"), for someone who picked Hindi in My
//! languages and "Write in Latin letters (Hinglish)" (`config.hinglish`).
//! Whisper writes Hindi in Devanagari; [`to_latin`] spells it out in Latin
//! letters, deterministically, on this PC — before AI cleanup, which then
//! tidies it as Hinglish (`languages::language_line`), or alone when cleanup
//! is off.
//!
//! Yap's own small table, no dependency: the letters and vowel signs
//! ([`consonant`], [`vowel`], [`matra`]), Hindi's dropped "a" (a word's last
//! consonant, and one between a vowel and a consonant with a vowel —
//! "samajhta", "karna", "kal"), the vowels as Hinglish writes them ("aa" in a
//! word's first syllable, "a" at its end: "jaana", "tha"; "ee" inside a word,
//! "i" at its end: "teen", "bhi"), and two word lists: Hindi's commonest
//! words as they're usually typed ([`COMMON`]: "hai", "nahi", "mein",
//! "chahiye") and English words Whisper writes in Devanagari ([`LOANS`]:
//! मीटिंग → "meeting"), so English stays English. Anything already in Latin
//! letters is left as it is. Pure.

/// Hindi's commonest words, as Hinglish types them (keys in [`key`]'s
/// form: no nukta, ं for ँ).
const COMMON: &[(&str, &str)] = &[
    ("है", "hai"),
    ("हैं", "hain"),
    ("में", "mein"),
    ("मैं", "main"),
    ("नहीं", "nahi"),
    ("नही", "nahi"),
    ("क्या", "kya"),
    ("यह", "yeh"),
    ("ये", "ye"),
    ("वह", "woh"),
    ("वो", "wo"),
    ("और", "aur"),
    ("कि", "ki"),
    ("की", "ki"),
    ("के", "ke"),
    ("का", "ka"),
    ("को", "ko"),
    ("से", "se"),
    ("पर", "par"),
    ("भी", "bhi"),
    ("तो", "to"),
    ("था", "tha"),
    ("थे", "the"),
    ("थी", "thi"),
    ("हूं", "hoon"),
    ("हो", "ho"),
    ("रहा", "raha"),
    ("रही", "rahi"),
    ("रहे", "rahe"),
    ("कर", "kar"),
    ("चाहिए", "chahiye"),
    ("चाहिये", "chahiye"),
    ("लिए", "liye"),
    ("लिये", "liye"),
    ("गए", "gaye"),
    ("गये", "gaye"),
    ("गया", "gaya"),
    ("गई", "gayi"),
    ("किया", "kiya"),
    ("कहा", "kaha"),
    ("अभी", "abhi"),
    ("कभी", "kabhi"),
    ("सब", "sab"),
    ("कुछ", "kuch"),
    ("बहुत", "bahut"),
    ("अच्छा", "accha"),
    ("अच्छी", "acchi"),
    ("अच्छे", "acche"),
    ("ठीक", "theek"),
    ("हां", "haan"),
    ("जी", "ji"),
    ("आप", "aap"),
    ("तुम", "tum"),
    ("हम", "hum"),
    ("मुझे", "mujhe"),
    ("मेरा", "mera"),
    ("मेरी", "meri"),
    ("मेरे", "mere"),
    ("तुम्हारा", "tumhara"),
    ("कल", "kal"),
    ("आज", "aaj"),
    ("अब", "ab"),
    ("फिर", "phir"),
    ("लेकिन", "lekin"),
    ("क्योंकि", "kyunki"),
    ("इसलिए", "isliye"),
    ("वाला", "wala"),
    ("वाली", "wali"),
    ("वाले", "wale"),
    ("यार", "yaar"),
    ("बात", "baat"),
    ("काम", "kaam"),
    ("समय", "samay"),
    ("एक", "ek"),
    ("दो", "do"),
    ("तीन", "teen"),
    ("चार", "chaar"),
    ("पांच", "paanch"),
    ("कैसे", "kaise"),
    ("कैसा", "kaisa"),
    ("कहां", "kahan"),
    ("यहां", "yahan"),
    ("वहां", "wahan"),
    ("क्यों", "kyun"),
    ("कौन", "kaun"),
    ("कब", "kab"),
    ("हमें", "hamein"),
    ("उन्हें", "unhe"),
    ("इस", "is"),
    ("उस", "us"),
    ("जो", "jo"),
    ("जब", "jab"),
    ("तब", "tab"),
    ("साथ", "saath"),
    ("बाद", "baad"),
    ("पहले", "pehle"),
    ("मत", "mat"),
    ("ना", "na"),
    ("न", "na"),
    ("जरूर", "zaroor"),
    ("ज्यादा", "zyada"),
    ("थोडा", "thoda"),
    ("थोडी", "thodi"),
    ("बडा", "bada"),
    ("बडी", "badi"),
    ("ऐसा", "aisa"),
    ("वैसे", "waise"),
    ("मतलब", "matlab"),
    ("शायद", "shayad"),
    ("बिल्कुल", "bilkul"),
    ("सही", "sahi"),
    ("गलत", "galat"),
    ("कोई", "koi"),
    ("किसी", "kisi"),
    ("हर", "har"),
    ("दिन", "din"),
    ("रात", "raat"),
    ("सुबह", "subah"),
    ("शाम", "shaam"),
    ("घंटे", "ghante"),
];

/// English words Whisper writes in Devanagari, back in English (keys in
/// [`key`]'s form).
const LOANS: &[(&str, &str)] = &[
    ("मीटिंग", "meeting"),
    ("मिटिंग", "meeting"),
    ("मीटिंग्स", "meetings"),
    ("ऑफिस", "office"),
    ("आफिस", "office"),
    ("रिपोर्ट", "report"),
    ("प्रोजेक्ट", "project"),
    ("डेडलाइन", "deadline"),
    ("प्लीज", "please"),
    ("टाइम", "time"),
    ("कॉल", "call"),
    ("ईमेल", "email"),
    ("मेल", "mail"),
    ("टीम", "team"),
    ("क्लाइंट", "client"),
    ("अपडेट", "update"),
    ("फाइल", "file"),
    ("लैपटॉप", "laptop"),
    ("फोन", "phone"),
    ("मैसेज", "message"),
    ("सॉरी", "sorry"),
    ("थैंक्स", "thanks"),
    ("थैंक्यू", "thank you"),
    ("ओके", "okay"),
    ("प्रेजेंटेशन", "presentation"),
    ("मैनेजर", "manager"),
    ("बॉस", "boss"),
    ("वीकेंड", "weekend"),
    ("मंडे", "Monday"),
    ("ट्यूजडे", "Tuesday"),
    ("वेडनेसडे", "Wednesday"),
    ("थर्सडे", "Thursday"),
    ("फ्राइडे", "Friday"),
    ("सैटरडे", "Saturday"),
    ("संडे", "Sunday"),
    ("प्लान", "plan"),
    ("बजट", "budget"),
    ("शेड्यूल", "schedule"),
    ("डॉक्यूमेंट", "document"),
    ("लिंक", "link"),
    ("शेयर", "share"),
    ("चेक", "check"),
    ("प्रॉब्लम", "problem"),
    ("प्रॉब्लेम", "problem"),
    ("आइडिया", "idea"),
    ("पॉइंट", "point"),
    ("लेट", "late"),
    ("श्योर", "sure"),
    ("एक्चुअली", "actually"),
    ("बेसिकली", "basically"),
    ("फाइनल", "final"),
    ("लिस्ट", "list"),
    ("फीडबैक", "feedback"),
    ("रिव्यू", "review"),
    ("डील", "deal"),
    ("वर्क", "work"),
    ("हॉलिडे", "holiday"),
    ("रूम", "room"),
    ("कॉफी", "coffee"),
    ("लंच", "lunch"),
    ("डिनर", "dinner"),
    ("पार्टी", "party"),
    ("बर्थडे", "birthday"),
    ("वीडियो", "video"),
    ("फोटो", "photo"),
    ("ऐप", "app"),
    ("ऑनलाइन", "online"),
    ("इंटरनेट", "internet"),
    ("कंप्यूटर", "computer"),
    ("सिस्टम", "system"),
    ("डेटा", "data"),
    ("डाटा", "data"),
    ("स्लाइड", "slide"),
    ("स्लाइड्स", "slides"),
    ("डॉक्टर", "doctor"),
    ("स्कूल", "school"),
    ("कॉलेज", "college"),
    ("क्लास", "class"),
    ("एग्जाम", "exam"),
    ("टेस्ट", "test"),
    ("टिकट", "ticket"),
    ("होटल", "hotel"),
    ("ऑर्डर", "order"),
    ("डिलीवरी", "delivery"),
    ("पेमेंट", "payment"),
    ("अकाउंट", "account"),
    ("बैंक", "bank"),
    ("कार्ड", "card"),
    ("नंबर", "number"),
    ("मॉर्निंग", "morning"),
    ("गुड", "good"),
    ("नाइस", "nice"),
    ("ग्रेट", "great"),
    ("हेलो", "hello"),
    ("हैलो", "hello"),
    ("बाय", "bye"),
    ("फ्री", "free"),
    ("मिनट", "minute"),
    ("मिनट्स", "minutes"),
    ("वीक", "week"),
    ("टास्क", "task"),
    ("इश्यू", "issue"),
    ("सर्वर", "server"),
    ("कोड", "code"),
    ("बग", "bug"),
    ("रिलीज", "release"),
    ("डेमो", "demo"),
    ("स्टेटस", "status"),
    ("कस्टमर", "customer"),
    ("सेल्स", "sales"),
    ("ब्रेक", "break"),
    ("स्टेशन", "station"),
    ("एयरपोर्ट", "airport"),
    ("फ्लाइट", "flight"),
    ("जॉब", "job"),
    ("इंटरव्यू", "interview"),
    ("सैलरी", "salary"),
    ("ऑफर", "offer"),
    ("जूम", "Zoom"),
    ("गूगल", "Google"),
    ("व्हाट्सएप", "WhatsApp"),
];

/// Any Devanagari in `text` (a letter, a sign or a digit).
pub fn has_devanagari(text: &str) -> bool {
    text.chars().any(is_devanagari)
}

fn is_devanagari(c: char) -> bool {
    ('\u{0900}'..='\u{097F}').contains(&c)
}

/// A Devanagari word's letters and signs (not the dandas, digits or the
/// abbreviation sign), and the joiners inside a word.
fn in_word(c: char) -> bool {
    (is_devanagari(c) && !matches!(c, '\u{0964}' | '\u{0965}' | '\u{0966}'..='\u{096F}' | '\u{0970}'))
        || c == '\u{200C}'
        || c == '\u{200D}'
}

/// A consonant's Latin letters (a nukta form's own: ज़ "z", फ़ "f").
fn consonant(c: char) -> Option<&'static str> {
    Some(match c {
        'क' | '\u{0958}' => "k",
        'ख' | '\u{0959}' => "kh",
        'ग' | '\u{095A}' => "g",
        'घ' => "gh",
        'ङ' => "n",
        'च' => "ch",
        'छ' => "chh",
        'ज' => "j",
        '\u{095B}' => "z",
        'झ' => "jh",
        'ञ' => "n",
        'ट' => "t",
        'ठ' => "th",
        'ड' | '\u{095C}' => "d",
        'ढ' | '\u{095D}' => "dh",
        'ण' => "n",
        'त' => "t",
        'थ' => "th",
        'द' => "d",
        'ध' => "dh",
        'न' | 'ऩ' => "n",
        'प' => "p",
        'फ' => "ph",
        '\u{095E}' => "f",
        'ब' => "b",
        'भ' => "bh",
        'म' => "m",
        'य' | '\u{095F}' => "y",
        'र' | 'ऱ' => "r",
        'ल' | 'ळ' | 'ऴ' => "l",
        'व' => "v",
        'श' | 'ष' => "sh",
        'स' => "s",
        'ह' => "h",
        _ => return None,
    })
}

/// A vowel written on its own (at a word's start, or after another vowel).
fn vowel(c: char) -> Option<Vowel> {
    Some(match c {
        'अ' | 'ऄ' => Vowel::A,
        'आ' => Vowel::Aa,
        'इ' => Vowel::I,
        'ई' => Vowel::Ee,
        'उ' => Vowel::U,
        'ऊ' => Vowel::Oo,
        'ऋ' | 'ॠ' => Vowel::Ri,
        'ए' | 'ऍ' | 'ऎ' | 'ॲ' => Vowel::E,
        'ऐ' => Vowel::Ai,
        'ओ' | 'ऑ' | 'ऒ' => Vowel::O,
        'औ' => Vowel::Au,
        _ => return None,
    })
}

/// A vowel sign on a consonant.
fn matra(c: char) -> Option<Vowel> {
    Some(match c {
        'ा' => Vowel::Aa,
        'ि' => Vowel::I,
        'ी' => Vowel::Ee,
        'ु' => Vowel::U,
        'ू' => Vowel::Oo,
        'ृ' | 'ॄ' => Vowel::Ri,
        'े' | 'ॅ' | 'ॆ' => Vowel::E,
        'ै' => Vowel::Ai,
        'ो' | 'ॉ' | 'ॊ' => Vowel::O,
        'ौ' => Vowel::Au,
        _ => return None,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Vowel {
    /// The consonant's own "a" (or अ).
    A,
    Aa,
    I,
    Ee,
    U,
    Oo,
    Ri,
    E,
    Ai,
    O,
    Au,
}

/// One letter of a word: a consonant with the vowel it carries (`None`: a
/// virama, or its "a" dropped), or a vowel on its own; nasal (ं, ँ) and
/// visarga (ः) after it.
#[derive(Debug, Clone)]
struct Unit {
    /// The consonant's letters; "" for a vowel on its own.
    cons: String,
    vowel: Option<Vowel>,
    nasal: bool,
    visarga: bool,
}

impl Unit {
    fn is_cons(&self) -> bool {
        !self.cons.is_empty()
    }
}

/// The word's letters (pure).
fn units(word: &str) -> Vec<Unit> {
    let mut out: Vec<Unit> = Vec::new();
    for c in word.chars() {
        if let Some(l) = consonant(c) {
            // A consonant carries its "a" until a sign says otherwise.
            out.push(Unit { cons: l.to_string(), vowel: Some(Vowel::A), nasal: false, visarga: false });
        } else if let Some(v) = matra(c) {
            match out.last_mut() {
                Some(u) if u.is_cons() => u.vowel = Some(v),
                _ => out.push(Unit { cons: String::new(), vowel: Some(v), nasal: false, visarga: false }),
            }
        } else if let Some(v) = vowel(c) {
            out.push(Unit { cons: String::new(), vowel: Some(v), nasal: false, visarga: false });
        } else {
            match c {
                // Virama: no vowel.
                '\u{094D}' => {
                    if let Some(u) = out.last_mut().filter(|u| u.is_cons()) {
                        u.vowel = None;
                    }
                }
                // Nukta after a consonant: ज़ "z", फ़ "f".
                '\u{093C}' => {
                    if let Some(u) = out.last_mut() {
                        match u.cons.as_str() {
                            "j" => u.cons = "z".into(),
                            "ph" => u.cons = "f".into(),
                            _ => {}
                        }
                    }
                }
                'ं' | 'ँ' | 'ऀ' => {
                    if let Some(u) = out.last_mut() {
                        u.nasal = true;
                    }
                }
                'ः' => {
                    if let Some(u) = out.last_mut() {
                        u.visarga = true;
                    }
                }
                'ॐ' => out.push(Unit { cons: String::new(), vowel: Some(Vowel::O), nasal: false, visarga: false }),
                // Avagraha, accents, joiners: nothing.
                _ => {}
            }
        }
    }
    // Clusters Hinglish spells its own way: च्छ "cch" (accha), च्च "cch"
    // (baccha), ज्ञ "gy" (gyaan).
    for i in 1..out.len() {
        if out[i - 1].vowel.is_none() && out[i - 1].cons == "ch" && out[i].cons.starts_with("ch") {
            out[i - 1].cons = "c".into();
            if out[i].cons == "chh" {
                out[i].cons = "ch".into();
            }
        }
        if out[i - 1].vowel.is_none() && out[i - 1].cons == "j" && out[i].cons == "n" {
            out[i - 1].cons = "g".into();
            out[i].cons = "y".into();
        }
    }
    drop_schwas(&mut out);
    out
}

/// Hindi's dropped "a" (pure): a word's last consonant's (unless it's a
/// one-letter word, or ends a cluster: "mantra"), then, right to left, each
/// one between a vowel and a consonant that has a vowel ("kar-na",
/// "sam-ajh-ta", "aap-ne") — never one before a nasal ("hans").
fn drop_schwas(u: &mut [Unit]) {
    let n = u.len();
    if n >= 2 {
        let last = n - 1;
        let cluster = u[last - 1].is_cons() && u[last - 1].vowel.is_none();
        if u[last].is_cons() && u[last].vowel == Some(Vowel::A) && !u[last].nasal && !u[last].visarga && !cluster {
            u[last].vowel = None;
        }
    }
    if n >= 3 {
        for i in (1..n - 1).rev() {
            let droppable = u[i].is_cons() && u[i].vowel == Some(Vowel::A) && !u[i].nasal && !u[i].visarga;
            let vowel_before = u[i - 1].vowel.is_some();
            let cv_after = u[i + 1].is_cons() && u[i + 1].vowel.is_some();
            if droppable && vowel_before && cv_after {
                u[i].vowel = None;
            }
        }
    }
}

/// A word's nasal before `next` (its following consonant's letters): "m"
/// before p, b and m, else "n".
fn nasal_before(next: Option<&str>) -> &'static str {
    match next {
        Some(c) if c.starts_with('p') || c.starts_with('b') || c.starts_with('m') => "m",
        _ => "n",
    }
}

/// One Devanagari word in Latin letters, lower case (pure): a word in the
/// lists as they have it, else letter by letter.
fn word_to_latin(word: &str) -> String {
    let k = key(word);
    if let Some((_, l)) = COMMON.iter().chain(LOANS).find(|(d, _)| *d == k) {
        return (*l).to_string();
    }
    let u = units(word);
    let n = u.len();
    let mut out = String::new();
    // Syllables so far (units with a vowel).
    let mut syllable = 0usize;
    for (i, unit) in u.iter().enumerate() {
        out.push_str(&unit.cons);
        let last = i + 1 == n;
        let first = syllable == 0;
        let after_vowel = i > 0 && u[i - 1].vowel.is_some();
        let alone = n == 1;
        let next_cons = u.get(i + 1).map(|x| x.cons.as_str()).filter(|c| !c.is_empty());
        let vowel_next = u.get(i + 1).is_some_and(|x| !x.is_cons());
        if let Some(v) = unit.vowel {
            syllable += 1;
            let nasal = unit.nasal;
            let s: String = match v {
                Vowel::A => "a".into(),
                // "aa" in the first syllable ("aaj", "jaana"), "a" after it
                // ("batana"), at the end ("tha", "kya") and before a vowel
                // ("jao", "jaiye"); "aa" alone.
                Vowel::Aa if alone && !unit.is_cons() => "aa".into(),
                Vowel::Aa if unit.is_cons() && vowel_next => "a".into(),
                Vowel::Aa if nasal => if first { "aa" } else { "a" }.into(),
                Vowel::Aa if last => "a".into(),
                Vowel::Aa => if first { "aa" } else { "a" }.into(),
                Vowel::I => "i".into(),
                // ई after a vowel: "gayi".
                Vowel::Ee if !unit.is_cons() && after_vowel => "yi".into(),
                Vowel::Ee if last => "i".into(),
                Vowel::Ee => "ee".into(),
                Vowel::U => "u".into(),
                Vowel::Oo if last && !nasal => "u".into(),
                Vowel::Oo => "oo".into(),
                Vowel::Ri => "ri".into(),
                // ए after a vowel: "liye", "gaye".
                Vowel::E if !unit.is_cons() && after_vowel => if nasal { "yei" } else { "ye" }.into(),
                // ें: "mein", "karein".
                Vowel::E if nasal => "ei".into(),
                Vowel::E => "e".into(),
                Vowel::Ai => "ai".into(),
                Vowel::O => "o".into(),
                Vowel::Au => "au".into(),
            };
            out.push_str(&s);
        }
        if unit.nasal {
            out.push_str(nasal_before(next_cons));
        }
        if unit.visarga {
            out.push('h');
        }
    }
    out
}

/// A word as the lists have it: no nukta (ज़ → ज), ं for ँ, no joiners.
fn key(word: &str) -> String {
    let mut s = String::new();
    for c in word.chars() {
        match c {
            '\u{093C}' | '\u{200C}' | '\u{200D}' => {}
            'ँ' => s.push('ं'),
            '\u{0958}' => s.push('क'),
            '\u{0959}' => s.push('ख'),
            '\u{095A}' => s.push('ग'),
            '\u{095B}' => s.push('ज'),
            '\u{095C}' => s.push('ड'),
            '\u{095D}' => s.push('ढ'),
            '\u{095E}' => s.push('फ'),
            '\u{095F}' => s.push('य'),
            other => s.push(other),
        }
    }
    s
}

/// `text` with its Devanagari in Latin letters, as Hinglish is typed (pure):
/// each Devanagari word spelt out ([`word_to_latin`]), the dandas (।, ॥) as
/// full stops, Devanagari digits as 0–9, and a sentence that starts with a
/// word spelt out given its capital. Everything else — English words Whisper
/// wrote in Latin letters, punctuation, spaces — as it was. Text with no
/// Devanagari comes back unchanged.
pub fn to_latin(text: &str) -> String {
    if !has_devanagari(text) {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    // A sentence starts here: the next word spelt out gets a capital.
    let mut sentence_start = true;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if in_word(c) {
            let start = i;
            while i < chars.len() && in_word(chars[i]) {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            // Joiners with no Devanagari among them (an emoji's ZWJ
            // sequence, a ZWNJ in another script): as they were.
            if !word.chars().any(is_devanagari) {
                out.push_str(&word);
                continue;
            }
            let mut latin = word_to_latin(&word);
            if sentence_start {
                latin = capitalised(&latin);
            }
            out.push_str(&latin);
            sentence_start = false;
            continue;
        }
        match c {
            '।' | '॥' => {
                // "है ।" → "hai."
                while out.ends_with(' ') {
                    out.pop();
                }
                out.push('.');
                sentence_start = true;
            }
            '॰' => out.push('.'),
            '०'..='९' => {
                out.push(char::from(b'0' + (c as u32 - '०' as u32) as u8));
                sentence_start = false;
            }
            '.' | '?' | '!' => {
                out.push(c);
                sentence_start = true;
            }
            c if c.is_alphanumeric() => {
                out.push(c);
                sentence_start = false;
            }
            c => out.push(c),
        }
        i += 1;
    }
    out
}

/// Whether AI cleanup's `after` kept most of `before`'s words (a Hindi
/// dictation as [`to_latin`] spelt it out): at least half of them, whole
/// words, any case. A cleanup that translated it into English keeps few —
/// the on-device Qwen2.5-1.5B did 16 times in 20 (docs/model-evals.md
/// "Hinglish") — and then the dictation goes in as Yap spelt it out. Under
/// three words: kept. Pure.
pub fn kept_the_words(before: &str, after: &str) -> bool {
    let words = |s: &str| -> Vec<String> {
        s.to_lowercase().split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(str::to_string).collect()
    };
    let b = words(before);
    if b.len() < 3 {
        return true;
    }
    let a = words(after);
    b.iter().filter(|w| a.contains(w)).count() * 2 >= b.len()
}

/// `s` with its first letter upper case.
fn capitalised(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(f) => f.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whole_sentences_come_out_as_hinglish_is_typed() {
        // Made-up sentences, as Whisper writes Hindi.
        assert_eq!(to_latin("कल की मीटिंग तीन बजे है।"), "Kal ki meeting teen baje hai.");
        assert_eq!(
            to_latin("मुझे लगता है कि हमें प्रोजेक्ट की डेडलाइन आगे बढ़ानी चाहिए।"),
            "Mujhe lagta hai ki hamein project ki deadline aage badhani chahiye."
        );
        assert_eq!(to_latin("क्या तुमने रिपोर्ट भेज दी?"), "Kya tumne report bhej di?");
        assert_eq!(to_latin("आज ऑफ़िस में बहुत काम है ।"), "Aaj office mein bahut kaam hai.");
        assert_eq!(to_latin("ठीक है, मैं ३ बजे कॉल करता हूँ।"), "Theek hai, main 3 baje call karta hoon.");
    }

    #[test]
    fn english_words_whisper_kept_in_latin_letters_stay_as_they_are() {
        assert_eq!(to_latin("कल की meeting cancel हो गई"), "Kal ki meeting cancel ho gayi");
        assert_eq!(to_latin("Please मुझे Priya का email भेज दो।"), "Please mujhe Priya ka email bhej do.");
        // No Devanagari at all: untouched, byte for byte.
        let english = "Send it to robin@example.com by 5pm, thanks!";
        assert_eq!(to_latin(english), english);
    }

    #[test]
    fn the_dropped_a_and_the_vowels_follow_hindi() {
        // A word's last "a" goes; one between a vowel and a syllable goes.
        assert_eq!(word_to_latin("कमल"), "kamal");
        assert_eq!(word_to_latin("करना"), "karna");
        assert_eq!(word_to_latin("समझता"), "samajhta");
        assert_eq!(word_to_latin("आपने"), "aapne");
        assert_eq!(word_to_latin("सकते"), "sakte");
        assert_eq!(word_to_latin("लड़की"), "ladki");
        // A one-letter word keeps it; so does a nasal syllable.
        assert_eq!(word_to_latin("व"), "va");
        assert_eq!(word_to_latin("हंस"), "hans");
        // "aa" in the first syllable, "a" after it and at the end.
        assert_eq!(word_to_latin("जाना"), "jaana");
        assert_eq!(word_to_latin("बताना"), "batana");
        assert_eq!(word_to_latin("आ"), "aa");
        // "ee" inside a word, "i" at its end; "oo" inside, "u" at the end.
        assert_eq!(word_to_latin("जीत"), "jeet");
        assert_eq!(word_to_latin("पानी"), "paani");
        assert_eq!(word_to_latin("तू"), "tu");
        assert_eq!(word_to_latin("भूल"), "bhool");
        // Nasals: n, m before p/b/m, "ein" for ें, "aan" for ाँ.
        assert_eq!(word_to_latin("हिंदी"), "hindi");
        assert_eq!(word_to_latin("करें"), "karein");
        assert_eq!(word_to_latin("हाँ"), "haan");
        assert_eq!(word_to_latin("कहाँ"), "kahan");
        assert_eq!(word_to_latin("अंबर"), "ambar");
        // A vowel after a vowel: "gaye", "aaiye", "jao".
        assert_eq!(word_to_latin("जाओ"), "jao");
        assert_eq!(word_to_latin("आइए"), "aaiye");
        assert_eq!(word_to_latin("नई"), "nayi");
        // Clusters: accha, pakka, gyaan, kyonki-like ones.
        assert_eq!(word_to_latin("पक्का"), "pakka");
        assert_eq!(word_to_latin("ज्ञान"), "gyaan");
        assert_eq!(word_to_latin("बच्चा"), "baccha");
        assert_eq!(word_to_latin("ज़िद"), "zid");
        assert_eq!(word_to_latin("फ़ैसला"), "faisla");
    }

    #[test]
    fn the_lists_win_and_take_any_spelling_of_the_word() {
        assert_eq!(word_to_latin("नहीं"), "nahi");
        assert_eq!(word_to_latin("में"), "mein");
        assert_eq!(word_to_latin("मैं"), "main");
        // With or without the nukta, ँ or ं.
        assert_eq!(word_to_latin("प्लीज़"), "please");
        assert_eq!(word_to_latin("प्लीज"), "please");
        assert_eq!(word_to_latin("हूं"), "hoon");
        assert_eq!(word_to_latin("हूँ"), "hoon");
        assert_eq!(word_to_latin("ज़रूर"), "zaroor");
        assert_eq!(word_to_latin("फ़ोन"), "phone");
        // Every list's key is in its own form, once.
        let mut seen = std::collections::HashSet::new();
        for (d, l) in COMMON.iter().chain(LOANS) {
            assert_eq!(key(d), *d, "{d} isn't in key form");
            assert!(seen.insert(*d), "{d} twice");
            assert!(!has_devanagari(l) && !l.is_empty(), "{d}");
        }
    }

    #[test]
    fn a_cleanup_that_translated_it_is_caught() {
        let said = "Mujhe lagta hai ki hamein project ki deadline aage badhani chahiye.";
        // Tidied as Hinglish: kept.
        assert!(kept_the_words(said, "Mujhe lagta hai ki hamein project ki deadline aage badhaani chahiye."));
        assert!(kept_the_words("Kal ki meeting teen baje hai.", "Kal ki meeting 3 baje hai."));
        // Translated into English: not.
        assert!(!kept_the_words(said, "I think we need to extend the project deadline."));
        assert!(!kept_the_words("Kal ki meeting teen baje hai.", "Okay, let's meet at 2 PM."));
        // Too short to tell.
        assert!(kept_the_words("Haan ji", "Yes"));
    }

    #[test]
    fn digits_dandas_and_capitals() {
        // The nukta letters written as one character each, as some keyboards
        // do: the same words.
        assert_eq!(to_latin("\u{095B}\u{0930}\u{0942}\u{0930} \u{0932}\u{0921}\u{093C}\u{0915}\u{0940}"), "Zaroor ladki");
        assert_eq!(to_latin("\u{0932}\u{095C}\u{0915}\u{0940} \u{095E}\u{0948}\u{0938}\u{0932}\u{093E}"), "Ladki faisla");
        assert_eq!(to_latin("२०२६"), "2026");
        assert_eq!(to_latin("हाँ। ठीक है॥ चलो"), "Haan. Theek hai. Chalo");
        // A sentence started by a Latin word keeps its own case.
        assert_eq!(to_latin("ok तो कल मिलते हैं"), "ok to kal milte hain");
        assert!(!has_devanagari(&to_latin("ॐ शांति ःः ऽ")));
        assert_eq!(to_latin(""), "");
    }

    #[test]
    fn odd_unicode_never_panics_and_what_isnt_hindi_stays_as_it_was() {
        // An emoji made with joiners (a family) beside Hindi keeps them; so
        // does a ZWNJ between Latin letters.
        let family = "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}";
        assert_eq!(to_latin(&format!("कल {family} आना")), format!("Kal {family} aana"));
        assert_eq!(to_latin("ठीक है, a\u{200C}b"), "Theek hai, a\u{200C}b");
        // Every Devanagari character, alone and beside the signs, joiners,
        // a Latin combining accent and an emoji, in any order: no panic, and
        // no Devanagari left.
        let others = ['\u{094D}', '\u{093C}', '\u{0902}', '\u{0901}', '\u{0903}', '\u{200C}', '\u{200D}', '\u{0301}', '\u{1F44D}', 'a', ' '];
        for c in '\u{0900}'..='\u{097F}' {
            for o in others {
                for s in [format!("{c}"), format!("{c}{o}"), format!("{o}{c}"), format!("{o}{c}{o}{c}"), format!("क{o}{c}")] {
                    let out = to_latin(&s);
                    assert!(!has_devanagari(&out), "{:?} → {out:?}", s.escape_unicode().to_string());
                }
            }
        }
    }
}
