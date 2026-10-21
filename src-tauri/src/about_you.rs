//! **About you** (Settings → General → About you, 2026-10-21): what Yap
//! tells the AI models about the person, so an answer fits where they are
//! and who they are — Nathan's ask: "It keeps finding things in dollars…"
//! (his Grok question on Qwen3.5-4B answered in US $ from a UK PC).
//!
//! - **Region and language, from Windows** ([`detected`]): the regional
//!   format (`GetUserDefaultLocaleName`, "en-GB"), the home location
//!   (`GetUserDefaultGeoName`, else `GetUserGeoID` → `GEO_ISO2`: "GB"), the
//!   currency (`LOCALE_SINTLSYMBOL` "GBP", `LOCALE_SCURRENCY` "£"), the
//!   measurement system (`LOCALE_IMEASURE`: metric or US) and the time zone
//!   (`GetDynamicTimeZoneInformation`'s key name, "GMT Standard Time" →
//!   "Europe/London" through `calendar/tz.rs`' CLDR table). Each can be
//!   changed in Settings (`config.region_*`, empty = follow Windows); a
//!   country picked there brings its own currency and units unless those
//!   are picked too ([`resolve`]).
//! - **What to call them** (`about_name`) and **anything the AI should
//!   know** (`about_me`, ≤ [`ABOUT_MAX`] characters).
//!
//! Every model gets it in one short block at the end of its system prompt
//! ([`prompt_block`]: Chat, the Voice Assistant, the Meetings and Notes Ask
//! bars; local and cloud), web searches are aimed at the country (Exa's
//! `userLocation`, Tavily's `country`, the providers' own searches — see
//! `web.rs`), and dictation cleanup keeps the person's English spelling
//! ([`cleanup_spelling`], "Use my region's spelling").
//!
//! Kept on this PC (config.json) and sent only to the AI model the person
//! uses, with each question — never to Yap's account service: a signed-out
//! Yap never contacts it, and accounts don't sync these (yet).
//!
//! **Test mode** never reads Windows' settings: the debug-only
//! `YAP_E2E_LOCALE` stands in for them — a tag ("en-GB", the rest derived
//! from its region) or JSON `{locale, country, currency, symbol, metric,
//! timeZone}` (a Windows zone name or an IANA one); unset, nothing is
//! detected. A model evaluation runs in test mode too (`--locale`).

use chrono::{DateTime, Utc};
use serde::Serialize;

/// "Anything the AI should know about you": at most this many characters.
pub const ABOUT_MAX: usize = 300;
/// "What should Yap call you?": at most this many characters.
pub const NAME_MAX: usize = 60;

// ---- tables -------------------------------------------------------------------------------

/// A country (ISO 3166-1 alpha-2), its English short name and its currency
/// (ISO 4217; "" where it has none of its own).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Country {
    pub code: &'static str,
    pub name: &'static str,
    pub currency: &'static str,
}

macro_rules! countries {
    ($(($code:literal, $name:literal, $cur:literal)),* $(,)?) => {
        &[$(Country { code: $code, name: $name, currency: $cur }),*]
    };
}

