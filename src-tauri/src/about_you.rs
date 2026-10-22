//! **About you** (Settings → General → About you, 2026-10-21): what Yap
//! tells the AI models about the person, so an answer fits where they are
//! and who they are — a tester's ask: "It keeps finding things in dollars…"
//! (their Grok question on Qwen3.5-4B answered in US $ from a UK PC).
//!
//! - **The region: one country** (2026-10-22, the maintainer: "all of that should
//!   be covered in the region selector only"). Settings → About you →
//!   **Region** is "Automatic: United Kingdom (from Windows)" — Windows'
//!   home location ([`detected`]: `GetUserDefaultGeoName`, else
//!   `GetUserGeoID` → `GEO_ISO2`: "GB") — or a country picked from the list
//!   (`config.region_country`, empty = Automatic). **Everything else
//!   follows the country** ([`resolve`]): its currency (the [`COUNTRIES`]
//!   table; Windows' own currency settings while its regional format is for
//!   that country — `LOCALE_SINTLSYMBOL` "GBP", `LOCALE_SCURRENCY` "£"), its
//!   units (km, or miles in the UK, or US units; `LOCALE_IMEASURE` likewise),
//!   the language searches and the prompt use (Windows' language,
//!   `GetUserDefaultLocaleName`'s "en", with the country's variety: en-GB)
//!   and its English spelling (British, American or Canadian). The **time
//!   zone is always the PC's own** (`GetDynamicTimeZoneInformation`'s key
//!   name, "GMT Standard Time" → "Europe/London" through `calendar/tz.rs`'
//!   CLDR table), never a setting. Settings shows what follows from it in
//!   one line ([`summary`]: "Prices in £ · miles and °C · British spelling ·
//!   UK time").
//! - **What to call them** (`about_name`) and **anything the AI should
//!   know** (`about_me`, ≤ [`ABOUT_MAX`] characters).
//!
//! Every model gets it in one short block at the end of its system prompt
//! (Chat, the Voice Assistant, the Meetings and Notes Ask bars; local and
//! cloud) — without the clock ([`profile_block`]), which goes with the
//! question ([`clock_line`], `chats::with_context`) so the system prompt
//! stays the same from turn to turn; whole ([`prompt_block`]) in a prompt
//! made for one question (a small model's web answer) — web searches are
//! aimed at the country (Exa's
//! `userLocation`, Tavily's `country`, the providers' own searches, and a
//! price question's query names it: "… price UK" — see `web.rs`), and
//! dictation cleanup keeps the person's English spelling
//! ([`cleanup_spelling`]; its one switch, "Write in my region's spelling",
//! is in Language Models → Dictation cleanup).
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

/// Countries that measure in US customary units (miles, pounds, °F).
const IMPERIAL: &[&str] = &["US", "LR", "MM", "PR", "GU", "VI", "AS", "MP", "UM"];

/// Metric, but distances and speeds in miles: the UK and the Crown
/// Dependencies.
const MILES: &[&str] = &["GB", "IM", "JE", "GG"];

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
            Spelling::British => "honour, recognise, paralyse, metre, cancelled, catalogue, defence",
            Spelling::American => "honor, recognize, paralyze, meter, canceled, catalog, defense",
            Spelling::Canadian => "honour, recognize, paralyze, metre, cancelled, catalogue, defence",
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
/// Pure. (Test runs only: a release build has no stand-in.)
#[cfg(any(test, debug_assertions))]
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

// ---- the region: the country, and what follows from it ------------------------------------

/// How the person measures things.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Units {
    /// Kilometres, kilograms, °C.
    Metric,
    /// Metric, but distances and speeds in miles (the UK).
    MetricMiles,
    /// US customary units: miles, pounds, °F.
    Us,
}

impl Units {
    fn of(country: &str, metric: Option<bool>) -> Option<Units> {
        match metric {
            Some(false) => Some(Units::Us),
            Some(true) if MILES.contains(&country) => Some(Units::MetricMiles),
            Some(true) => Some(Units::Metric),
            None => None,
        }
    }

    /// Settings' summary: "km and °C", "miles and °C", "miles and °F".
    pub fn short(self) -> &'static str {
        match self {
            Units::Metric => "km and °C",
            Units::MetricMiles => "miles and °C",
            Units::Us => "miles and °F",
        }
    }

    /// The prompt's: "metric units", "metric units, but miles for distances
    /// and speeds", "US units (miles, pounds, °F)".
    fn in_prompt(self) -> &'static str {
        match self {
            Units::Metric => "metric units",
            Units::MetricMiles => "metric units, but miles for distances and speeds",
            Units::Us => "US units (miles, pounds, °F)",
        }
    }
}

/// Where the person is and what follows from it: their money, units,
/// language and spelling — and the PC's clock.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Region {
    pub country: Option<String>,
    pub country_name: Option<String>,
    /// Follows Windows' home location (Settings' "Automatic").
    pub automatic: bool,
    /// Windows' language with the country's variety ("en-GB").
    pub language: Option<String>,
    pub currency: Option<String>,
    pub symbol: Option<String>,
    pub metric: Option<bool>,
    pub units: Option<Units>,
    /// The PC's own time zone (IANA), never a setting.
    pub time_zone: Option<String>,
    pub time_zone_label: Option<String>,
    /// How English is spelled there (`None`: not an English speaker, or
    /// not known).
    pub spelling: Option<Spelling>,
}

