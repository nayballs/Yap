//! **Where a place is: OpenStreetMap's Nominatim** (`nominatim.openstreetmap
//! .org`, keyless), for "where is…" and "the address of…". Its usage policy
//! (operations.osmfoundation.org/policies/nominatim, read 1 Nov 2026): "an
//! absolute maximum of 1 request per second" (Yap: one every 1.1 s, never
//! two at once); "Provide a valid HTTP Referer or User-Agent identifying
//! the application" (Yap's own, with its contact address); "Results must be
//! cached on your side" (a day here); "Auto-complete search" is not allowed
//! (Yap asks once per search, never as someone types) nor systematic or bulk
//! queries; "Clearly display attribution" (each result says "Map data ©
//! OpenStreetMap contributors (ODbL)" and links to openstreetmap.org); and
//! "Apps must make sure that they can switch the service at our request at
//! any time" — [`NOMINATIM`] is the one address to change (or empty it to
//! switch the source off), and a refusal leaves the answer to Wikipedia.

use std::time::Duration;

use reqwest::Url;
use serde_json::Value;

use crate::privacy::net::FreeSource;

use super::super::{tidy, Endpoints, Hit, Settings};
use super::polite::{self, Ask, Rules};
use super::route::normal;
use super::Out;

/// Nominatim's search, the one address its policy may ask Yap to change.
pub const NOMINATIM: &str = "https://nominatim.openstreetmap.org/search";
const RULES: Rules = Rules { gap: Duration::from_millis(1_100), per_minute: None, keep: Duration::from_secs(24 * 3600) };
/// The attribution its policy asks for (ODbL).
pub const CREDIT: &str = "Map data © OpenStreetMap contributors (ODbL).";

/// The words around a place in a question (in the person's languages,
/// English always): taken off both ends ("where is", "the address of"…),
/// and the ones never in a place's name ("location", "located"…) wherever
/// they are: "Eden Project location Cornwall UK" finds nothing on Nominatim.
struct Around {
    ends: Vec<&'static str>,
    never: Vec<&'static str>,
}

impl Around {
    fn for_langs(langs: &[String]) -> Around {
        const EN: &[&str] = &[
            "is", "are", "the", "a", "an", "post", "code", "zip", "of", "for", "which", "country", "county", "state",
            "region", "province", "city", "town", "continent", "in", "find", "map", "on", "show", "me", "what", "what's",
            "can", "you", "tell", "it", "its",
        ];
        const EN_NEVER: &[&str] = &[
            "where", "where's", "wheres", "address", "location", "coordinates", "postcode", "located", "situated",
            "exactly", "please",
        ];
        const OTHER: &[(&str, &[&str], &[&str])] = &[
            ("fr", &["où", "ou", "est", "se", "sont", "de", "du", "des", "la", "le", "les"], &["trouve", "situe", "adresse"]),
            ("de", &["wo", "ist", "sind", "sich", "von", "der", "die", "das"], &["liegt", "befindet", "adresse"]),
            ("es", &["dónde", "donde", "está", "esta", "queda", "se", "están", "de", "el", "la"], &["encuentra", "dirección"]),
            ("it", &["dove", "si", "è", "dov'è", "sono", "di", "del", "della", "il", "la"], &["trova", "indirizzo"]),
            ("pt", &["onde", "está", "é", "ficam", "de", "do", "da", "o", "a"], &["fica", "endereço"]),
            ("nl", &["waar", "is", "zijn", "zich", "van", "de", "het"], &["ligt", "bevindt", "adres"]),
        ];
        let mut around = Around { ends: EN.to_vec(), never: EN_NEVER.to_vec() };
        for (code, ends, never) in OTHER {
            if langs.iter().any(|l| l.to_ascii_lowercase().starts_with(code)) {
                around.ends.extend_from_slice(ends);
                around.never.extend_from_slice(never);
            }
        }
        around
    }

    fn is(list: &[&str], w: &str) -> bool {
        let w = normal(w);
        let w = w.trim_matches(',');
        list.contains(&w)
    }