/// Every country and territory Windows can name as a home location.
pub const COUNTRIES: &[Country] = countries![
    ("AD", "Andorra", "EUR"), ("AE", "United Arab Emirates", "AED"), ("AF", "Afghanistan", "AFN"),
    ("AG", "Antigua and Barbuda", "XCD"), ("AI", "Anguilla", "XCD"), ("AL", "Albania", "ALL"),
    ("AM", "Armenia", "AMD"), ("AO", "Angola", "AOA"), ("AQ", "Antarctica", ""), ("AR", "Argentina", "ARS"),
    ("AS", "American Samoa", "USD"), ("AT", "Austria", "EUR"), ("AU", "Australia", "AUD"), ("AW", "Aruba", "AWG"),
    ("AX", "Åland Islands", "EUR"), ("AZ", "Azerbaijan", "AZN"), ("BA", "Bosnia and Herzegovina", "BAM"),
    ("BB", "Barbados", "BBD"), ("BD", "Bangladesh", "BDT"), ("BE", "Belgium", "EUR"), ("BF", "Burkina Faso", "XOF"),
    ("BG", "Bulgaria", "EUR"), ("BH", "Bahrain", "BHD"), ("BI", "Burundi", "BIF"), ("BJ", "Benin", "XOF"),
    ("BL", "Saint Barthélemy", "EUR"), ("BM", "Bermuda", "BMD"), ("BN", "Brunei", "BND"), ("BO", "Bolivia", "BOB"),
    ("BQ", "Caribbean Netherlands", "USD"), ("BR", "Brazil", "BRL"), ("BS", "Bahamas", "BSD"), ("BT", "Bhutan", "BTN"),
    ("BV", "Bouvet Island", "NOK"), ("BW", "Botswana", "BWP"), ("BY", "Belarus", "BYN"), ("BZ", "Belize", "BZD"),
    ("CA", "Canada", "CAD"), ("CC", "Cocos (Keeling) Islands", "AUD"), ("CD", "DR Congo", "CDF"),
    ("CF", "Central African Republic", "XAF"), ("CG", "Congo", "XAF"), ("CH", "Switzerland", "CHF"),
    ("CI", "Côte d'Ivoire", "XOF"), ("CK", "Cook Islands", "NZD"), ("CL", "Chile", "CLP"), ("CM", "Cameroon", "XAF"),
    ("CN", "China", "CNY"), ("CO", "Colombia", "COP"), ("CR", "Costa Rica", "CRC"), ("CU", "Cuba", "CUP"),
    ("CV", "Cape Verde", "CVE"), ("CW", "Curaçao", "XCG"), ("CX", "Christmas Island", "AUD"), ("CY", "Cyprus", "EUR"),
    ("CZ", "Czechia", "CZK"), ("DE", "Germany", "EUR"), ("DJ", "Djibouti", "DJF"), ("DK", "Denmark", "DKK"),
    ("DM", "Dominica", "XCD"), ("DO", "Dominican Republic", "DOP"), ("DZ", "Algeria", "DZD"), ("EC", "Ecuador", "USD"),
    ("EE", "Estonia", "EUR"), ("EG", "Egypt", "EGP"), ("EH", "Western Sahara", "MAD"), ("ER", "Eritrea", "ERN"),
    ("ES", "Spain", "EUR"), ("ET", "Ethiopia", "ETB"), ("FI", "Finland", "EUR"), ("FJ", "Fiji", "FJD"),
    ("FK", "Falkland Islands", "FKP"), ("FM", "Micronesia", "USD"), ("FO", "Faroe Islands", "DKK"), ("FR", "France", "EUR"),
    ("GA", "Gabon", "XAF"), ("GB", "United Kingdom", "GBP"), ("GD", "Grenada", "XCD"), ("GE", "Georgia", "GEL"),
    ("GF", "French Guiana", "EUR"), ("GG", "Guernsey", "GBP"), ("GH", "Ghana", "GHS"), ("GI", "Gibraltar", "GIP"),
    ("GL", "Greenland", "DKK"), ("GM", "Gambia", "GMD"), ("GN", "Guinea", "GNF"), ("GP", "Guadeloupe", "EUR"),
    ("GQ", "Equatorial Guinea", "XAF"), ("GR", "Greece", "EUR"), ("GS", "South Georgia and the South Sandwich Islands", "GBP"),
    ("GT", "Guatemala", "GTQ"), ("GU", "Guam", "USD"), ("GW", "Guinea-Bissau", "XOF"), ("GY", "Guyana", "GYD"),
    ("HK", "Hong Kong", "HKD"), ("HM", "Heard Island and McDonald Islands", "AUD"), ("HN", "Honduras", "HNL"),
    ("HR", "Croatia", "EUR"), ("HT", "Haiti", "HTG"), ("HU", "Hungary", "HUF"), ("ID", "Indonesia", "IDR"),
    ("IE", "Ireland", "EUR"), ("IL", "Israel", "ILS"), ("IM", "Isle of Man", "GBP"), ("IN", "India", "INR"),
    ("IO", "British Indian Ocean Territory", "USD"), ("IQ", "Iraq", "IQD"), ("IR", "Iran", "IRR"), ("IS", "Iceland", "ISK"),
    ("IT", "Italy", "EUR"), ("JE", "Jersey", "GBP"), ("JM", "Jamaica", "JMD"), ("JO", "Jordan", "JOD"), ("JP", "Japan", "JPY"),
    ("KE", "Kenya", "KES"), ("KG", "Kyrgyzstan", "KGS"), ("KH", "Cambodia", "KHR"), ("KI", "Kiribati", "AUD"),
    ("KM", "Comoros", "KMF"), ("KN", "Saint Kitts and Nevis", "XCD"), ("KP", "North Korea", "KPW"), ("KR", "South Korea", "KRW"),
    ("KW", "Kuwait", "KWD"), ("KY", "Cayman Islands", "KYD"), ("KZ", "Kazakhstan", "KZT"), ("LA", "Laos", "LAK"),
    ("LB", "Lebanon", "LBP"), ("LC", "Saint Lucia", "XCD"), ("LI", "Liechtenstein", "CHF"), ("LK", "Sri Lanka", "LKR"),
    ("LR", "Liberia", "LRD"), ("LS", "Lesotho", "LSL"), ("LT", "Lithuania", "EUR"), ("LU", "Luxembourg", "EUR"),
    ("LV", "Latvia", "EUR"), ("LY", "Libya", "LYD"), ("MA", "Morocco", "MAD"), ("MC", "Monaco", "EUR"), ("MD", "Moldova", "MDL"),
    ("ME", "Montenegro", "EUR"), ("MF", "Saint Martin", "EUR"), ("MG", "Madagascar", "MGA"), ("MH", "Marshall Islands", "USD"),
    ("MK", "North Macedonia", "MKD"), ("ML", "Mali", "XOF"), ("MM", "Myanmar", "MMK"), ("MN", "Mongolia", "MNT"),
    ("MO", "Macao", "MOP"), ("MP", "Northern Mariana Islands", "USD"), ("MQ", "Martinique", "EUR"), ("MR", "Mauritania", "MRU"),
    ("MS", "Montserrat", "XCD"), ("MT", "Malta", "EUR"), ("MU", "Mauritius", "MUR"), ("MV", "Maldives", "MVR"),
    ("MW", "Malawi", "MWK"), ("MX", "Mexico", "MXN"), ("MY", "Malaysia", "MYR"), ("MZ", "Mozambique", "MZN"),
    ("NA", "Namibia", "NAD"), ("NC", "New Caledonia", "XPF"), ("NE", "Niger", "XOF"), ("NF", "Norfolk Island", "AUD"),
    ("NG", "Nigeria", "NGN"), ("NI", "Nicaragua", "NIO"), ("NL", "Netherlands", "EUR"), ("NO", "Norway", "NOK"),
    ("NP", "Nepal", "NPR"), ("NR", "Nauru", "AUD"), ("NU", "Niue", "NZD"), ("NZ", "New Zealand", "NZD"), ("OM", "Oman", "OMR"),
    ("PA", "Panama", "USD"), ("PE", "Peru", "PEN"), ("PF", "French Polynesia", "XPF"), ("PG", "Papua New Guinea", "PGK"),
    ("PH", "Philippines", "PHP"), ("PK", "Pakistan", "PKR"), ("PL", "Poland", "PLN"), ("PM", "Saint Pierre and Miquelon", "EUR"),
    ("PN", "Pitcairn Islands", "NZD"), ("PR", "Puerto Rico", "USD"), ("PS", "Palestine", "ILS"), ("PT", "Portugal", "EUR"),
    ("PW", "Palau", "USD"), ("PY", "Paraguay", "PYG"), ("QA", "Qatar", "QAR"), ("RE", "Réunion", "EUR"), ("RO", "Romania", "RON"),
    ("RS", "Serbia", "RSD"), ("RU", "Russia", "RUB"), ("RW", "Rwanda", "RWF"), ("SA", "Saudi Arabia", "SAR"),
    ("SB", "Solomon Islands", "SBD"), ("SC", "Seychelles", "SCR"), ("SD", "Sudan", "SDG"), ("SE", "Sweden", "SEK"),
    ("SG", "Singapore", "SGD"), ("SH", "Saint Helena", "SHP"), ("SI", "Slovenia", "EUR"), ("SJ", "Svalbard and Jan Mayen", "NOK"),
    ("SK", "Slovakia", "EUR"), ("SL", "Sierra Leone", "SLE"), ("SM", "San Marino", "EUR"), ("SN", "Senegal", "XOF"),
    ("SO", "Somalia", "SOS"), ("SR", "Suriname", "SRD"), ("SS", "South Sudan", "SSP"), ("ST", "São Tomé and Príncipe", "STN"),
    ("SV", "El Salvador", "USD"), ("SX", "Sint Maarten", "XCG"), ("SY", "Syria", "SYP"), ("SZ", "Eswatini", "SZL"),
    ("TC", "Turks and Caicos Islands", "USD"), ("TD", "Chad", "XAF"), ("TF", "French Southern Territories", "EUR"),
    ("TG", "Togo", "XOF"), ("TH", "Thailand", "THB"), ("TJ", "Tajikistan", "TJS"), ("TK", "Tokelau", "NZD"),
    ("TL", "Timor-Leste", "USD"), ("TM", "Turkmenistan", "TMT"), ("TN", "Tunisia", "TND"), ("TO", "Tonga", "TOP"),
    ("TR", "Turkey", "TRY"), ("TT", "Trinidad and Tobago", "TTD"), ("TV", "Tuvalu", "AUD"), ("TW", "Taiwan", "TWD"),
    ("TZ", "Tanzania", "TZS"), ("UA", "Ukraine", "UAH"), ("UG", "Uganda", "UGX"), ("UM", "U.S. Outlying Islands", "USD"),
    ("US", "United States", "USD"), ("UY", "Uruguay", "UYU"), ("UZ", "Uzbekistan", "UZS"), ("VA", "Vatican City", "EUR"),
    ("VC", "Saint Vincent and the Grenadines", "XCD"), ("VE", "Venezuela", "VES"), ("VG", "British Virgin Islands", "USD"),
    ("VI", "U.S. Virgin Islands", "USD"), ("VN", "Vietnam", "VND"), ("VU", "Vanuatu", "VUV"), ("WF", "Wallis and Futuna", "XPF"),
    ("WS", "Samoa", "WST"), ("XK", "Kosovo", "EUR"), ("YE", "Yemen", "YER"), ("YT", "Mayotte", "EUR"),
    ("ZA", "South Africa", "ZAR"), ("ZM", "Zambia", "ZMW"), ("ZW", "Zimbabwe", "USD"),
];