/// **The region**: the country Settings picked (`picked`, ISO; empty or
/// unknown = Windows' home location, else the region of Windows' regional
/// format), and everything that follows from it — its currency and units
/// (Windows' own settings while Windows' regional format is for that
/// country, so a person's own changes to them stay; else the [`COUNTRIES`]
/// table's), Windows' language with the country's variety ("en" + "GB" →
/// "en-GB") and so its English spelling. The time zone is always the PC's.
/// Pure.
pub fn resolve(d: &Detected, picked: &str) -> Region {
    let picked = country(picked).map(|c| c.code.to_string());
    let (windows_lang, format_region) = d.locale.as_deref().map(split_tag).unwrap_or_default();
    let windows_country = d.country.clone().filter(|c| country(c).is_some());
    let automatic = picked.is_none() || picked == windows_country;
    let country_code = picked
        .or_else(|| windows_country.clone())
        .or_else(|| format_region.clone().filter(|c| country(c).is_some()));
    // Windows' currency and measurement are its regional format's: theirs
    // only while that format is for this country (en-GB for the UK).
    let windows_formats_here = country_code.is_some() && (format_region == country_code || (format_region.is_none() && automatic));
    let (currency_code, symbol) = if windows_formats_here && d.currency.is_some() {
        (d.currency.clone(), d.symbol.clone().or_else(|| d.currency.clone()))
    } else {
        let code = country_code.as_deref().and_then(country).map(|c| c.currency).filter(|c| !c.is_empty());
        (code.map(str::to_string), code.map(|c| currency(c).map_or(c, |k| k.symbol).to_string()))
    };
    let metric = if windows_formats_here && d.metric.is_some() {
        d.metric
    } else {
        country_code.as_deref().map(|c| !IMPERIAL.contains(&c))
    };
    let language = match (windows_lang.is_empty(), country_code.as_deref()) {
        (false, Some(c)) => Some(format!("{windows_lang}-{c}")),
        (false, None) => d.locale.clone(),
        (true, Some(c)) if ENGLISH_SPEAKING.contains(&c) => Some(format!("en-{c}")),
        (true, _) => None,
    };
    let spelling = match (language.as_deref().map(split_tag), country_code.as_deref()) {
        (Some((lang, _)), Some(c)) if lang == "en" => Some(spelling_for_region(c)),
        (Some((lang, Some(r))), None) if lang == "en" => Some(spelling_for_region(&r)),
        _ => None,
    };
    Region {
        country_name: country_code.as_deref().and_then(country).map(|c| c.name.to_string()),
        units: country_code.as_deref().and_then(|c| Units::of(c, metric)),
        country: country_code,
        automatic,
        language,
        currency: currency_code,
        symbol,
        metric,
        time_zone_label: d.time_zone.as_deref().map(zone_label),
        time_zone: d.time_zone.clone(),
        spelling,
    }
}

/// The money as Settings' summary names it: "£" for the pound, euro and
/// dollar, a shared sign with its code ("$ (AUD)", "kr (SEK)"), else the
/// sign or the code ("CHF", "₹"). Pure.
fn money_short(r: &Region) -> Option<String> {
    let code = r.currency.as_deref()?;
    let symbol = r.symbol.as_deref().filter(|s| !s.is_empty() && *s != code);
    Some(match symbol {
        Some(s) if ["GBP", "EUR", "USD"].contains(&code) => s.to_string(),
        Some(s) if CURRENCIES.iter().filter(|c| c.symbol == s).count() > 1 || s.chars().any(|c| c.is_ascii_alphabetic()) => {
            format!("{s} ({code})")
        }
        Some(s) => s.to_string(),
        None => code.to_string(),
    })
}