    /// `words` without the words around a place at either end.
    fn trim(&self, mut words: Vec<String>) -> String {
        let around = |w: &String| Self::is(&self.ends, w) || Self::is(&self.never, w);
        while words.first().is_some_and(around) {
            words.remove(0);
        }
        while words.last().is_some_and(around) {
            words.pop();
        }
        words.join(" ").trim_matches(',').to_string()
    }
}

fn words_of(query: &str) -> Vec<String> {
    query.replace(['?', '!', '"', '“', '”', '¿', '¡'], " ").split_whitespace().map(str::to_string).collect()
}

/// What to look up: the place, without "where is", "the address of",
/// "located"… — from both ends, and the words never in a name from the
/// middle too ("on" stays: "Stoke on Trent"). Pure.
pub fn place_terms(query: &str, langs: &[String]) -> String {
    let around = Around::for_langs(langs);
    around.trim(words_of(query).into_iter().filter(|w| !Around::is(&around.never, w)).collect())
}

/// The place before the first word never in a name ("Eden Project" from
/// "Eden Project location Cornwall UK"): the second try when the whole
/// finds nothing. Pure.
pub fn place_head(query: &str, langs: &[String]) -> String {
    let around = Around::for_langs(langs);
    let words = words_of(query);
    let start = words.iter().position(|w| !Around::is(&around.ends, w) && !Around::is(&around.never, w)).unwrap_or(words.len());
    around.trim(words[start..].iter().take_while(|w| !Around::is(&around.never, w)).cloned().collect())
}

fn letters(s: &str) -> usize {
    s.chars().filter(|c| c.is_alphabetic()).count()
}

/// Nominatim's request: two places, in the person's language. Pure.
pub fn search_url(place: &str, lang: &str) -> Option<Url> {
    let mut url = Url::parse(NOMINATIM).ok()?;
    url.query_pairs_mut()
        .append_pair("q", place)
        .append_pair("format", "jsonv2")
        .append_pair("limit", "2")
        .append_pair("accept-language", lang);
    Some(url)
}

/// Nominatim's places as results (jsonv2: `[{osm_type, osm_id, lat, lon,
/// category, type, name, display_name}]`), each linked to its page on
/// openstreetmap.org, with where it is and the attribution. Pure.
pub fn parse(v: &Value) -> Vec<Hit> {
    v.as_array()
        .into_iter()
        .flatten()
        .filter_map(|p| {
            let osm_type = p["osm_type"].as_str().filter(|t| matches!(*t, "node" | "way" | "relation"))?;
            let osm_id = p["osm_id"].as_u64()?;
            let display = p["display_name"].as_str()?.trim().to_string();
            let name = p["name"].as_str().map(str::trim).filter(|n| !n.is_empty()).map_or_else(|| display.split(',').next().unwrap_or("").trim().to_string(), str::to_string);
            let kind = p["type"].as_str().unwrap_or("").replace('_', " ");
            let category = p["category"].as_str().unwrap_or("").replace('_', " ");
            let what = match (kind.as_str(), category.as_str()) {
                ("", _) | ("yes", _) => String::new(),
                (k, "") => format!(" It's a {k}."),
                (k, c) if k == c => format!(" It's a {k}."),
                (k, c) => format!(" It's a {k} ({c})."),
            };
            let at = match (p["lat"].as_str().and_then(|s| s.parse::<f64>().ok()), p["lon"].as_str().and_then(|s| s.parse::<f64>().ok())) {
                (Some(lat), Some(lon)) => format!(" Coordinates: {lat:.4}, {lon:.4}."),
                _ => String::new(),
            };
            Some(Hit {
                title: tidy(&name, 120),
                url: format!("https://www.openstreetmap.org/{osm_type}/{osm_id}"),
                published: None,
                text: tidy(&format!("{display}.{what}{at} {CREDIT}"), 600),
            })
        })
        .collect()
}

/// **A place for a search**: what to look up, from the model's query (else
/// the person's question), asked of Nominatim; nothing found → once more
/// with only the place's name ([`place_head`]).
pub(super) async fn look_up(s: &Settings, ep: &Endpoints, query: &str, context: &str, langs: &[String]) -> Result<Out, String> {
    let mut from = query;
    let mut place = place_terms(query, langs);
    if letters(&place) < 2 {
        from = context;
        place = place_terms(context, langs);
    }
    if letters(&place) < 2 {
        return Ok(Out::default());
    }
    let mut hits = ask(s, ep, &place).await?;
    let head = place_head(from, langs);
    if hits.is_empty() && head != place && letters(&head) >= 2 {
        hits = ask(s, ep, &head).await?;
    }
    Ok(Out { hits, from: "OpenStreetMap".into(), ..Default::default() })
}