/// A currency (ISO 4217) the Settings picker offers, with the symbol the
/// prompt uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Currency {
    pub code: &'static str,
    pub symbol: &'static str,
    pub name: &'static str,
}

macro_rules! currencies {
    ($(($code:literal, $sym:literal, $name:literal)),* $(,)?) => {
        &[$(Currency { code: $code, symbol: $sym, name: $name }),*]
    };
}

pub const CURRENCIES: &[Currency] = currencies![
    ("GBP", "£", "Pound sterling"), ("EUR", "€", "Euro"), ("USD", "$", "US dollar"), ("CAD", "$", "Canadian dollar"),
    ("AUD", "$", "Australian dollar"), ("NZD", "$", "New Zealand dollar"), ("CHF", "CHF", "Swiss franc"),
    ("SEK", "kr", "Swedish krona"), ("NOK", "kr", "Norwegian krone"), ("DKK", "kr", "Danish krone"), ("ISK", "kr", "Icelandic króna"),
    ("PLN", "zł", "Polish złoty"), ("CZK", "Kč", "Czech koruna"), ("HUF", "Ft", "Hungarian forint"), ("RON", "lei", "Romanian leu"),
    ("RSD", "din", "Serbian dinar"), ("TRY", "₺", "Turkish lira"), ("UAH", "₴", "Ukrainian hryvnia"), ("RUB", "₽", "Russian rouble"),
    ("ILS", "₪", "Israeli shekel"), ("AED", "AED", "UAE dirham"), ("SAR", "SAR", "Saudi riyal"), ("QAR", "QAR", "Qatari riyal"),
    ("EGP", "E£", "Egyptian pound"), ("ZAR", "R", "South African rand"), ("NGN", "₦", "Nigerian naira"), ("KES", "KSh", "Kenyan shilling"),
    ("INR", "₹", "Indian rupee"), ("PKR", "Rs", "Pakistani rupee"), ("BDT", "৳", "Bangladeshi taka"), ("LKR", "Rs", "Sri Lankan rupee"),
    ("JPY", "¥", "Japanese yen"), ("CNY", "¥", "Chinese yuan"), ("HKD", "$", "Hong Kong dollar"), ("TWD", "NT$", "New Taiwan dollar"),
    ("KRW", "₩", "South Korean won"), ("SGD", "$", "Singapore dollar"), ("MYR", "RM", "Malaysian ringgit"), ("THB", "฿", "Thai baht"),
    ("IDR", "Rp", "Indonesian rupiah"), ("PHP", "₱", "Philippine peso"), ("VND", "₫", "Vietnamese đồng"),
    ("BRL", "R$", "Brazilian real"), ("MXN", "$", "Mexican peso"), ("ARS", "$", "Argentine peso"), ("CLP", "$", "Chilean peso"),
    ("COP", "$", "Colombian peso"), ("PEN", "S/", "Peruvian sol"),
];

/// Language names (ISO 639-1, and the few 639-2 codes Windows uses) in English.
const LANGUAGES: &[(&str, &str)] = &[
    ("af", "Afrikaans"), ("am", "Amharic"), ("ar", "Arabic"), ("az", "Azerbaijani"), ("be", "Belarusian"), ("bg", "Bulgarian"),
    ("bn", "Bengali"), ("bs", "Bosnian"), ("ca", "Catalan"), ("cs", "Czech"), ("cy", "Welsh"), ("da", "Danish"), ("de", "German"),
    ("el", "Greek"), ("en", "English"), ("es", "Spanish"), ("et", "Estonian"), ("eu", "Basque"), ("fa", "Persian"), ("fi", "Finnish"),
    ("fil", "Filipino"), ("fr", "French"), ("ga", "Irish"), ("gd", "Scottish Gaelic"), ("gl", "Galician"), ("gu", "Gujarati"),
    ("he", "Hebrew"), ("hi", "Hindi"), ("hr", "Croatian"), ("hu", "Hungarian"), ("hy", "Armenian"), ("id", "Indonesian"),
    ("is", "Icelandic"), ("it", "Italian"), ("ja", "Japanese"), ("ka", "Georgian"), ("kk", "Kazakh"), ("km", "Khmer"),
    ("kn", "Kannada"), ("ko", "Korean"), ("lt", "Lithuanian"), ("lv", "Latvian"), ("mk", "Macedonian"), ("ml", "Malayalam"),
    ("mn", "Mongolian"), ("mr", "Marathi"), ("ms", "Malay"), ("mt", "Maltese"), ("nb", "Norwegian"), ("ne", "Nepali"),
    ("nl", "Dutch"), ("nn", "Norwegian"), ("no", "Norwegian"), ("pa", "Punjabi"), ("pl", "Polish"), ("pt", "Portuguese"),
    ("ro", "Romanian"), ("ru", "Russian"), ("si", "Sinhala"), ("sk", "Slovak"), ("sl", "Slovenian"), ("sq", "Albanian"),
    ("sr", "Serbian"), ("sv", "Swedish"), ("sw", "Swahili"), ("ta", "Tamil"), ("te", "Telugu"), ("th", "Thai"), ("tl", "Tagalog"),
    ("tr", "Turkish"), ("uk", "Ukrainian"), ("ur", "Urdu"), ("uz", "Uzbek"), ("vi", "Vietnamese"), ("xh", "Xhosa"),
    ("zh", "Chinese"), ("zu", "Zulu"),
];