/// What follows from the region, as Settings shows it under the picker:
/// "Prices in £ · miles and °C · British spelling · UK time" — only the
/// parts known. Pure.
pub fn summary(r: &Region) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(m) = money_short(r) {
        parts.push(format!("Prices in {m}"));
    }
    if let Some(u) = r.units {
        parts.push(u.short().to_string());
    }
    if let Some(s) = r.spelling {
        parts.push(format!("{} spelling", s.name()));
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
        region: resolve(&detected(), &cfg.region_country),
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

/// **The whole block**: where the person is and what they speak, the date
/// and their time, how to give prices, times and units, their name and what
/// they said about themselves — only the parts that are known. `now`: the
/// moment it describes. What Settings shows, and what a prompt made for one
/// question ends with (a small model's web answer); a prompt kept from turn
/// to turn has [`profile_block`] and the question [`clock_line`]. Pure.
///
/// "About the user: they're in the United Kingdom (British English); it's
/// Wednesday 21 October 2026, 21:40 UK time (UTC+01:00). Give times in UK
/// time and metric units. Prices: in £ (GBP) when you have them in £; never
/// change a price's currency sign — a price in another currency stays as it
/// is, labelled, like "$15 a month (US price)".
/// Their name is Imogen. They told Yap: "I run at weekends.""
pub fn prompt_block(p: &Profile, now: DateTime<Utc>) -> String {
    let when = when(p, now);
    let mut out = String::from("About the user: ");
    out.push_str(&match where_and_language(p) {
        Some(w) => format!("{w}; {when}."),
        None => format!("{when}."),
    });
    out.push_str(&how_to_answer(p));
    out
}

/// The moment `now` in the person's zone: "it's Wednesday 21 October 2026,
/// 21:40 UK time (UTC+01:00)". Pure.
fn when(p: &Profile, now: DateTime<Utc>) -> String {
    let r = &p.region;
    let zone = r.time_zone.as_deref().and_then(|z| z.parse::<chrono_tz::Tz>().ok());
    match zone {
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
    }
}

/// "they're in the United Kingdom (British English)", when either is known.
/// Pure.
fn where_and_language(p: &Profile) -> Option<String> {
    let place = p.region.country.as_deref().and_then(country).map(in_sentence);
    match (place, language_in_prompt(&p.region)) {
        (Some(c), Some(l)) => Some(format!("they're in {c} ({l})")),
        (Some(c), None) => Some(format!("they're in {c}")),
        (None, Some(l)) => Some(format!("they speak {l}")),
        (None, None) => None,
    }
}

/// **[`prompt_block`] without the clock**, for a prompt that should stay
/// the same from one request to the next (Chat's system prompt, so a model
/// re-uses its cache; the clock goes in the question's context,
/// [`clock_line`]): "About the user: they're in the United Kingdom (British
/// English). Give times in UK time and metric units. Prices: …". Empty when
/// nothing is known. Pure.
pub fn profile_block(p: &Profile) -> String {
    let rest = how_to_answer(p);
    match where_and_language(p) {
        Some(w) => format!("About the user: {w}.{rest}"),
        None if !rest.is_empty() => format!("About the user:{rest}"),
        None => String::new(),
    }
}

/// The clock, for a question's context: "It's Wednesday 21 October 2026,
/// 21:40 UK time (UTC+01:00)." Pure.
pub fn clock_line(p: &Profile, now: DateTime<Utc>) -> String {
    let w = when(p, now);
    let mut chars = w.chars();
    let first = chars.next().map(|c| c.to_uppercase().collect::<String>()).unwrap_or_default();
    format!("{first}{}.", chars.as_str())
}

/// The rest of the block: times, units, prices, the name and the note —
/// each starting with a space. Pure.
fn how_to_answer(p: &Profile) -> String {
    let r = &p.region;
    let mut out = String::new();
    let mut give: Vec<String> = Vec::new();
    if let Some(l) = &r.time_zone_label {
        give.push(format!("times in {l}"));
    }
    if let Some(u) = r.units {
        give.push(u.in_prompt().to_string());
    }
    if !give.is_empty() {
        out.push_str(&format!(" Give {}.", give.join(" and ")));
    }
    // Prices: theirs when known — never a sign swapped. Told "Give prices
    // in £", Qwen3.5-4B wrote x.ai's US "$8/month" as "£8/month" 2 times in
    // 3; told only to label a foreign price, it still listed bare "$8/month"
    // 1 time in 3 (2026-10-21). An example in another currency than theirs.
    if let Some(c) = &r.currency {
        let money = match &r.symbol {
            Some(s) if s != c => format!("{s} ({c})"),
            _ => c.clone(),
        };
        let theirs = r.symbol.clone().filter(|s| s != c).unwrap_or_else(|| c.clone());
        let example = if c == "USD" { "£15 a month (UK price)" } else { "$15 a month (US price)" };
        out.push_str(&format!(
            " Prices: in {money} when you have them in {theirs}; never change a price's currency sign — a price in another currency stays as it is, labelled, like \"{example}\"."
        ));
    }
    if !p.name.is_empty() {
        out.push_str(&format!(" Their name is {}.", p.name));
    }
    if !p.about.is_empty() {
        out.push_str(&format!(" They told Yap: \"{}\"", p.about.replace('"', "'")));
    }
    out
}

/// Yap's note under an answer that gives prices in $ without saying whose
/// dollars, for someone whose money isn't the dollar: "*Prices in $ are as
/// the sources give them (most likely US dollars), not in £.*" — told to
/// label a foreign price, Qwen3.5-4B still listed Grok's US plans as bare
/// "$8/month" about 1 time in 4 (the model evaluations, 2026-10-21). `None`
/// when the answer has no $ price, already says which dollars ("US$8", "USD",
/// "US price", "American"…), or the person pays in a dollar. Pure.
pub fn foreign_price_note(reply: &str, r: &Region) -> Option<String> {
    static DOLLARS: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r"(?:^|[^A-Za-z])\$\s?\d").expect("static regex"));
    static SAID: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"(?i)\b(?:US|U\.S\.?|USD|CAD|AUD|NZD|HKD|SGD|dollars?|American|United States|America)\b|[A-Z]{1,3}\$")
            .expect("static regex")
    });
    let (code, symbol) = (r.currency.as_deref()?, r.symbol.as_deref().unwrap_or(""));
    if code == "USD" || symbol.contains('$') || !DOLLARS.is_match(reply) || SAID.is_match(reply) {
        return None;
    }
    let theirs = if symbol.is_empty() { code } else { symbol };
    Some(format!("*Prices in $ are as the sources give them (most likely US dollars), not in {theirs}.*"))
}

/// **Prices where the person is** (2026-10-22): a tester asked for Grok's
/// price from a UK PC; Yap searched "Grok subscription price 2026" (Exa's
/// `userLocation` was GB) and found only x.ai's US page and US price lists,
/// and the answer gave the US tiers and offered to "help finding the UK
/// pricing". With this, a price question's search names the country
/// (`web::local_query`), results with only another country's prices lead to
/// one more search for the local price, and then to one plain line saying
/// there are none (`web::Turn`), never an offer to look further
/// ([`strip_price_offer`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalPrices {
    /// The country, ISO ("GB").
    pub country: String,
    /// How a search and a sentence name it: "UK", "Germany".
    pub place: String,
    /// Its money: "GBP", "£" (the code when it has no sign of its own).
    pub currency: String,
    pub symbol: String,
    /// Whether a page's price can be told apart as theirs: not for someone
    /// who pays in a dollar (a page's bare "$" may be theirs or the US's).
    pub tells_apart: bool,
}

impl LocalPrices {
    /// For someone whose country Yap knows, outside the US (where a page's
    /// price already is theirs). Pure.
    pub fn for_region(r: &Region) -> Option<LocalPrices> {
        let code = r.country.as_deref()?;
        if ["US", "PR", "GU", "VI", "AS", "MP", "UM"].contains(&code) {
            return None;
        }
        let currency = r.currency.clone().unwrap_or_default();
        let symbol = r.symbol.clone().filter(|s| !s.trim().is_empty()).unwrap_or_else(|| currency.clone());
        Some(LocalPrices {
            country: code.to_string(),
            place: short_name(code),
            tells_apart: !currency.is_empty() && !symbol.contains('$') && !DOLLARS.contains(&currency.as_str()),
            currency,
            symbol,
        })
    }

    /// The sign a search for a price in their money adds: "£", "€", else
    /// the code ("SEK": "kr" alone finds anything). Pure.
    pub fn search_sign(&self) -> &str {
        if self.symbol.chars().any(|c| c.is_alphabetic()) || self.symbol.is_empty() {
            &self.currency
        } else {
            &self.symbol
        }
    }

