//! **The weather: Open-Meteo** (`api.open-meteo.com` and its own geocoder,
//! `geocoding-api.open-meteo.com`; keyless). Its terms (open-meteo.com/en/
//! terms, read 1 Nov 2026): "You may only use the free API services for
//! non-commercial purposes", with "Less than 10'000 API calls per day, 5'000
//! per hour and 600 per minute"; non-commercial means apps "without
//! subscriptions or ads" (a tip jar is fine) — Yap is free and open source,
//! so it qualifies; **a paid Yap tier would need Open-Meteo's commercial
//! plan** (yap-private `docs/web-search-without-keys-2026-10.md` §5 Q4).
//! "The data obtained through the API is provided under the terms of the
//! CC-BY 4.0 licence": each result says "Weather data by Open-Meteo.com (CC
//! BY 4.0)", and its source links to Open-Meteo.
//!
//! The place comes from the words ([`place_in`]: "weather in Glenbrackie
//! today" → "Glenbrackie"), found by the geocoder in the person's language
//! (a place in their country first: "Perth" from the UK is Scotland's); the
//! forecast — now and the next seven days — in their units (°F, mph and
//! inches in the US; mph in the UK; °C, km/h and mm elsewhere; a question
//! naming the other unit gets it). No place named: the model asks which.

use std::sync::LazyLock;
use std::time::Duration;

use regex::Regex;
use reqwest::Url;
use serde_json::Value;

use crate::ai::about_you::Units;
use crate::privacy::net::FreeSource;

use super::super::{tidy, Endpoints, Hit, Settings};
use super::polite::{self, Ask, Rules};
use super::Out;

pub const GEOCODER: &str = "https://geocoding-api.open-meteo.com/v1/search";
pub const FORECAST: &str = "https://api.open-meteo.com/v1/forecast";
/// Well inside 600 a minute; a forecast kept 15 minutes, a place a day.
const FORECAST_RULES: Rules = Rules { gap: Duration::from_millis(100), per_minute: Some(60), keep: Duration::from_secs(15 * 60) };
const GEOCODER_RULES: Rules = Rules { gap: Duration::from_millis(100), per_minute: Some(60), keep: Duration::from_secs(24 * 3600) };
/// The credit CC BY 4.0 asks for.
pub const CREDIT: &str = "Weather data by Open-Meteo.com (CC BY 4.0).";

/// The units a forecast is asked in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WeatherUnits {
    pub fahrenheit: bool,
    pub mph: bool,
    pub inches: bool,
}

/// The person's units (About you), or the one the question names. Pure.
pub fn units_for(units: Option<Units>, question: &str) -> WeatherUnits {
    let q = question.to_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| q.contains(w));
    let says_f = has(&["fahrenheit", "°f"]);
    let says_c = has(&["celsius", "centigrade", "°c"]);
    let says_kmh = has(&["km/h", "kph", "kilometres per hour", "kilometers per hour", "m/s"]);
    let says_mph = has(&["mph", "miles per hour", "miles an hour"]);
    let us = units == Some(Units::Us);
    WeatherUnits {
        fahrenheit: says_f || (us && !says_c),
        mph: says_mph || (matches!(units, Some(Units::Us | Units::MetricMiles)) && !says_kmh),
        inches: us,
    }
}