/// The regional formats the Settings language picker offers (any other tag
/// Windows reports still works, shown by its own name).
pub const LANGUAGE_CHOICES: &[&str] = &[
    "en-GB", "en-US", "en-AU", "en-CA", "en-IE", "en-IN", "en-NZ", "en-ZA", "en-SG", "fr-FR", "fr-CA", "fr-BE", "fr-CH",
    "de-DE", "de-AT", "de-CH", "es-ES", "es-MX", "es-US", "it-IT", "pt-PT", "pt-BR", "nl-NL", "nl-BE", "pl-PL", "sv-SE",
    "da-DK", "nb-NO", "fi-FI", "cs-CZ", "hu-HU", "ro-RO", "el-GR", "tr-TR", "uk-UA", "ru-RU", "he-IL", "ar-SA", "hi-IN",
    "ja-JP", "ko-KR", "zh-CN", "zh-TW", "th-TH", "vi-VN", "id-ID",
];

/// Countries that measure in US customary units (miles, pounds, °F).
const IMPERIAL: &[&str] = &["US", "LR", "MM", "PR", "GU", "VI", "AS", "MP", "UM"];

/// Where English is spelled the American way, and the Canadian way; any
/// other region of an English-speaking user spells it the British way.
const AMERICAN: &[&str] = &["US", "PR", "GU", "VI", "AS", "MP", "UM", "PH", "LR"];

/// Tavily's `country` values (docs.tavily.com, search endpoint, as read on
/// 21 Oct 2026): a lower-case English name, "only if topic is general".
const TAVILY_COUNTRIES: &[&str] = &[
    "afghanistan", "albania", "algeria", "andorra", "angola", "argentina", "armenia", "australia", "austria", "azerbaijan",
    "bahamas", "bahrain", "bangladesh", "barbados", "belarus", "belgium", "belize", "benin", "bhutan", "bolivia",
    "bosnia and herzegovina", "botswana", "brazil", "brunei", "bulgaria", "burkina faso", "burundi", "cambodia", "cameroon",
    "canada", "cape verde", "central african republic", "chad", "chile", "china", "colombia", "comoros", "congo",
    "costa rica", "croatia", "cuba", "cyprus", "czech republic", "denmark", "djibouti", "dominican republic", "ecuador",
    "egypt", "el salvador", "equatorial guinea", "eritrea", "estonia", "ethiopia", "fiji", "finland", "france", "gabon",
    "gambia", "georgia", "germany", "ghana", "greece", "guatemala", "guinea", "haiti", "honduras", "hungary", "iceland",
    "india", "indonesia", "iran", "iraq", "ireland", "israel", "italy", "jamaica", "japan", "jordan", "kazakhstan", "kenya",
    "kuwait", "kyrgyzstan", "latvia", "lebanon", "lesotho", "liberia", "libya", "liechtenstein", "lithuania", "luxembourg",
    "madagascar", "malawi", "malaysia", "maldives", "mali", "malta", "mauritania", "mauritius", "mexico", "moldova", "monaco",
    "mongolia", "montenegro", "morocco", "mozambique", "myanmar", "namibia", "nepal", "netherlands", "new zealand",
    "nicaragua", "niger", "nigeria", "north korea", "north macedonia", "norway", "oman", "pakistan", "panama",
    "papua new guinea", "paraguay", "peru", "philippines", "poland", "portugal", "qatar", "romania", "russia", "rwanda",
    "saudi arabia", "senegal", "serbia", "singapore", "slovakia", "slovenia", "somalia", "south africa", "south korea",
    "south sudan", "spain", "sri lanka", "sudan", "sweden", "switzerland", "syria", "taiwan", "tajikistan", "tanzania",
    "thailand", "togo", "trinidad and tobago", "tunisia", "turkey", "turkmenistan", "uganda", "ukraine",
    "united arab emirates", "united kingdom", "united states", "uruguay", "uzbekistan", "venezuela", "vietnam", "yemen",
    "zambia", "zimbabwe",
];

pub fn country(code: &str) -> Option<&'static Country> {
    let code = code.trim();
    COUNTRIES.iter().find(|c| c.code.eq_ignore_ascii_case(code))
}

pub fn currency(code: &str) -> Option<&'static Currency> {
    let code = code.trim();
    CURRENCIES.iter().find(|c| c.code.eq_ignore_ascii_case(code))
}

/// A country's name as a sentence says it ("the United Kingdom", "France").
/// Pure.
pub fn in_sentence(c: &Country) -> String {
    let n = c.name;
    let the = n.starts_with("United ")
        || n.ends_with(" Islands")
        || n.contains("Republic")
        || ["NL", "PH", "BS", "GM", "MV", "KM", "CD", "CG", "VA", "IO", "TF", "UM", "GS", "HM"].contains(&c.code);
    if the {
        format!("the {n}")
    } else {
        n.to_string()
    }
}

/// A country's short name for a summary ("UK", "US", "UAE", else its name).
/// Pure.
pub fn short_name(code: &str) -> String {
    match code.to_ascii_uppercase().as_str() {
        "GB" => "UK".into(),
        "US" => "US".into(),
        "AE" => "UAE".into(),
        other => country(other).map_or_else(|| other.to_string(), |c| c.name.to_string()),
    }
}

/// Tavily's name for a country, when it has one. Pure.
pub fn tavily_country(code: &str) -> Option<&'static str> {
    let c = country(code)?;
    let name = match c.code {
        "CZ" => "czech republic".to_string(),
        "CD" | "CG" => "congo".to_string(),
        _ => c.name.to_lowercase(),
    };
    TAVILY_COUNTRIES.iter().copied().find(|t| *t == name)
}

/// The language and region of a tag ("en-GB" → ("en", Some("GB")),
/// "zh-Hans-CN" → ("zh", Some("CN"))). Pure.
pub fn split_tag(tag: &str) -> (String, Option<String>) {
    let mut parts = tag.trim().split(['-', '_']);
    let lang = parts.next().unwrap_or("").to_ascii_lowercase();
    let region = parts.find(|p| p.len() == 2 && p.chars().all(|c| c.is_ascii_alphabetic())).map(|p| p.to_ascii_uppercase());
    (lang, region)
}

/// A language's English name ("en" → "English"). Pure.
pub fn language_name(code: &str) -> Option<&'static str> {
    let code = code.trim().to_ascii_lowercase();
    LANGUAGES.iter().find(|(c, _)| *c == code).map(|(_, n)| *n)
}

/// A tag as Settings shows it: "English (UK)", "French (Canada)". Pure.
pub fn tag_name(tag: &str) -> String {
    let (lang, region) = split_tag(tag);
    let name = language_name(&lang).map(str::to_string).unwrap_or_else(|| tag.trim().to_string());
    match region {
        Some(r) => format!("{name} ({})", short_name(&r)),
        None => name,
    }
}

/// How English is spelled where the person is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Spelling {
    British,
    American,
    Canadian,
}

impl Spelling {
    pub fn name(self) -> &'static str {
        match self {
            Spelling::British => "British",
            Spelling::American => "American",
            Spelling::Canadian => "Canadian",
        }
    }
    /// A few words in that spelling, for the prompt and Settings.
    pub fn examples(self) -> &'static str {
        match self {
            Spelling::British => "colour, organise, centre, travelled",
            Spelling::American => "color, organize, center, traveled",
            Spelling::Canadian => "colour, organize, centre, travelled",
        }
    }
}