    /// The sign the prompts write prices in: "£", or "SEK".
    pub fn sign(&self) -> &str {
        if self.symbol.is_empty() {
            &self.currency
        } else {
            &self.symbol
        }
    }

    /// Does `text` give a price in their money ("£12.99", "12,99 €", "GBP
    /// 12", "12 pounds")? Pure.
    pub fn local_in(&self, text: &str) -> bool {
        if self.currency.is_empty() {
            return false;
        }
        let mut alts = vec![format!(r"\b{}\s?\d", regex::escape(&self.currency)), format!(r"\d\s?{}\b", regex::escape(&self.currency))];
        if !self.symbol.is_empty() && self.symbol != self.currency {
            let s = regex::escape(&self.symbol);
            let edge = if self.symbol.chars().any(char::is_alphabetic) { r"\b" } else { "" };
            alts.push(format!(r"{edge}{s}\s?\d"));
            alts.push(format!(r"\d\s?{s}{edge}"));
        }
        match self.currency.as_str() {
            "GBP" => alts.push(r"\d\s?(?:pounds|pence)\b".into()),
            "EUR" => alts.push(r"\d\s?euros?\b".into()),
            _ => {}
        }
        regex::Regex::new(&format!("(?i){}", alts.join("|"))).is_ok_and(|re| re.is_match(text))
    }

    /// Does `text` give a price in another money — "$8", "US$8", "€9.99",
    /// "8 USD" — than theirs? Pure.
    pub fn foreign_in(&self, text: &str) -> bool {
        static SIGN: std::sync::LazyLock<regex::Regex> =
            std::sync::LazyLock::new(|| regex::Regex::new(r"([$€£¥₹₩₽₺])\s?\d|\d\s?([€£₹])").expect("static regex"));
        static CODE: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
            regex::Regex::new(r"\b(USD|EUR|GBP|CAD|AUD|NZD|JPY|CNY|INR|CHF|SEK|NOK|DKK|PLN)\s?\d|\d\s?(USD|EUR|GBP|CAD|AUD|NZD|JPY|CNY|INR|CHF|SEK|NOK|DKK|PLN)\b")
                .expect("static regex")
        });
        SIGN.captures_iter(text).any(|c| c.get(1).or(c.get(2)).is_some_and(|m| m.as_str() != self.symbol))
            || CODE.captures_iter(text).any(|c| c.get(1).or(c.get(2)).is_some_and(|m| m.as_str() != self.currency))
    }

    /// Does a search already say where, or in what money ("Grok price UK",
    /// "Netflix US price", "Spotify GBP")? Then Yap leaves it as the model
    /// wrote it. Pure.
    pub fn query_names_a_place(&self, query: &str) -> bool {
        let q = format!(" {} ", query.to_lowercase().replace(['.', ',', '?', '!', '(', ')', '"', '\''], " "));
        let has = |w: &str| q.contains(&format!(" {} ", w.to_lowercase()));
        let own: &[&str] = match self.country.as_str() {
            "GB" => &["uk", "u k", "britain", "british", "england", "scotland", "wales", "northern ireland"],
            "IE" => &["irish"],
            "AU" => &["australian", "aussie"],
            "CA" => &["canadian"],
            "NZ" => &["nz"],
            _ => &[],
        };
        const ELSEWHERE: &[&str] =
            &["usa", "u s", "u s a", "america", "american", "eu", "europe", "european", "worldwide", "global", "international", "uae"];
        // Country names that are everyday words too ("turkey", "jersey").
        const NOT_A_PLACE: &[&str] = &["Chad", "Jordan", "Georgia", "Turkey", "Jersey", "Guernsey", "Chile", "Guinea", "Mali", "Niger"];
        let us = query.split(|c: char| !c.is_alphanumeric()).any(|w| w == "US");
        us || own.iter().chain(ELSEWHERE).any(|w| has(w))
            || COUNTRIES.iter().any(|c| !NOT_A_PLACE.contains(&c.name) && has(c.name))
            || has(&self.place)
            || query.contains(['£', '$', '€', '¥', '₹'])
            || CURRENCIES.iter().any(|c| query.split(|ch: char| !ch.is_alphanumeric()).any(|w| w == c.code))
    }

    /// The words that name their money in an answer: "£", "gbp", "pounds".
    fn money_words(&self) -> Vec<String> {
        let mut w = vec![self.symbol.to_lowercase(), self.currency.to_lowercase()];
        match self.currency.as_str() {
            "GBP" => w.extend(["pounds", "sterling"].map(String::from)),
            "EUR" => w.push("euros".into()),
            _ => {}
        }
        w.retain(|s| !s.is_empty());
        w
    }
}

/// Dollar currencies (a page's bare "$" may be one of them).
const DOLLARS: &[&str] = &[
    "USD", "CAD", "AUD", "NZD", "HKD", "SGD", "TWD", "XCD", "BBD", "BSD", "BZD", "BMD", "KYD", "JMD", "TTD", "GYD", "SRD",
    "LRD", "NAD", "FJD", "SBD", "BND",
];