async fn ask(s: &Settings, ep: &Endpoints, place: &str) -> Result<Vec<Hit>, String> {
    let Some(url) = search_url(place, &s.lang) else { return Ok(Vec::new()) };
    let body = polite::get(ep, &url, Ask { purpose: FreeSource::Places, rules: RULES, accept: "application/json", headers: &[] }).await?;
    let v: Value = serde_json::from_str(&body).map_err(|_| "OpenStreetMap sent an answer Yap couldn't read.".to_string())?;
    Ok(parse(&v))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_place_is_what_is_asked_about() {
        let en = vec!["en".to_string()];
        assert_eq!(place_terms("Where is Glenbrackie Castle?", &en), "Glenbrackie Castle");
        assert_eq!(place_terms("address of Kellbrook Town Hall", &en), "Kellbrook Town Hall");
        assert_eq!(place_terms("Glenbrackie Castle location", &en), "Glenbrackie Castle");
        assert_eq!(place_terms("Which country is Port Ellis in?", &en), "Port Ellis");
        // The middle is kept ("Stoke on Trent")…
        assert_eq!(place_terms("where is Stoke on Trent", &en), "Stoke on Trent");
        // …but not a word never in a name (Nominatim finds nothing with it).
        assert_eq!(place_terms("Glenbrackie Castle location Fellshire UK", &en), "Glenbrackie Castle Fellshire UK");
        assert_eq!(place_terms("Where is Port Ellis located exactly?", &en), "Port Ellis");
        // The second try: the place's name alone.
        assert_eq!(place_head("Glenbrackie Castle location Fellshire UK", &en), "Glenbrackie Castle");
        assert_eq!(place_head("Where is Glenbrackie Castle?", &en), "Glenbrackie Castle");
        let fr = vec!["fr".to_string(), "en".to_string()];
        assert_eq!(place_terms("Où se trouve Port Ellis ?", &fr), "Port Ellis");
        let url = search_url("Glenbrackie Castle", "en").unwrap().to_string();
        assert_eq!(url, "https://nominatim.openstreetmap.org/search?q=Glenbrackie+Castle&format=jsonv2&limit=2&accept-language=en");
    }

    #[test]
    fn its_places_become_results_with_the_attribution() {
        // Nominatim's documented jsonv2 shape, a made-up place.
        let v = json!([
            { "place_id": 1, "licence": "Data © OpenStreetMap contributors, ODbL 1.0. http://osm.org/copyright", "osm_type": "way", "osm_id": 4242,
              "lat": "56.4907", "lon": "-3.1872", "category": "historic", "type": "castle", "place_rank": 30, "importance": 0.31,
              "addresstype": "historic", "name": "Glenbrackie Castle", "display_name": "Glenbrackie Castle, Kellbrook, Fellshire, Scotland, United Kingdom" },
            { "place_id": 2, "osm_type": "node", "osm_id": 7, "lat": "1", "lon": "2", "category": "place", "type": "place", "name": "", "display_name": "Glenbrackie, Fellshire" },
            { "place_id": 3, "osm_type": "bogus", "osm_id": 9, "display_name": "Nowhere" }
        ]);
        let hits = parse(&v);
        assert_eq!(hits.len(), 2, "an unknown kind of object is dropped");
        assert_eq!(hits[0].title, "Glenbrackie Castle");
        assert_eq!(hits[0].url, "https://www.openstreetmap.org/way/4242");
        assert_eq!(
            hits[0].text,
            "Glenbrackie Castle, Kellbrook, Fellshire, Scotland, United Kingdom. It's a castle (historic). Coordinates: 56.4907, -3.1872. Map data © OpenStreetMap contributors (ODbL)."
        );
        // No name: the first part of its address.
        assert_eq!(hits[1].title, "Glenbrackie");
        assert!(hits[1].text.contains("It's a place."));
        assert!(parse(&json!({ "error": "x" })).is_empty());
    }
}