fn spelling_for_region(region: &str) -> Spelling {
    let r = region.to_ascii_uppercase();
    if AMERICAN.contains(&r.as_str()) {
        Spelling::American
    } else if r == "CA" {
        Spelling::Canadian
    } else {
        Spelling::British
    }
}

/// English-speaking countries: where a person whose language Yap doesn't
/// know still spells English that country's way.
const ENGLISH_SPEAKING: &[&str] = &["GB", "US", "CA", "AU", "NZ", "IE", "ZA", "IM", "JE", "GG", "GI", "PR", "GU", "VI"];

/// The time zone's name in a sentence: "UK time" for Europe/London, else
/// its city ("New York time"), or the zone itself. Pure.
pub fn zone_label(iana: &str) -> String {
    match iana {
        "Europe/London" => "UK time".into(),
        "Europe/Dublin" => "Irish time".into(),
        z if z.starts_with("Etc/") || !z.contains('/') => z.trim_start_matches("Etc/").to_string(),
        z => format!("{} time", z.rsplit('/').next().unwrap_or(z).replace('_', " ")),
    }
}

// ---- what Windows says ---------------------------------------------------------------------

/// The person's region as Windows (or a test run's stand-in) has it.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Detected {
    /// The regional format, "en-GB".
    pub locale: Option<String>,
    /// The home location, "GB".
    pub country: Option<String>,
    /// "GBP", and its symbol "£".
    pub currency: Option<String>,
    pub symbol: Option<String>,
    /// Metric (true) or US customary units.
    pub metric: Option<bool>,
    /// IANA, "Europe/London".
    pub time_zone: Option<String>,
}

/// What Windows says (never read in a test run: [`stand_in`]).
pub fn detected() -> Detected {
    if crate::e2e::active() {
        return stand_in();
    }
    windows_detected()
}

/// A test run's stand-in for Windows' settings (`YAP_E2E_LOCALE`, debug
/// builds only): a tag ("en-GB") or JSON. Nothing when it isn't set.
fn stand_in() -> Detected {
    #[cfg(debug_assertions)]
    {
        if let Ok(v) = std::env::var("YAP_E2E_LOCALE") {
            return parse_stand_in(&v);
        }
    }
    Detected::default()
}

/// `YAP_E2E_LOCALE`'s value: a tag, its region's currency and units
/// derived; or JSON `{locale, country, currency, symbol, metric, timeZone}`.
/// Pure.
pub fn parse_stand_in(v: &str) -> Detected {
    let v = v.trim();
    if v.starts_with('{') {
        let j: serde_json::Value = serde_json::from_str(v).unwrap_or_default();
        let s = |k: &str| j[k].as_str().map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);
        return Detected {
            locale: s("locale"),
            country: s("country").map(|c| c.to_ascii_uppercase()),
            currency: s("currency").map(|c| c.to_ascii_uppercase()),
            symbol: s("symbol"),
            metric: j["metric"].as_bool(),
            time_zone: s("timeZone").and_then(|z| iana_zone(&z)),
        };
    }
    if v.is_empty() {
        return Detected::default();
    }
    let (_, region) = split_tag(v);
    let c = region.as_deref().and_then(country);
    let cur = c.map(|c| c.currency).filter(|c| !c.is_empty());
    Detected {
        locale: Some(v.to_string()),
        country: c.map(|c| c.code.to_string()),
        currency: cur.map(str::to_string),
        symbol: cur.and_then(currency).map(|c| c.symbol.to_string()),
        metric: c.map(|c| !IMPERIAL.contains(&c.code)),
        time_zone: None,
    }
}

/// An IANA zone from a Windows zone name or an IANA one. Pure.
pub fn iana_zone(name: &str) -> Option<String> {
    let name = name.trim();
    if name.parse::<chrono_tz::Tz>().is_ok() {
        return Some(name.to_string());
    }
    crate::calendar::tz::windows_to_iana(name).map(str::to_string)
}

#[cfg(windows)]
fn windows_detected() -> Detected {
    use windows::core::PCWSTR;
    use windows::Win32::Globalization::{
        GetGeoInfoW, GetLocaleInfoEx, GetUserDefaultGeoName, GetUserDefaultLocaleName, GetUserGeoID, GEOCLASS_NATION,
        GEO_ISO2, LOCALE_IMEASURE, LOCALE_SCURRENCY, LOCALE_SINTLSYMBOL,
    };
    use windows::Win32::System::Time::{GetDynamicTimeZoneInformation, DYNAMIC_TIME_ZONE_INFORMATION};

    let text = |buf: &[u16], n: i32| {
        let n = usize::try_from(n).unwrap_or(0).min(buf.len());
        (n > 1).then(|| String::from_utf16_lossy(&buf[..n - 1])).filter(|s| !s.trim().is_empty())
    };
    // SAFETY (every call below): stack buffers; Windows writes a
    // NUL-terminated string into each and returns its length with the NUL
    // (0 = failed).
    let mut buf = [0u16; 85];
    let n = unsafe { GetUserDefaultLocaleName(&mut buf) };
    let locale = text(&buf, n);
    // The user's own regional format (LOCALE_NAME_USER_DEFAULT = null), with
    // their own changes to it.
    let info = |lctype: u32| {
        let mut out = [0u16; 32];
        let n = unsafe { GetLocaleInfoEx(PCWSTR::null(), lctype, Some(&mut out)) };
        text(&out, n)
    };
    let mut geo = [0u16; 16];
    let n = unsafe { GetUserDefaultGeoName(&mut geo) };
    let mut country = text(&geo, n);
    // Windows before 1709, or no home location set ("001" = the world).
    if !country.as_deref().is_some_and(|c| c.len() == 2 && c.chars().all(|ch| ch.is_ascii_alphabetic())) {
        let id = unsafe { GetUserGeoID(GEOCLASS_NATION) };
        let mut iso = [0u16; 8];
        let n = unsafe { GetGeoInfoW(id, GEO_ISO2, Some(&mut iso), 0) };
        country = text(&iso, n).filter(|c| c.len() == 2);
    }
    let mut tzi = DYNAMIC_TIME_ZONE_INFORMATION::default();
    // SAFETY: a valid out-pointer to a zeroed struct.
    let ok = unsafe { GetDynamicTimeZoneInformation(&mut tzi) } != u32::MAX;
    let key = ok.then(|| {
        let end = tzi.TimeZoneKeyName.iter().position(|&c| c == 0).unwrap_or(tzi.TimeZoneKeyName.len());
        String::from_utf16_lossy(&tzi.TimeZoneKeyName[..end])
    });
    Detected {
        locale,
        country: country.map(|c| c.to_ascii_uppercase()),
        currency: info(LOCALE_SINTLSYMBOL).filter(|c| c.len() == 3),
        symbol: info(LOCALE_SCURRENCY),
        metric: info(LOCALE_IMEASURE).map(|m| m.trim() != "1"),
        time_zone: key.as_deref().and_then(iana_zone),
    }
}