/// An answer's offer to look for prices where the person is — "Let me know
/// if you need help finding the UK pricing…", "Would you like me to look up
/// UK prices?" — taken out after a search, when Yap could have looked (and
/// did: a price question searches for the local price, and once more when
/// the first results had none): a tester's Grok answer ended so, 2026-10-22.
/// A sentence that offers (let me know, would you like, I can…), to look
/// (find, look, search, check, help…), for a price, where they are (the
/// UK, £, local, your country…) goes — never one that says something
/// wasn't found ("I couldn't find UK prices") — and a paragraph left empty
/// goes too. `None` when there's no such offer. Pure.
pub fn strip_price_offer(reply: &str, local: &LocalPrices) -> Option<String> {
    static OFFER: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"(?i)\b(?:let me know|would you like|do you want|want me to|i can\b|i could\b|i['’]d be happy|i['’]m happy|happy to|shall i|should i|feel free to ask|just ask)")
            .expect("static regex")
    });
    static ACT: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"(?i)\b(?:find|finding|look|looking|search|searching|check|checking|get|help|dig|track|pull)").expect("static regex")
    });
    static PRICE: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r"(?i)pric|\bcost|\brates?\b|\bplans?\b|\bdeals?\b|tariff").expect("static regex"));
    static NOT: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"(?i)\b(?:couldn['’]t|could not|can['’]t|cannot|didn['’]t|did not|unable|wasn['’]t|weren['’]t|isn['’]t|aren['’]t|no)\b").expect("static regex")
    });
    let mut local_words: Vec<String> = local.money_words();
    local_words.extend([local.place.to_lowercase(), "local".into(), "your country".into(), "your region".into(), "your currency".into(), "your area".into()]);
    if local.country == "GB" {
        local_words.push("british".into());
    }
    let is_offer = |s: &str| {
        let lower = s.to_lowercase();
        OFFER.is_match(s)
            && ACT.is_match(s)
            && PRICE.is_match(s)
            && !NOT.is_match(s)
            && local_words.iter().any(|w| {
                // A word, not part of one ("uk" in "ukulele"); a sign anywhere.
                if w.chars().any(char::is_alphanumeric) {
                    lower.split(|c: char| !c.is_alphanumeric()).any(|x| x == w.as_str()) || (w.contains(' ') && lower.contains(w.as_str()))
                } else {
                    lower.contains(w.as_str())
                }
            })
    };
    let mut changed = false;
    let paragraphs: Vec<String> = reply
        .split("\n\n")
        .filter_map(|para| {
            // Sentence by sentence (a full stop before a digit is a price's
            // decimal point, not an end): the rest of the paragraph stays.
            let mut sentences: Vec<String> = Vec::new();
            let mut current = String::new();
            let mut chars = para.chars().peekable();
            while let Some(c) = chars.next() {
                current.push(c);
                let next_is_digit = chars.peek().is_some_and(char::is_ascii_digit);
                if matches!(c, '.' | '!' | '?' | '\n') && !next_is_digit {
                    sentences.push(std::mem::take(&mut current));
                }
            }
            if !current.is_empty() {
                sentences.push(current);
            }
            let before = sentences.len();
            sentences.retain(|s| !is_offer(s));
            if sentences.len() == before {
                return Some(para.to_string());
            }
            changed = true;
            let kept = sentences.concat().trim().to_string();
            kept.chars().any(char::is_alphanumeric).then_some(kept)
        })
        .collect();
    changed.then(|| paragraphs.join("\n\n").trim_end().to_string())
}

/// **A price never passed off as theirs**: an answer's price in the
/// person's money ("£8") that the web gave this turn only in another
/// ("$8", never "£8") gets its page's sign back ("$8") — Qwen3.5-4B, told
/// about the UK, wrote x.ai's "$8/month" as "£8/month" about 1 time in 3 in
/// a chat whose earlier turns found UK pages (the model evaluations,
/// 2026-10-21/22). Not 0 (free is free), not an estimate said as one
/// ("about £6", "~£6", "≈ £6"), and only the £, € and $ signs. `pages`: the
/// web text the model was given (`web::Turn::texts`). Returns the answer
/// and the prices put back ("$8"); `None` when nothing changes. Pure.
pub fn restore_price_signs(reply: &str, pages: &[String], local: &LocalPrices) -> Option<(String, Vec<String>)> {
    static PRICE: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"(?i)(~|≈|\babout|\baround|\broughly|\bapprox\.?|\bapproximately|\bequivalent(?:\s+to)?|\best\.?)?(\s*)([£€$])(\s?)(\d[\d,]*(?:\.\d+)?)")
            .expect("static regex")
    });
    if !["£", "€"].contains(&local.symbol.as_str()) {
        return None;
    }
    let norm = |n: &str| {
        let n = n.replace(',', "");
        let n = n.strip_suffix(".00").unwrap_or(&n).to_string();
        n.strip_suffix(".0").unwrap_or(&n).to_string()
    };
    let given: Vec<(String, String)> =
        pages.iter().flat_map(|p| PRICE.captures_iter(p).map(|c| (c[3].to_string(), norm(&c[5]))).collect::<Vec<_>>()).collect();
    let mut out = String::with_capacity(reply.len());
    let mut last = 0;
    let mut restored = Vec::new();
    for c in PRICE.captures_iter(reply) {
        let sign = c.get(3).expect("group 3");
        let amount = norm(&c[5]);
        let estimate = c.get(1).is_some();
        if sign.as_str() != local.symbol || estimate || amount.parse::<f64>().map_or(true, |v| v <= 0.0) {
            continue;
        }
        if given.iter().any(|(s, a)| *s == local.symbol && *a == amount) {
            continue;
        }
        // The page's sign: the dollar first (the usual one), else another.
        let Some(theirs) = ["$", "€", "£"].into_iter().filter(|s| *s != local.symbol).find(|s| given.iter().any(|(gs, a)| gs == s && *a == amount)) else {
            continue;
        };
        out.push_str(&reply[last..sign.start()]);
        out.push_str(theirs);
        last = sign.end();
        let price = format!("{theirs}{}", &c[5]);
        if !restored.contains(&price) {
            restored.push(price);
        }
    }
    (last > 0).then(|| {
        out.push_str(&reply[last..]);
        (out, restored)
    })
}