/// Words around a place in a weather question, in the person's languages
/// (English always): left off its ends.
fn stop_words(langs: &[String]) -> Vec<&'static str> {
    const EN: &[&str] = &[
        "weather", "forecast", "forecasts", "temperature", "temperatures", "degrees", "rain", "raining", "rainy", "snow",
        "snowing", "sunny", "sun", "wind", "windy", "cold", "hot", "warm", "wet", "dry", "frost", "frosty", "icy",
        "stormy", "storm", "humid", "humidity", "umbrella", "coat", "sunscreen", "need", "do", "i", "a", "an", "the",
        "what", "what's", "whats", "how", "how's", "is", "it", "it's", "will", "be", "going", "gonna", "to", "like",
        "outside", "there", "current", "currently", "conditions", "now", "right", "today", "tonight", "tomorrow",
        "this", "next", "week", "weekend", "morning", "afternoon", "evening", "later", "monday", "tuesday",
        "wednesday", "thursday", "friday", "saturday", "sunday", "on", "in", "at", "for", "near", "around", "me",
        "please", "tell", "check", "look", "up", "celsius", "fahrenheit", "mph", "and", "or", "chance", "of",
        "expected", "the", "days", "few", "coming", "local", "report", "update", "time", "hourly", "daily",
    ];
    const OTHER: &[(&str, &[&str])] = &[
        ("fr", &["météo", "meteo", "prévisions", "temps", "quel", "fait-il", "fait", "il", "va-t-il", "faire", "pleuvoir", "neiger", "aujourd'hui", "demain", "ce", "soir", "maintenant", "semaine", "week-end", "le", "la", "les", "à", "au", "aux", "en", "pour", "de", "du", "des", "qu'il"]),
        ("de", &["wetter", "wettervorhersage", "vorhersage", "wie", "wird", "das", "ist", "es", "regnen", "schneien", "heute", "morgen", "jetzt", "abend", "wochenende", "woche", "in", "bei", "für", "der", "die", "am"]),
        ("es", &["tiempo", "pronóstico", "pronostico", "clima", "qué", "que", "hace", "hará", "va", "a", "llover", "nevar", "hoy", "mañana", "ahora", "esta", "noche", "este", "fin", "de", "semana", "en", "el", "la", "para"]),
        ("it", &["meteo", "previsioni", "tempo", "che", "fa", "farà", "pioverà", "nevicherà", "oggi", "domani", "stasera", "adesso", "ora", "questo", "fine", "settimana", "a", "in", "per", "il", "la", "del", "della"]),
        ("pt", &["previsão", "previsao", "tempo", "como", "está", "vai", "chover", "nevar", "hoje", "amanhã", "agora", "esta", "noite", "este", "fim", "de", "semana", "em", "no", "na", "o", "a", "para"]),
        ("nl", &["weer", "weerbericht", "weersverwachting", "hoe", "wordt", "het", "gaat", "regenen", "sneeuwen", "vandaag", "morgen", "nu", "vanavond", "dit", "weekend", "in", "te", "voor", "bij", "de"]),
    ];
    let mut words = EN.to_vec();
    for (code, list) in OTHER {
        if langs.iter().any(|l| l.to_ascii_lowercase().starts_with(code)) {
            words.extend_from_slice(list);
        }
    }
    words
}

/// The words before a place, in the person's languages ("in", "à", "bei"…).
fn prepositions(langs: &[String]) -> Regex {
    let mut words = vec!["in", "at", "for", "near", "around"];
    let more: &[(&str, &[&str])] = &[
        ("fr", &["à", "au", "aux", "en", "pour"]),
        ("de", &["in", "bei", "für"]),
        ("es", &["en", "para"]),
        ("it", &["a", "in", "per"]),
        ("pt", &["em", "no", "na", "para"]),
        ("nl", &["in", "te", "voor", "bij"]),
    ];
    for (code, list) in more {
        if langs.iter().any(|l| l.to_ascii_lowercase().starts_with(code)) {
            words.extend_from_slice(list);
        }
    }
    Regex::new(&format!(r"(?i)(?:^|\s)(?:{})\s+(.+)$", words.join("|"))).expect("escaped words")
}