#[cfg(not(windows))]
fn windows_detected() -> Detected {
    Detected::default()
}

// ---- what the person changed, and the result ---------------------------------------------

/// Settings' changes (`config.region_*`; empty = follow Windows).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Overrides {
    pub country: String,
    pub language: String,
    pub currency: String,
    /// "metric" | "imperial" | "".
    pub units: String,
    /// IANA.
    pub time_zone: String,
}

impl Overrides {
    pub fn from_config(cfg: &crate::config::YapConfig) -> Overrides {
        Overrides {
            country: cfg.region_country.clone(),
            language: cfg.region_language.clone(),
            currency: cfg.region_currency.clone(),
            units: cfg.region_units.clone(),
            time_zone: cfg.region_time_zone.clone(),
        }
    }
}

/// Where the person is, what they speak, their money, units and clock: the
/// result of Windows' settings and Settings' changes.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Region {
    pub country: Option<String>,
    pub country_name: Option<String>,
    pub language: Option<String>,
    pub language_name: Option<String>,
    pub currency: Option<String>,
    pub symbol: Option<String>,
    pub metric: Option<bool>,
    pub time_zone: Option<String>,
    pub time_zone_label: Option<String>,
    /// How English is spelled there (`None`: not an English speaker, or
    /// not known).
    pub spelling: Option<Spelling>,
}

/// Windows' settings with Settings' changes on top. A country picked in
/// Settings brings its own currency and units (unless those are picked
/// too); the language and the time zone follow Windows until picked. Pure.
pub fn resolve(d: &Detected, o: &Overrides) -> Region {
    let picked_country = country(&o.country).map(|c| c.code.to_string());
    let language = Some(o.language.trim()).filter(|l| !l.is_empty()).map(str::to_string).or_else(|| d.locale.clone());
    let lang_region = language.as_deref().and_then(|l| split_tag(l).1);
    let country_code = picked_country
        .clone()
        .or_else(|| d.country.clone().filter(|c| country(c).is_some()))
        .or_else(|| lang_region.clone().filter(|c| country(c).is_some()));
    let follows_windows = picked_country.is_none() || picked_country == d.country;

    let (currency_code, symbol) = match currency(&o.currency) {
        Some(c) => (Some(c.code.to_string()), Some(c.symbol.to_string())),
        None if !o.currency.trim().is_empty() && o.currency.trim().len() == 3 => {
            let code = o.currency.trim().to_ascii_uppercase();
            (Some(code.clone()), Some(code))
        }
        None if follows_windows && d.currency.is_some() => (d.currency.clone(), d.symbol.clone().or_else(|| d.currency.clone())),
        None => {
            let code = country_code.as_deref().and_then(country).map(|c| c.currency).filter(|c| !c.is_empty());
            (code.map(str::to_string), code.map(|c| currency(c).map_or(c, |k| k.symbol).to_string()))
        }
    };
    let metric = match o.units.trim() {
        "metric" => Some(true),
        "imperial" | "us" => Some(false),
        _ if follows_windows && d.metric.is_some() => d.metric,
        _ => country_code.as_deref().map(|c| !IMPERIAL.contains(&c)),
    };
    let time_zone = Some(o.time_zone.trim())
        .filter(|z| z.parse::<chrono_tz::Tz>().is_ok())
        .map(str::to_string)
        .or_else(|| d.time_zone.clone());
    let spelling = match language.as_deref().map(split_tag) {
        Some((lang, region)) if lang == "en" => region.or_else(|| country_code.clone()).map(|r| spelling_for_region(&r)),
        Some((lang, _)) if !lang.is_empty() => None,
        _ => country_code.as_deref().filter(|c| ENGLISH_SPEAKING.contains(c)).map(spelling_for_region),
    };
    Region {
        country_name: country_code.as_deref().and_then(country).map(|c| c.name.to_string()),
        country: country_code,
        language_name: language.as_deref().map(tag_name),
        language,
        currency: currency_code,
        symbol,
        metric,
        time_zone_label: time_zone.as_deref().map(zone_label),
        time_zone,
        spelling,
    }
}

/// The region as Settings shows it: "United Kingdom · English (UK) · £ GBP ·
/// metric · UK time". Pure.
pub fn summary(r: &Region) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(n) = &r.country_name {
        parts.push(n.clone());
    }
    if let Some(l) = &r.language_name {
        parts.push(l.clone());
    }
    match (&r.symbol, &r.currency) {
        (Some(s), Some(c)) if s != c => parts.push(format!("{s} {c}")),
        (_, Some(c)) => parts.push(c.clone()),
        _ => {}
    }
    if let Some(m) = r.metric {
        parts.push(if m { "metric" } else { "US units" }.into());
    }
    if let Some(z) = &r.time_zone_label {
        parts.push(z.clone());
    }
    parts.join(" · ")
}

/// Everything the models are told: the region, and the name and note.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    #[serde(flatten)]
    pub region: Region,
    pub name: String,
    pub about: String,
}

/// The person's profile, as Settings has it now.
pub fn profile(cfg: &crate::config::YapConfig) -> Profile {
    Profile {
        region: resolve(&detected(), &Overrides::from_config(cfg)),
        name: one_line(&cfg.about_name, NAME_MAX),
        about: one_line(&cfg.about_me, ABOUT_MAX),
    }
}

/// Whitespace runs as one space, at most `max` characters. Pure.
fn one_line(s: &str, max: usize) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(max).collect()
}

/// The language as the prompt says it: "British English", "French". Pure.
fn language_in_prompt(r: &Region) -> Option<String> {
    let (lang, region) = split_tag(r.language.as_deref()?);
    if lang == "en" {
        let variety = match region.as_deref().or(r.country.as_deref()) {
            Some("GB" | "IM" | "JE" | "GG") => "British English",
            Some(c) if AMERICAN.contains(&c) => "American English",
            Some("CA") => "Canadian English",
            Some("AU") => "Australian English",
            Some("NZ") => "New Zealand English",
            Some("IE") => "Irish English",
            Some("IN") => "Indian English",
            Some("ZA") => "South African English",
            _ => "English",
        };
        return Some(variety.to_string());
    }
    language_name(&lang).map(str::to_string)
}