/// Yap's note under an answer whose prices it gave back their page's sign
/// ([`restore_price_signs`]): "*$8 and $80 are as their pages give them, in
/// dollars: not UK prices.*" Pure.
pub fn restored_price_note(restored: &[String], local: &LocalPrices) -> String {
    let money = |p: &str| match p.chars().next() {
        Some('$') => "dollars",
        Some('€') => "euros",
        _ => "pounds",
    };
    let list = match restored {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    };
    let (is, gives) = if restored.len() == 1 { ("is", "its page gives it") } else { ("are", "their pages give them") };
    let currency = restored.first().map_or("dollars", |p| money(p));
    format!("*{list} {is} as {gives}, in {currency}: not {} prices.*", local.place)
}

/// Dictation cleanup's spelling line ("Write in my region's spelling", on by
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

/// `about_you_status {overrides?}` (About you, and the cleanup's spelling
/// switch): the region as Windows has it (Settings' "Automatic"), the
/// region with the country picked (`regionCountry` passed, else the saved
/// one), what follows from it in one line, the prompt block as it reads
/// now (with `aboutName`, `aboutMe`), the spelling, and the countries.
#[tauri::command]
pub fn about_you_status(overrides: Option<serde_json::Value>) -> serde_json::Value {
    let cfg = crate::config::load();
    let (mut picked, mut name, mut about, mut spelling_on) =
        (cfg.region_country.clone(), cfg.about_name.clone(), cfg.about_me.clone(), cfg.region_spelling);
    if let Some(v) = overrides {
        let s = |k: &str| v[k].as_str().map(str::to_string);
        picked = s("regionCountry").unwrap_or(picked);
        name = s("aboutName").unwrap_or(name);
        about = s("aboutMe").unwrap_or(about);
        spelling_on = v["regionSpelling"].as_bool().unwrap_or(spelling_on);
    }
    let d = detected();
    let windows = resolve(&d, "");
    let region = resolve(&d, &picked);
    let profile = Profile { region: region.clone(), name: one_line(&name, NAME_MAX), about: one_line(&about, ABOUT_MAX) };
    serde_json::json!({
        "windows": windows,
        "region": region,
        "summary": summary(&region),
        "prompt": prompt_block(&profile, Utc::now()),
        "spelling": region.spelling.map(Spelling::name),
        "spellingExamples": region.spelling.map(Spelling::examples),
        "spellingLine": spelling_line(spelling_on, &region),
        "aboutMax": ABOUT_MAX,
        "nameMax": NAME_MAX,
        "countries": COUNTRIES.iter().map(|c| serde_json::json!({ "code": c.code, "name": c.name })).collect::<Vec<_>>(),
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

    const UK_BLOCK: &str = "About the user: they're in the United Kingdom (British English). Give times in UK time and metric units, but miles for distances and speeds. Prices: in £ (GBP) when you have them in £; never change a price's currency sign — a price in another currency stays as it is, labelled, like \"$15 a month (US price)\".";

    #[test]
    fn a_uk_pc_reads_as_the_uk() {
        let r = resolve(&uk(), "");
        assert!(r.automatic);
        assert_eq!(r.country_name.as_deref(), Some("United Kingdom"));
        assert_eq!(summary(&r), "Prices in £ · miles and °C · British spelling · UK time");
        assert_eq!(r.spelling, Some(Spelling::British));
        assert_eq!(r.language.as_deref(), Some("en-GB"));
        let p = Profile { region: r, name: "Imogen".into(), about: "I'm a teacher in Bristol and I run at weekends".into() };
        assert_eq!(
            prompt_block(&p, at()),
            "About the user: they're in the United Kingdom (British English); it's Wednesday 21 October 2026, 21:40 UK time (UTC+01:00). Give times in UK time and metric units, but miles for distances and speeds. Prices: in £ (GBP) when you have them in £; never change a price's currency sign — a price in another currency stays as it is, labelled, like \"$15 a month (US price)\". Their name is Imogen. They told Yap: \"I'm a teacher in Bristol and I run at weekends\""
        );
    }

    #[test]
    fn chat_gets_the_block_without_the_clock_and_the_clock_on_its_own() {
        let p = Profile { region: resolve(&uk(), ""), name: "Imogen".into(), about: String::new() };
        // The same every minute (Chat's system prompt stays cacheable)…
        let block = profile_block(&p);
        assert_eq!(block, format!("{UK_BLOCK} Their name is Imogen."));
        assert!(!block.contains("2026") && !block.contains("21:40"));
        // …and the moment in the question's context.
        assert_eq!(clock_line(&p, at()), "It's Wednesday 21 October 2026, 21:40 UK time (UTC+01:00).");
        // The two say what the whole block says.
        assert!(prompt_block(&p, at()).starts_with("About the user: they're in the United Kingdom (British English); it's Wednesday"));
        // Nothing known: no block, only the clock (in the PC's own zone).
        let none = Profile::default();
        assert_eq!(profile_block(&none), "");
        assert!(clock_line(&none, at()).starts_with("It's Wednesday 21 October 2026, "));
        // Only a name.
        assert_eq!(profile_block(&Profile { name: "Sam".into(), ..Default::default() }), "About the user: Their name is Sam.");
    }

    #[test]
    fn everything_follows_the_country_picked() {
        // The US picked on a UK PC: its money, units, spelling and variety of
        // English; the clock stays the PC's.
        let r = resolve(&uk(), "us");
        assert!(!r.automatic);
        assert_eq!(r.country.as_deref(), Some("US"));
        assert_eq!((r.currency.as_deref(), r.symbol.as_deref()), (Some("USD"), Some("$")));
        assert_eq!((r.metric, r.units), (Some(false), Some(Units::Us)));
        assert_eq!(r.language.as_deref(), Some("en-US"));
        assert_eq!(r.spelling, Some(Spelling::American));
        assert_eq!(r.time_zone_label.as_deref(), Some("UK time"));
        assert_eq!(summary(&r), "Prices in $ · miles and °F · American spelling · UK time");
        // Germany for an English speaker: euros, km, British spelling (an
        // English speaker outside the Americas), still English.
        let r = resolve(&uk(), "DE");
        assert_eq!(summary(&r), "Prices in € · km and °C · British spelling · UK time");
        assert_eq!(r.language.as_deref(), Some("en-DE"));
        // Canada and Australia: their own dollars, said with the code.
        assert_eq!(summary(&resolve(&uk(), "CA")), "Prices in $ (CAD) · km and °C · Canadian spelling · UK time");
        assert_eq!(summary(&resolve(&uk(), "AU")), "Prices in $ (AUD) · km and °C · British spelling · UK time");
        // Windows' own country picked: the same as Automatic, Windows' own
        // currency settings kept.
        let mine = Detected { symbol: Some("GBP£".into()), ..uk() };
        let r = resolve(&mine, "GB");
        assert!(r.automatic);
        assert_eq!(r.symbol.as_deref(), Some("GBP£"));
        // Junk is Automatic.
        assert_eq!(resolve(&uk(), "ZZ"), resolve(&uk(), ""));
    }

    #[test]
    fn the_home_location_wins_over_the_regional_format() {
        // A UK PC set up with the US format (Windows' currency then says $):
        // the country is the UK, and so are the money, units and spelling.
        let d = Detected {
            locale: Some("en-US".into()),
            currency: Some("USD".into()),
            symbol: Some("$".into()),
            metric: Some(false),
            ..uk()
        };
        let r = resolve(&d, "");
        assert_eq!(summary(&r), "Prices in £ · miles and °C · British spelling · UK time");
        assert_eq!(r.language.as_deref(), Some("en-GB"));
        // No home location: the format's region.
        let d = Detected { country: None, ..uk() };
        assert_eq!(resolve(&d, "").country.as_deref(), Some("GB"));
        // A French speaker in the UK: French, no English spelling assumed.
        let d = Detected { locale: Some("fr-FR".into()), ..uk() };
        let r = resolve(&d, "");
        assert_eq!((r.language.as_deref(), r.spelling), (Some("fr-GB"), None));
        assert_eq!(summary(&r), "Prices in £ · miles and °C · UK time");
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
        let r = resolve(&fr, "");
        assert_eq!(summary(&r), "Prices in € · km and °C");
        assert_eq!(r.spelling, None, "a French speaker's English isn't assumed British or American");
        let p = Profile { region: r, ..Default::default() };
        let line = prompt_block(&p, at());
        assert!(line.starts_with("About the user: they're in France (French); it's "), "{line}");
        assert!(line.contains("Give metric units. Prices: in € (EUR) when you have them in €; never change"), "{line}");
        // "the" where English says it.
        assert_eq!(in_sentence(country("NL").unwrap()), "the Netherlands");
        assert_eq!(in_sentence(country("DE").unwrap()), "Germany");
        assert_eq!(in_sentence(country("KY").unwrap()), "the Cayman Islands");
        // Canada spells its own way; Australia the British way.
        assert_eq!(resolve(&parse_stand_in("en-CA"), "").spelling, Some(Spelling::Canadian));
        assert_eq!(resolve(&parse_stand_in("en-AU"), "").spelling, Some(Spelling::British));
        let us = resolve(&parse_stand_in("en-US"), "");
        assert_eq!(us.spelling, Some(Spelling::American));
        assert_eq!(summary(&us), "Prices in $ · miles and °F · American spelling");
        // Shared signs and letter signs carry the code.
        assert_eq!(summary(&resolve(&parse_stand_in("sv-SE"), "")), "Prices in kr (SEK) · km and °C");
        assert_eq!(summary(&resolve(&parse_stand_in("ja-JP"), "")), "Prices in ¥ (JPY) · km and °C");
        assert_eq!(summary(&resolve(&parse_stand_in("hi-IN"), "")), "Prices in ₹ · km and °C");
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
        let r = resolve(&uk(), "");
        assert_eq!(
            spelling_line(true, &r).as_deref(),
            Some("When the transcript is in English, spell it the British way (honour, recognise, paralyse, metre, cancelled, catalogue, defence), without changing any words.")
        );
        assert_eq!(spelling_line(false, &r), None);
        assert_eq!(spelling_line(true, &Region::default()), None);
        // The US picked: American.
        assert!(spelling_line(true, &resolve(&uk(), "US")).unwrap().contains("the American way (honor"));
    }

    #[test]
    fn tavily_and_names() {
        assert_eq!(tavily_country("GB"), Some("united kingdom"));
        assert_eq!(tavily_country("cz"), Some("czech republic"));
        assert_eq!(tavily_country("CD"), Some("congo"));
        assert_eq!(tavily_country("HK"), None, "not one of Tavily's");
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
        // Every Tavily name is one of the countries'.
        for t in TAVILY_COUNTRIES {
            assert!(COUNTRIES.iter().any(|c| tavily_country(c.code) == Some(*t)), "{t}");
        }
    }

    #[test]
    fn bare_dollar_prices_get_a_note_for_someone_who_pays_in_pounds() {
        let gb = resolve(&uk(), "");
        let bare = "Here are the current Grok subscription prices:\n\n- **Plus**: $8/month\n- **Premium**: $30/month";
        assert_eq!(
            foreign_price_note(bare, &gb).as_deref(),
            Some("*Prices in $ are as the sources give them (most likely US dollars), not in £.*")
        );
        // Already said whose dollars, or no $ price at all: nothing.
        for said in ["Plus is $8/month (US price).", "Plus: US$8", "Plus costs 8 USD", "These are US prices: Plus $8.", "Plus is £8.", "It's free."] {
            assert_eq!(foreign_price_note(said, &gb), None, "{said}");
        }
        // Someone who pays in dollars, or whose money Yap doesn't know.
        let us = resolve(&parse_stand_in("en-US"), "");
        let ca = resolve(&parse_stand_in("en-CA"), "");
        assert_eq!(foreign_price_note(bare, &us), None);
        assert_eq!(foreign_price_note(bare, &ca), None);
        assert_eq!(foreign_price_note(bare, &Region::default()), None);
    }

    #[test]
    fn local_prices_are_told_apart_from_foreign_ones() {
        let gb = LocalPrices::for_region(&resolve(&uk(), "")).unwrap();
        assert_eq!((gb.place.as_str(), gb.sign(), gb.search_sign(), gb.tells_apart), ("UK", "£", "£", true));
        for local in ["Premium Individual: £12.99/month", "12.99 GBP a month", "GBP 12.99", "costs 12 pounds"] {
            assert!(gb.local_in(local), "{local}");
        }
        for foreign in ["Plus: $8/month", "US$8", "€13.99 in Germany", "8 USD", "13,99 €"] {
            assert!(gb.foreign_in(foreign), "{foreign}");
            assert!(!gb.local_in(foreign), "{foreign}");
        }
        assert!(!gb.foreign_in("£12.99 a month, 256 GB"));
        // Sweden: "kr" is a word, so searches add the code.
        let se = LocalPrices::for_region(&resolve(&parse_stand_in("sv-SE"), "")).unwrap();
        assert_eq!((se.place.as_str(), se.search_sign()), ("Sweden", "SEK"));
        assert!(se.local_in("Spotify kostar 129 kr i månaden") && !se.local_in("krona"));
        // No local prices for the US; dollars can't be told apart.
        assert_eq!(LocalPrices::for_region(&resolve(&parse_stand_in("en-US"), "")), None);
        assert!(!LocalPrices::for_region(&resolve(&parse_stand_in("en-CA"), "")).unwrap().tells_apart);
        assert_eq!(LocalPrices::for_region(&Region::default()), None);
    }

    #[test]
    fn a_query_that_already_says_where_is_left_alone() {
        let gb = LocalPrices::for_region(&resolve(&uk(), "")).unwrap();
        for q in [
            "Grok price UK",
            "Spotify Premium price United Kingdom",
            "Netflix price in Britain",
            "Grok subscription US price",
            "iPhone 17 price USA",
            "Game Pass price GBP",
            "Netflix £ price",
            "Spotify price Germany",
            "Spotify price Europe",
        ] {
            assert!(gb.query_names_a_place(q), "{q}");
        }
        for q in ["Grok subscription price 2026", "iPhone 17 Pro 256GB price", "price for us", "Arsenal jersey price", "turkey crown price"] {
            assert!(!gb.query_names_a_place(q), "{q}");
        }
    }

    #[test]
    fn an_offer_to_find_local_prices_goes() {
        let gb = LocalPrices::for_region(&resolve(&uk(), "")).unwrap();
        // A tester's answer of 2026-10-22, its last paragraph.
        let reply = "- **Plus**: $8/month\n- **Premium**: $30/month\n\nNote: These are US prices. Let me know if you need help finding the UK pricing or more details about what each plan includes!";
        assert_eq!(
            strip_price_offer(reply, &gb).as_deref(),
            Some("- **Plus**: $8/month\n- **Premium**: $30/month\n\nNote: These are US prices.")
        );
        for offer in [
            "Plus is $8.\n\nWould you like me to look up UK prices?",
            "Plus is $8.\n\nI can search for the price in pounds if you'd like.",
            "Plus is $8.\n\nIf you’d like, I can check the local pricing for you.",
        ] {
            assert_eq!(strip_price_offer(offer, &gb).as_deref(), Some("Plus is $8."), "{offer}");
        }
        // Not offers: saying what wasn't found, advice, a general offer.
        for kept in [
            "I couldn't find UK prices for Grok. Here are the US ones: Plus $8.99/month.",
            "If you want the cheapest UK plan, Plus is £8.",
            "Would you like more details about any plan?",
            "Plus is £7.99 a month in the UK.",
        ] {
            assert_eq!(strip_price_offer(kept, &gb), None, "{kept}");
        }
    }

    #[test]
    fn a_dollar_price_is_never_passed_off_as_pounds() {
        let gb = LocalPrices::for_region(&resolve(&uk(), "")).unwrap();
        let pages = ["Plus: $8/month or $80/year. Premium: $30.00 a month. Free $0.".to_string(), "SuperGrok ≈ £24/mo in the UK".to_string()];
        // The 4B's turn of 2026-10-22: x.ai's $8 written as £8.
        let (fixed, restored) = restore_price_signs("**Plus** - £8/month (£80/year), **Premium** £30.", &pages, &gb).unwrap();
        assert_eq!(fixed, "**Plus** - $8/month ($80/year), **Premium** $30.");
        assert_eq!(restored, ["$8", "$80", "$30"]);
        assert_eq!(restored_price_note(&restored, &gb), "*$8, $80 and $30 are as their pages give them, in dollars: not UK prices.*");
        assert_eq!(restored_price_note(&["$8".into()], &gb), "*$8 is as its page gives it, in dollars: not UK prices.*");
        // A £ price a page gives, free, an estimate said as one, a price no
        // page has, and dollars left as dollars: nothing changes.
        for kept in ["SuperGrok is £24 a month.", "Free (£0)", "about £6, ~£6, ≈ £6.50", "Plus costs £9.", "Plus is $8 (US price)."] {
            assert_eq!(restore_price_signs(kept, &pages, &gb), None, "{kept}");
        }
        // Someone who pays in dollars: never.
        let ca = LocalPrices::for_region(&resolve(&parse_stand_in("en-CA"), "")).unwrap();
        assert_eq!(restore_price_signs("$8", &pages, &ca), None);
    }

    #[test]
    fn the_note_is_one_line_and_short() {
        assert_eq!(one_line("  I'm in\n\nBristol  ", 300), "I'm in Bristol");
        assert_eq!(one_line(&"a".repeat(400), ABOUT_MAX).chars().count(), ABOUT_MAX);
        let p = Profile { region: Region::default(), name: String::new(), about: "I say \"hi\"".into() };
        assert!(prompt_block(&p, at()).ends_with("They told Yap: \"I say 'hi'\""));
    }
}