/// **The place a weather question names** ("What's the weather like in
/// Glenbrackie today?", "weather Glenbrackie tomorrow" → "Glenbrackie";
/// "Will it rain in Stoke on Trent this weekend?" → "Stoke on Trent"): the
/// words after the first "in / at / for…", else all of them, with weather
/// and time words left off both ends (never from the middle). `None` when
/// nothing's left. Pure.
pub fn place_in(text: &str, langs: &[String]) -> Option<String> {
    static PUNCT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"[?!.;:"“”¿¡()]+"#).expect("static regex"));
    let text = PUNCT.replace_all(&text.replace(['\u{2019}', '\u{2018}'], "'"), " ").into_owned();
    let stops = stop_words(langs);
    let trim = |phrase: &str| -> Option<String> {
        let mut words: Vec<&str> = phrase.split_whitespace().collect();
        let is_stop = |x: &str| stops.contains(&x);
        let stop = |w: &str| {
            let w = w.trim_matches(|c: char| c == ',' || c == '\'').to_lowercase();
            w.is_empty() || is_stop(&w) || (w.ends_with("'s") && is_stop(w.trim_end_matches("'s")))
        };
        while words.first().is_some_and(|w| stop(w)) {
            words.remove(0);
        }
        while words.last().is_some_and(|w| stop(w)) {
            words.pop();
        }
        let place = words.join(" ").trim_matches(|c: char| c == ',' || c.is_whitespace()).to_string();
        (place.chars().filter(|c| c.is_alphabetic()).count() >= 2).then_some(place)
    };
    if let Some(c) = prepositions(langs).captures(&text) {
        if let Some(p) = trim(&c[1]) {
            return Some(p);
        }
    }
    trim(&text)
}

/// The geocoder's request: the place's name (before a comma), five
/// candidates, names in the person's language. Pure.
pub fn geocode_url(name: &str, lang: &str) -> Url {
    let mut url = Url::parse(GEOCODER).expect("static address");
    url.query_pairs_mut().append_pair("name", name).append_pair("count", "5").append_pair("language", lang).append_pair("format", "json");
    url
}

/// A place the geocoder found.
#[derive(Debug, Clone, PartialEq)]
pub struct Spot {
    pub name: String,
    pub admin1: String,
    pub country: String,
    pub country_code: String,
    pub lat: f64,
    pub lon: f64,
}

impl Spot {
    /// "Glenbrackie, Fellshire, United Kingdom".
    pub fn label(&self) -> String {
        [self.name.as_str(), self.admin1.as_str(), self.country.as_str()]
            .iter()
            .filter(|s| !s.is_empty())
            .fold(Vec::<&str>::new(), |mut v, s| {
                if !v.contains(s) {
                    v.push(s);
                }
                v
            })
            .join(", ")
    }
}

/// The geocoder's places (`results[]`; none: no `results`). Pure.
pub fn parse_spots(v: &Value) -> Vec<Spot> {
    v["results"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|r| {
            Some(Spot {
                name: r["name"].as_str()?.to_string(),
                admin1: r["admin1"].as_str().unwrap_or("").to_string(),
                country: r["country"].as_str().unwrap_or("").to_string(),
                country_code: r["country_code"].as_str().unwrap_or("").to_string(),
                lat: r["latitude"].as_f64()?,
                lon: r["longitude"].as_f64()?,
            })
        })
        .collect()
}

/// The place meant: one matching what follows a comma ("Paris, Texas"),
/// else one in the person's country, else the geocoder's first (the
/// biggest). Pure.
pub fn pick<'a>(spots: &'a [Spot], hint: Option<&str>, country: Option<&str>) -> Option<&'a Spot> {
    let hint = hint.map(|h| h.trim().to_lowercase()).filter(|h| !h.is_empty());
    if let Some(h) = &hint {
        let matches = |s: &&Spot| [&s.country, &s.admin1, &s.country_code].iter().any(|f| !f.is_empty() && (f.to_lowercase().contains(h.as_str()) || h.contains(&f.to_lowercase())));
        if let Some(s) = spots.iter().find(matches) {
            return Some(s);
        }
    }
    if let Some(c) = country {
        if let Some(s) = spots.iter().find(|s| s.country_code.eq_ignore_ascii_case(c)) {
            return Some(s);
        }
    }
    spots.first()
}