/// **The block every model gets**, last in its system prompt (where small
/// models heed it): where the person is and what they speak, the date and
/// their time, how to give prices, times and units, their name and what
/// they said about themselves — only the parts that are known. `now`: the
/// moment it describes. Pure.
///
/// "About the user: they're in the United Kingdom (British English); it's
/// Tuesday 21 October 2026, 21:40 UK time (UTC+01:00). Give prices in £
/// (GBP), times in UK time and metric units; if a source gives a price only
/// in another currency, say which currency and give that figure as it is.
/// Their name is Nathan. They told Yap: "I play PC games.""
pub fn prompt_block(p: &Profile, now: DateTime<Utc>) -> String {
    let r = &p.region;
    let zone = r.time_zone.as_deref().and_then(|z| z.parse::<chrono_tz::Tz>().ok());
    let when = match zone {
        Some(tz) => {
            let t = now.with_timezone(&tz);
            format!(
                "it's {}, {} {} (UTC{})",
                t.format("%A %-d %B %Y"),
                t.format("%H:%M"),
                r.time_zone_label.clone().unwrap_or_default(),
                t.format("%:z")
            )
            .replace("  ", " ")
        }
        None => {
            let t = now.with_timezone(&chrono::Local);
            format!("it's {}, {} (UTC{})", t.format("%A %-d %B %Y"), t.format("%H:%M"), t.format("%:z"))
        }
    };
    let place = r.country.as_deref().and_then(country).map(in_sentence);
    let lang = language_in_prompt(r);
    let mut out = String::from("About the user: ");
    out.push_str(&match (place, lang) {
        (Some(c), Some(l)) => format!("they're in {c} ({l}); {when}."),
        (Some(c), None) => format!("they're in {c}; {when}."),
        (None, Some(l)) => format!("they speak {l}; {when}."),
        (None, None) => format!("{when}."),
    });
    let mut give: Vec<String> = Vec::new();
    match (&r.symbol, &r.currency) {
        (Some(s), Some(c)) if s != c => give.push(format!("prices in {s} ({c})")),
        (_, Some(c)) => give.push(format!("prices in {c}")),
        _ => {}
    }
    if let Some(l) = &r.time_zone_label {
        give.push(format!("times in {l}"));
    }
    if let Some(m) = r.metric {
        give.push(if m { "metric units" } else { "US units (miles, pounds, °F)" }.to_string());
    }
    if !give.is_empty() {
        let list = match give.len() {
            1 => give[0].clone(),
            n => format!("{} and {}", give[..n - 1].join(", "), give[n - 1]),
        };
        out.push_str(&format!(" Give {list}"));
        if r.currency.is_some() {
            out.push_str("; if a source gives a price only in another currency, say which currency and give that figure as it is");
        }
        out.push('.');
    }
    if !p.name.is_empty() {
        out.push_str(&format!(" Their name is {}.", p.name));
    }
    if !p.about.is_empty() {
        out.push_str(&format!(" They told Yap: \"{}\"", p.about.replace('"', "'")));
    }
    out
}

/// [`prompt_block`] for now, as Settings has it.
pub fn prompt_line(cfg: &crate::config::YapConfig) -> String {
    prompt_block(&profile(cfg), Utc::now())
}

/// Dictation cleanup's spelling line ("Use my region's spelling", on by
/// default): keep the person's variety of English. `None` when it's off or
/// the variety isn't known. Pure over the region.
pub fn spelling_line(on: bool, r: &Region) -> Option<String> {
    let s = r.spelling.filter(|_| on)?;
    Some(format!(
        "When the transcript is in English, spell it the {} way ({}), without changing any words.",
        s.name(),
        s.examples()
    ))
}

/// [`spelling_line`] as Settings has it now.
pub fn cleanup_spelling(cfg: &crate::config::YapConfig) -> Option<String> {
    if !cfg.region_spelling {
        return None;
    }
    spelling_line(true, &profile(cfg).region)
}

// ---- Settings ------------------------------------------------------------------------------

/// `about_you_status {overrides?}`: what Windows says, the result of
/// Settings' changes (the ones passed, else the saved ones), its summary and
/// the prompt block as it reads now, and the pickers' choices.
#[tauri::command]
pub fn about_you_status(overrides: Option<serde_json::Value>) -> serde_json::Value {
    let cfg = crate::config::load();
    let mut o = Overrides::from_config(&cfg);
    let (mut name, mut about, mut spelling_on) = (cfg.about_name.clone(), cfg.about_me.clone(), cfg.region_spelling);
    if let Some(v) = overrides {
        let s = |k: &str| v[k].as_str().map(str::to_string);
        o = Overrides {
            country: s("regionCountry").unwrap_or(o.country),
            language: s("regionLanguage").unwrap_or(o.language),
            currency: s("regionCurrency").unwrap_or(o.currency),
            units: s("regionUnits").unwrap_or(o.units),
            time_zone: s("regionTimeZone").unwrap_or(o.time_zone),
        };
        name = s("aboutName").unwrap_or(name);
        about = s("aboutMe").unwrap_or(about);
        spelling_on = v["regionSpelling"].as_bool().unwrap_or(spelling_on);
    }
    let d = detected();
    let windows = resolve(&d, &Overrides::default());
    // What "Automatic" means for the currency and units with the country
    // picked now (Windows', or the picked country's own).
    let auto = resolve(&d, &Overrides { currency: String::new(), units: String::new(), ..o.clone() });
    let region = resolve(&d, &o);
    let profile = Profile { region: region.clone(), name: one_line(&name, NAME_MAX), about: one_line(&about, ABOUT_MAX) };
    serde_json::json!({
        "windows": windows,
        "windowsSummary": summary(&windows),
        "auto": auto,
        "region": region,
        "summary": summary(&region),
        "prompt": prompt_block(&profile, Utc::now()),
        "spellingLine": spelling_line(spelling_on, &region),
        "spellingExamples": region.spelling.map(Spelling::examples),
        "aboutMax": ABOUT_MAX,
        "nameMax": NAME_MAX,
        "countries": COUNTRIES.iter().map(|c| serde_json::json!({ "code": c.code, "name": c.name })).collect::<Vec<_>>(),
        "currencies": CURRENCIES.iter().map(|c| serde_json::json!({ "code": c.code, "symbol": c.symbol, "name": c.name })).collect::<Vec<_>>(),
        "languages": LANGUAGE_CHOICES.iter().map(|t| serde_json::json!({ "tag": t, "name": tag_name(t) })).collect::<Vec<_>>(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn uk() -> Detected {
        Detected {
            locale: Some("en-GB".into()),
            country: Some("GB".into()),
            currency: Some("GBP".into()),
            symbol: Some("£".into()),
            metric: Some(true),
            time_zone: Some("Europe/London".into()),
        }
    }

    // 21 Oct 2026, 20:40 UTC = 21:40 in London (BST).
    fn at() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 21, 20, 40, 0).unwrap()
    }

    #[test]
    fn nathans_pc_reads_as_the_uk() {
        let r = resolve(&uk(), &Overrides::default());
        assert_eq!(summary(&r), "United Kingdom · English (UK) · £ GBP · metric · UK time");
        assert_eq!(r.spelling, Some(Spelling::British));
        let p = Profile { region: r, name: "Nathan".into(), about: "I'm in Manchester, I play PC games, I work day shifts".into() };
        assert_eq!(
            prompt_block(&p, at()),
            "About the user: they're in the United Kingdom (British English); it's Wednesday 21 October 2026, 21:40 UK time (UTC+01:00). Give prices in £ (GBP), times in UK time and metric units; if a source gives a price only in another currency, say which currency and give that figure as it is. Their name is Nathan. They told Yap: \"I'm in Manchester, I play PC games, I work day shifts\""
        );
    }

    #[test]
    fn a_country_picked_in_settings_brings_its_currency_and_units() {
        let o = Overrides { country: "us".into(), ..Default::default() };
        let r = resolve(&uk(), &o);
        assert_eq!(r.country.as_deref(), Some("US"));
        assert_eq!((r.currency.as_deref(), r.symbol.as_deref()), (Some("USD"), Some("$")));
        assert_eq!(r.metric, Some(false));
        // The language and the clock still follow Windows.
        assert_eq!(r.language.as_deref(), Some("en-GB"));
        assert_eq!(r.time_zone_label.as_deref(), Some("UK time"));
        assert_eq!(summary(&r), "United States · English (UK) · $ USD · US units · UK time");
        // Picked too: they win.
        let o = Overrides { country: "US".into(), currency: "EUR".into(), units: "metric".into(), time_zone: "America/New_York".into(), language: "en-US".into() };
        let r = resolve(&uk(), &o);
        assert_eq!(summary(&r), "United States · English (US) · € EUR · metric · New York time");
        assert_eq!(r.spelling, Some(Spelling::American));
        // The same country as Windows: Windows' own currency settings stay.
        let r = resolve(&Detected { symbol: Some("GBP£".into()), ..uk() }, &Overrides { country: "GB".into(), ..Default::default() });
        assert_eq!(r.symbol.as_deref(), Some("GBP£"));
        // Junk is ignored.
        let r = resolve(&uk(), &Overrides { country: "ZZ".into(), time_zone: "Mars/Olympus".into(), ..Default::default() });
        assert_eq!(summary(&r), "United Kingdom · English (UK) · £ GBP · metric · UK time");
    }

    #[test]
    fn nothing_known_says_only_the_time() {
        let p = Profile::default();
        let line = prompt_block(&p, at());
        assert!(line.starts_with("About the user: it's Wednesday 21 October 2026, ") || line.starts_with("About the user: it's Thursday 22 October 2026, "), "{line}");
        assert!(!line.contains("prices"), "{line}");
        assert_eq!(summary(&p.region), "");
    }

    #[test]
    fn other_countries_read_right() {
        let fr = parse_stand_in("fr-FR");
        let r = resolve(&fr, &Overrides::default());
        assert_eq!(summary(&r), "France · French (France) · € EUR · metric");
        assert_eq!(r.spelling, None, "a French speaker's English isn't assumed British or American");
        let p = Profile { region: r, ..Default::default() };
        let line = prompt_block(&p, at());
        assert!(line.starts_with("About the user: they're in France (French); it's "), "{line}");
        assert!(line.contains("Give prices in € (EUR) and metric units;"), "{line}");
        // "the" where English says it.
        assert_eq!(in_sentence(country("NL").unwrap()), "the Netherlands");
        assert_eq!(in_sentence(country("DE").unwrap()), "Germany");
        assert_eq!(in_sentence(country("KY").unwrap()), "the Cayman Islands");
        // Canada spells its own way; Australia the British way.
        assert_eq!(resolve(&parse_stand_in("en-CA"), &Overrides::default()).spelling, Some(Spelling::Canadian));
        assert_eq!(resolve(&parse_stand_in("en-AU"), &Overrides::default()).spelling, Some(Spelling::British));
        let us = resolve(&parse_stand_in("en-US"), &Overrides::default());
        assert_eq!(us.spelling, Some(Spelling::American));
        assert_eq!(summary(&us), "United States · English (US) · $ USD · US units");
    }

    #[test]
    fn the_stand_in_reads_like_windows() {
        let d = parse_stand_in(r#"{"locale":"en-GB","country":"gb","currency":"GBP","symbol":"£","metric":true,"timeZone":"GMT Standard Time"}"#);
        assert_eq!(d, uk());
        assert_eq!(parse_stand_in(""), Detected::default());
        assert_eq!(iana_zone("Pacific Standard Time").as_deref(), Some("America/Los_Angeles"));
        assert_eq!(iana_zone("Europe/Paris").as_deref(), Some("Europe/Paris"));
        assert_eq!(iana_zone("Nowhere Standard Time"), None);
    }

    #[test]
    fn spelling_follows_the_region_and_the_setting() {
        let r = resolve(&uk(), &Overrides::default());
        assert_eq!(
            spelling_line(true, &r).as_deref(),
            Some("When the transcript is in English, spell it the British way (colour, organise, centre, travelled), without changing any words.")
        );
        assert_eq!(spelling_line(false, &r), None);
        assert_eq!(spelling_line(true, &Region::default()), None);
    }

    #[test]
    fn tavily_and_names() {
        assert_eq!(tavily_country("GB"), Some("united kingdom"));
        assert_eq!(tavily_country("cz"), Some("czech republic"));
        assert_eq!(tavily_country("CD"), Some("congo"));
        assert_eq!(tavily_country("HK"), None, "not one of Tavily's");
        assert_eq!(tag_name("en-GB"), "English (UK)");
        assert_eq!(tag_name("pt-BR"), "Portuguese (Brazil)");
        assert_eq!(tag_name("zh-Hans-CN"), "Chinese (China)");
        assert_eq!(zone_label("America/New_York"), "New York time");
        assert_eq!(zone_label("Etc/GMT+5"), "GMT+5");
        // Every table entry is well-formed and unique.
        let mut codes: Vec<&str> = COUNTRIES.iter().map(|c| c.code).collect();
        codes.sort_unstable();
        codes.dedup();
        assert_eq!(codes.len(), COUNTRIES.len());
        for c in COUNTRIES {
            assert!(c.code.len() == 2 && c.code.chars().all(|ch| ch.is_ascii_uppercase()), "{c:?}");
            assert!(c.currency.is_empty() || c.currency.len() == 3, "{c:?}");
        }
        for t in LANGUAGE_CHOICES {
            assert!(!tag_name(t).contains('-'), "{t} has no language name");
        }
        // Every Tavily name is one of the countries'.
        for t in TAVILY_COUNTRIES {
            assert!(COUNTRIES.iter().any(|c| tavily_country(c.code) == Some(*t)), "{t}");
        }
    }

    #[test]
    fn the_note_is_one_line_and_short() {
        assert_eq!(one_line("  I'm in\n\nManchester  ", 300), "I'm in Manchester");
        assert_eq!(one_line(&"a".repeat(400), ABOUT_MAX).chars().count(), ABOUT_MAX);
        let p = Profile { region: Region::default(), name: String::new(), about: "I say \"hi\"".into() };
        assert!(prompt_block(&p, at()).ends_with("They told Yap: \"I say 'hi'\""));
    }
}