/// The forecast's request: now and seven days, at the place (to two
/// decimals: ~1 km), in `units`, in the place's own time. Pure.
pub fn forecast_url(lat: f64, lon: f64, units: WeatherUnits) -> Url {
    let mut url = Url::parse(FORECAST).expect("static address");
    {
        let mut q = url.query_pairs_mut();
        q.append_pair("latitude", &format!("{lat:.2}"))
            .append_pair("longitude", &format!("{lon:.2}"))
            .append_pair("current", "temperature_2m,apparent_temperature,relative_humidity_2m,precipitation,weather_code,wind_speed_10m")
            .append_pair("daily", "weather_code,temperature_2m_max,temperature_2m_min,precipitation_probability_max,precipitation_sum,wind_speed_10m_max")
            .append_pair("timezone", "auto")
            .append_pair("forecast_days", "7");
        if units.fahrenheit {
            q.append_pair("temperature_unit", "fahrenheit");
        }
        q.append_pair("wind_speed_unit", if units.mph { "mph" } else { "kmh" });
        if units.inches {
            q.append_pair("precipitation_unit", "inch");
        }
    }
    url
}

/// A WMO weather code in words (Open-Meteo's documented table). Pure.
pub fn sky(code: i64) -> &'static str {
    match code {
        0 => "clear sky",
        1 => "mainly clear",
        2 => "partly cloudy",
        3 => "overcast",
        45 | 48 => "fog",
        51 | 53 | 55 => "drizzle",
        56 | 57 => "freezing drizzle",
        61 => "light rain",
        63 => "rain",
        65 => "heavy rain",
        66 | 67 => "freezing rain",
        71 => "light snow",
        73 => "snow",
        75 => "heavy snow",
        77 => "snow grains",
        80 => "light showers",
        81 => "showers",
        82 => "heavy showers",
        85 | 86 => "snow showers",
        95 => "thunderstorms",
        96 | 99 => "thunderstorms with hail",
        _ => "mixed weather",
    }
}

/// A number as a forecast says it: whole degrees and speeds, one decimal
/// for rain. Pure.
fn whole(v: &Value) -> Option<String> {
    v.as_f64().map(|n| format!("{}", n.round() as i64))
}

/// **The forecast as a result**: now, then each day ("Today (Sat 1 Nov):
/// partly cloudy, 8 to 13°C, rain 20% (0.4 mm), wind up to 15 mph"), the
/// credit last; its source Open-Meteo's page for the place. Pure.
pub fn parse_forecast(v: &Value, spot: &Spot) -> Option<Hit> {
    let unit = |group: &str, key: &str, or: &str| v[group][key].as_str().map_or(or.to_string(), |u| u.replace("mp/h", "mph").replace("inch", "in"));
    let (t, speed, rain) = (unit("current_units", "temperature_2m", "°C"), unit("current_units", "wind_speed_10m", "km/h"), unit("daily_units", "precipitation_sum", "mm"));
    let now = &v["current"];
    let mut parts = Vec::new();
    if let Some(temp) = whole(&now["temperature_2m"]) {
        let time = now["time"].as_str().and_then(|s| s.split('T').nth(1)).unwrap_or("");
        let mut line = format!("Now ({time} local time): {temp}{t}");
        if let Some(feels) = whole(&now["apparent_temperature"]) {
            line.push_str(&format!(", feels like {feels}{t}"));
        }
        if let Some(code) = now["weather_code"].as_i64() {
            line.push_str(&format!(", {}", sky(code)));
        }
        if let Some(w) = whole(&now["wind_speed_10m"]) {
            line.push_str(&format!(", wind {w} {speed}"));
        }
        if let Some(h) = whole(&now["relative_humidity_2m"]) {
            line.push_str(&format!(", humidity {h}%"));
        }
        parts.push(format!("{line}."));
    }
    let d = &v["daily"];
    let days = d["time"].as_array().cloned().unwrap_or_default();
    for (i, day) in days.iter().enumerate() {
        let Some(date) = day.as_str().and_then(|s| chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()) else { continue };
        let at = |key: &str| d[key].get(i).cloned().unwrap_or(Value::Null);
        let name = date.format("%a %-d %b").to_string();
        let name = match i {
            0 => format!("Today ({name})"),
            1 => format!("Tomorrow ({name})"),
            _ => name,
        };
        let mut line = name;
        line.push(':');
        if let Some(code) = at("weather_code").as_i64() {
            line.push_str(&format!(" {},", sky(code)));
        }
        if let (Some(lo), Some(hi)) = (whole(&at("temperature_2m_min")), whole(&at("temperature_2m_max"))) {
            line.push_str(&format!(" {lo} to {hi}{t},"));
        }
        if let Some(p) = whole(&at("precipitation_probability_max")) {
            let amount = at("precipitation_sum").as_f64().filter(|a| *a > 0.0).map(|a| format!(" ({a:.1} {rain})")).unwrap_or_default();
            line.push_str(&format!(" rain {p}%{amount},"));
        }
        if let Some(w) = whole(&at("wind_speed_10m_max")) {
            line.push_str(&format!(" wind up to {w} {speed}"));
        }
        parts.push(format!("{}.", line.trim_end_matches(',')));
    }
    if parts.is_empty() {
        return None;
    }
    parts.push(CREDIT.to_string());
    let published = days.first().and_then(Value::as_str).map(str::to_string);
    Some(Hit {
        title: format!("Weather in {}", spot.label()),
        url: format!("https://open-meteo.com/en/docs?latitude={:.2}&longitude={:.2}", spot.lat, spot.lon),
        published,
        text: tidy(&parts.join(" "), 1_100),
    })
}

/// **The weather for a search**: the place in the model's query (else the
/// person's question), found by the geocoder, its forecast. No place named:
/// `needs_place`.
pub(super) async fn look_up(s: &Settings, ep: &Endpoints, query: &str, context: &str, langs: &[String]) -> Result<Out, String> {
    let Some(place) = place_in(query, langs).or_else(|| place_in(context, langs)) else {
        return Ok(Out { needs_place: true, ..Default::default() });
    };
    let (name, hint) = match place.split_once(',') {
        Some((n, h)) => (n.trim().to_string(), Some(h.trim().to_string())),
        None => (place.clone(), None),
    };
    let geo = |name: &str| geocode_url(name, &s.lang);
    let ask = |rules| Ask { purpose: FreeSource::Weather, rules, accept: "application/json", headers: &[] };
    let read = |body: String| serde_json::from_str::<Value>(&body).map_err(|_| "Open-Meteo sent an answer Yap couldn't read.".to_string());
    let mut spots = parse_spots(&read(polite::get(ep, &geo(&name), ask(GEOCODER_RULES)).await?)?);
    // "Glenbrackie Scotland" (no comma): the first word alone, the rest as
    // the hint.
    let mut hint = hint;
    if spots.is_empty() {
        if let Some((first, rest)) = name.split_once(' ') {
            spots = parse_spots(&read(polite::get(ep, &geo(first), ask(GEOCODER_RULES)).await?)?);
            hint = hint.or(Some(rest.to_string()));
        }
    }
    let Some(spot) = pick(&spots, hint.as_deref(), s.country.as_deref()) else {
        tracing::info!("Web search: Open-Meteo's geocoder found no such place");
        return Ok(Out { from: "Open-Meteo".into(), ..Default::default() });
    };
    let units = units_for(s.units, &format!("{query} {context}"));
    let v = read(polite::get(ep, &forecast_url(spot.lat, spot.lon, units), ask(FORECAST_RULES)).await?)?;
    if let Some(reason) = v["reason"].as_str().filter(|_| v["error"] == Value::Bool(true)) {
        return Err(format!("Open-Meteo didn't take the request: {}", tidy(reason, 120)));
    }
    Ok(Out { hits: parse_forecast(&v, spot).into_iter().collect(), from: "Open-Meteo".into(), ..Default::default() })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn en() -> Vec<String> {
        vec!["en".into()]
    }

    #[test]
    fn the_place_is_read_from_the_words() {
        let place = |t: &str| place_in(t, &en());
        assert_eq!(place("What's the weather like in Glenbrackie today?").as_deref(), Some("Glenbrackie"));
        assert_eq!(place("weather Glenbrackie tomorrow").as_deref(), Some("Glenbrackie"));
        assert_eq!(place("Glenbrackie weather this weekend").as_deref(), Some("Glenbrackie"));
        assert_eq!(place("Will it rain in Stoke on Trent this weekend?").as_deref(), Some("Stoke on Trent"));
        assert_eq!(place("Will it rain tomorrow in Port Ellis?").as_deref(), Some("Port Ellis"));
        assert_eq!(place("forecast for Paris, France on Saturday").as_deref(), Some("Paris, France"));
        assert_eq!(place("rain at the weekend in Kellbrook").as_deref(), Some("Kellbrook"));
        assert_eq!(place("Is it going to be cold tonight in Fellshire?").as_deref(), Some("Fellshire"));
        assert_eq!(place("What's the weather like today?"), None);
        assert_eq!(place("Do I need an umbrella?"), None);
        // In the person's language.
        let fr = vec!["fr".to_string(), "en".to_string()];
        assert_eq!(place_in("Quel temps fait-il à Port Ellis demain ?", &fr).as_deref(), Some("Port Ellis"));
        assert_eq!(place_in("météo Glenbrackie aujourd'hui", &fr).as_deref(), Some("Glenbrackie"));
        let de = vec!["de".to_string(), "en".to_string()];
        assert_eq!(place_in("Wie wird das Wetter morgen in Kellbrook?", &de).as_deref(), Some("Kellbrook"));
    }

    #[test]
    fn the_units_are_the_persons_or_the_ones_asked_for() {
        assert_eq!(units_for(Some(Units::Us), "weather"), WeatherUnits { fahrenheit: true, mph: true, inches: true });
        assert_eq!(units_for(Some(Units::MetricMiles), "weather"), WeatherUnits { fahrenheit: false, mph: true, inches: false });
        assert_eq!(units_for(Some(Units::Metric), "weather"), WeatherUnits::default());
        assert_eq!(units_for(None, "weather"), WeatherUnits::default());
        assert!(units_for(Some(Units::MetricMiles), "weather in Fahrenheit").fahrenheit);
        assert!(!units_for(Some(Units::Us), "weather in celsius").fahrenheit);
        assert!(!units_for(Some(Units::MetricMiles), "wind in km/h").mph);
        let url = forecast_url(56.4907, -3.1872, units_for(Some(Units::MetricMiles), "")).to_string();
        assert!(url.starts_with("https://api.open-meteo.com/v1/forecast?latitude=56.49&longitude=-3.19&current=temperature_2m"), "{url}");
        assert!(url.contains("&timezone=auto&forecast_days=7&wind_speed_unit=mph"), "{url}");
        assert!(!url.contains("temperature_unit") && !url.contains("precipitation_unit"), "{url}");
        let us = forecast_url(40.0, -75.0, units_for(Some(Units::Us), "")).to_string();
        assert!(us.contains("temperature_unit=fahrenheit") && us.contains("precipitation_unit=inch"), "{us}");
    }

    // Recorded from Open-Meteo's documented shapes (open-meteo.com/en/docs,
    // /en/docs/geocoding-api), a made-up place.
    fn spots() -> Value {
        json!({ "results": [
            { "id": 1, "name": "Glenbrackie", "latitude": 39.12, "longitude": -84.5, "country_code": "US", "country": "United States", "admin1": "Ohio", "population": 90000 },
            { "id": 2, "name": "Glenbrackie", "latitude": 56.4907, "longitude": -3.1872, "country_code": "GB", "country": "United Kingdom", "admin1": "Scotland", "population": 2000 }
        ], "generationtime_ms": 0.6 })
    }

    #[test]
    fn the_place_meant_is_picked() {
        let s = parse_spots(&spots());
        assert_eq!(s.len(), 2);
        // The person's country first; else the biggest; a hint wins.
        assert_eq!(pick(&s, None, Some("GB")).unwrap().admin1, "Scotland");
        assert_eq!(pick(&s, None, Some("FR")).unwrap().admin1, "Ohio");
        assert_eq!(pick(&s, Some("Ohio"), Some("GB")).unwrap().admin1, "Ohio");
        assert_eq!(pick(&s, Some("Scotland"), None).unwrap().country_code, "GB");
        assert_eq!(s[1].label(), "Glenbrackie, Scotland, United Kingdom");
        assert!(parse_spots(&json!({ "generationtime_ms": 0.3 })).is_empty());
        let url = geocode_url("Glenbrackie", "fr").to_string();
        assert_eq!(url, "https://geocoding-api.open-meteo.com/v1/search?name=Glenbrackie&count=5&language=fr&format=json");
    }

    fn forecast() -> Value {
        json!({
            "latitude": 56.49, "longitude": -3.19, "timezone": "Europe/London",
            "current_units": { "time": "iso8601", "temperature_2m": "°C", "apparent_temperature": "°C", "relative_humidity_2m": "%", "precipitation": "mm", "weather_code": "wmo code", "wind_speed_10m": "mp/h" },
            "current": { "time": "2026-11-01T14:15", "interval": 900, "temperature_2m": 11.6, "apparent_temperature": 9.2, "relative_humidity_2m": 71, "precipitation": 0.0, "weather_code": 2, "wind_speed_10m": 9.4 },
            "daily_units": { "time": "iso8601", "weather_code": "wmo code", "temperature_2m_max": "°C", "temperature_2m_min": "°C", "precipitation_probability_max": "%", "precipitation_sum": "mm", "wind_speed_10m_max": "mp/h" },
            "daily": {
                "time": ["2026-11-01", "2026-11-02", "2026-11-03"],
                "weather_code": [2, 63, 3],
                "temperature_2m_max": [13.1, 10.6, 9.8],
                "temperature_2m_min": [7.8, 6.2, 4.4],
                "precipitation_probability_max": [20, 85, 10],
                "precipitation_sum": [0.4, 7.31, 0.0],
                "wind_speed_10m_max": [15.2, 22.7, 8.1]
            }
        })
    }

    #[test]
    fn a_forecast_becomes_a_result() {
        let spot = parse_spots(&spots())[1].clone();
        let hit = parse_forecast(&forecast(), &spot).unwrap();
        assert_eq!(hit.title, "Weather in Glenbrackie, Scotland, United Kingdom");
        assert_eq!(hit.url, "https://open-meteo.com/en/docs?latitude=56.49&longitude=-3.19");
        assert_eq!(hit.published.as_deref(), Some("2026-11-01"));
        // Open-Meteo's "mp/h" said as people write it.
        assert!(hit.text.starts_with("Now (14:15 local time): 12°C, feels like 9°C, partly cloudy, wind 9 mph, humidity 71%."), "{}", hit.text);
        assert!(hit.text.contains("Today (Sun 1 Nov): partly cloudy, 8 to 13°C, rain 20% (0.4 mm), wind up to 15 mph."), "{}", hit.text);
        assert!(hit.text.contains("Tomorrow (Mon 2 Nov): rain, 6 to 11°C, rain 85% (7.3 mm), wind up to 23 mph."), "{}", hit.text);
        assert!(hit.text.contains("Tue 3 Nov: overcast, 4 to 10°C, rain 10%, wind up to 8 mph."), "{}", hit.text);
        assert!(hit.text.ends_with(CREDIT), "the credit CC BY 4.0 asks for");
        assert_eq!(parse_forecast(&json!({}), &spot), None);
        assert_eq!(sky(95), "thunderstorms");
    }
}
