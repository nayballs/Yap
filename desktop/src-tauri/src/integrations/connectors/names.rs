//! **Connectors' tool names as a model sees them**: `<slug>__<tool>`, safe
//! for every provider Yap talks to. OpenAI, Groq and Anthropic take
//! `[A-Za-z0-9_-]{1,64}`; Gemini wants a letter (or `_`) first and at most
//! 64 too. So a wire name is the connector's slug (`[a-z0-9]`, a letter
//! first, at most [`SLUG_MAX`]), two underscores, and the tool's own name
//! with every other character made `_`; cut to [`MAX`] with a 4-character
//! hash of the tool's real name when it's longer, and the same when two of a
//! connector's tools would come out alike. Yap's own tools never contain
//! `__`, and a slug never does, so a wire name is a connector's at its
//! first `__` — and a server can't shadow one of Yap's tools, or another
//! connector's. The way back is the [`Map`], never a guess from the text
//! (making a name safe loses characters).

use std::collections::BTreeMap;

use sha2::{Digest, Sha256};

/// The longest wire name (every provider's limit).
pub const MAX: usize = 64;
/// The longest slug.
pub const SLUG_MAX: usize = 12;
/// Between the slug and the tool's name.
pub const SEP: &str = "__";

/// A connector's slug from its name: lower-case ASCII letters and digits
/// only, a letter first ("2do" → "c2do"), at most [`SLUG_MAX`], and not one
/// of `taken` (a digit added: "notion2"). Pure.
pub fn slug(name: &str, taken: &[&str]) -> String {
    let mut base: String = name
        .chars()
        .flat_map(char::to_lowercase)
        .filter(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        .collect();
    if base.is_empty() {
        base = "connector".into();
    }
    if !base.starts_with(|c: char| c.is_ascii_lowercase()) {
        base.insert(0, 'c');
    }
    base.truncate(SLUG_MAX);
    if !taken.contains(&base.as_str()) {
        return base;
    }
    for n in 2u32.. {
        let suffix = n.to_string();
        let mut s = base.clone();
        s.truncate(SLUG_MAX - suffix.len());
        s.push_str(&suffix);
        if !taken.contains(&s.as_str()) {
            return s;
        }
    }
    unreachable!("a free slug is always found")
}

/// Whether `name` is a connector's wire name (Yap's own never have `__`):
/// the tool loop's dispatch (wave 4).
#[cfg_attr(not(test), allow(dead_code))]
pub fn is_connector_tool(name: &str) -> bool {
    name.contains(SEP)
}

/// A tool's own name with only `[A-Za-z0-9_-]` left (others made `_`).
fn safe_part(tool: &str) -> String {
    let s: String = tool.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' }).collect();
    if s.is_empty() {
        "tool".into()
    } else {
        s
    }
}

/// Four hex characters of `tool`'s hash: what tells cut names apart.
fn tag(tool: &str) -> String {
    let h = Sha256::digest(tool.as_bytes());
    format!("{:02x}{:02x}", h[0], h[1])
}

/// `slug`'s wire name for `tool`, whole when it fits ([`MAX`]), else cut
/// with its tag (`forced`: the tag even when it fits, for a name another
/// tool already has). Pure.
fn wire(slug: &str, tool: &str, forced: bool) -> String {
    let part = safe_part(tool);
    let whole = format!("{slug}{SEP}{part}");
    if !forced && whole.len() <= MAX {
        return whole;
    }
    let room = MAX - slug.len() - SEP.len() - 5; // "_" + 4 hex
    let mut cut = part;
    cut.truncate(room);
    format!("{slug}{SEP}{cut}_{}", tag(tool))
}

/// The wire names of every connector's tools, and the way back.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Map {
    /// Wire name → (connector id, the tool's own name).
    back: BTreeMap<String, (String, String)>,
    /// (connector id, the tool's own name) → wire name.
    forth: BTreeMap<(String, String), String>,
}

impl Map {
    /// Add a connector's tools (`id`, its `slug`, the tools' own names, in
    /// the order the server listed them). Names that come out alike get
    /// their tags; the first keeps the plain one. Pure.
    pub fn add<'a>(&mut self, id: &str, slug: &str, tools: impl IntoIterator<Item = &'a str>) {
        for tool in tools {
            let key = (id.to_string(), tool.to_string());
            if self.forth.contains_key(&key) {
                continue;
            }
            let mut name = wire(slug, tool, false);
            if self.back.contains_key(&name) {
                name = wire(slug, tool, true);
            }
            // Two tags alike (one in 65,536): number it.
            let mut n = 2;
            while self.back.contains_key(&name) {
                let suffix = n.to_string();
                let mut base = wire(slug, tool, true);
                base.truncate(MAX - suffix.len());
                name = format!("{base}{suffix}");
                n += 1;
            }
            self.back.insert(name.clone(), key.clone());
            self.forth.insert(key, name);
        }
    }

    /// The connector and tool a wire name stands for (the tool loop's way
    /// back, wave 4).
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn resolve(&self, wire: &str) -> Option<(&str, &str)> {
        self.back.get(wire).map(|(id, tool)| (id.as_str(), tool.as_str()))
    }

    /// A tool's wire name.
    pub fn wire_name(&self, id: &str, tool: &str) -> Option<&str> {
        self.forth.get(&(id.to_string(), tool.to_string())).map(String::as_str)
    }
}

/// Whether every provider takes `name` as a tool's name: `[A-Za-z0-9_-]`,
/// a letter first, at most [`MAX`]. Pure.
#[cfg_attr(not(test), allow(dead_code))]
pub fn provider_safe(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX
        && name.starts_with(|c: char| c.is_ascii_alphabetic())
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs_are_short_lower_case_letters_first_and_unique() {
        assert_eq!(slug("Home Assistant", &[]), "homeassistan");
        assert_eq!(slug("GitHub", &[]), "github");
        assert_eq!(slug("2do", &[]), "c2do");
        assert_eq!(slug("Ünïcode ☃", &[]), "ncode");
        assert_eq!(slug("☃☃", &[]), "connector");
        assert_eq!(slug("Notion", &["notion"]), "notion2");
        assert_eq!(slug("Notion", &["notion", "notion2"]), "notion3");
        assert_eq!(slug("Home Assistant", &["homeassistan"]), "homeassista2");
        for s in [slug("a__b", &[]), slug("x_y", &[])] {
            assert!(!s.contains('_'), "{s}");
        }
    }

    #[test]
    fn wire_names_round_trip_and_every_provider_takes_them() {
        let mut m = Map::default();
        m.add("c1", "ha", ["HassTurnOn", "intent__HassTurnOn", "get.weather", "get weather", "créer-page"]);
        let names: Vec<&str> = ["HassTurnOn", "intent__HassTurnOn", "get.weather", "get weather", "créer-page"]
            .iter()
            .map(|t| m.wire_name("c1", t).unwrap())
            .collect();
        assert_eq!(names[0], "ha__HassTurnOn");
        // HA's own `__` survives: mapped back at the slug's `__`, by the map.
        assert_eq!(names[1], "ha__intent__HassTurnOn");
        assert_eq!(m.resolve("ha__intent__HassTurnOn"), Some(("c1", "intent__HassTurnOn")));
        assert_eq!(names[2], "ha__get_weather");
        // "get weather" comes out like "get.weather": tagged, both kept.
        assert!(names[3].starts_with("ha__get_weather_") && names[3].len() == "ha__get_weather_".len() + 4, "{}", names[3]);
        assert_eq!(names[4], "ha__cr_er-page");
        for (n, tool) in names.iter().zip(["HassTurnOn", "intent__HassTurnOn", "get.weather", "get weather", "créer-page"]) {
            assert!(provider_safe(n), "{n}");
            assert!(is_connector_tool(n));
            assert_eq!(m.resolve(n), Some(("c1", tool)));
        }
        assert_eq!(m.back.len(), 5);
        assert!(m.resolve("ha__nothing").is_none());
        assert!(!is_connector_tool("search_notes"));
    }

    #[test]
    fn long_names_are_cut_to_64_with_a_tag_that_tells_them_apart() {
        let long_a = format!("{}_alpha", "x".repeat(70));
        let long_b = format!("{}_beta", "x".repeat(70));
        let mut m = Map::default();
        m.add("c2", "linear", [long_a.as_str(), long_b.as_str()]);
        let a = m.wire_name("c2", &long_a).unwrap().to_string();
        let b = m.wire_name("c2", &long_b).unwrap().to_string();
        assert_eq!(a.len(), MAX);
        assert_eq!(b.len(), MAX);
        assert_ne!(a, b);
        assert!(provider_safe(&a) && provider_safe(&b));
        assert_eq!(m.resolve(&a), Some(("c2", long_a.as_str())));
        assert_eq!(m.resolve(&b), Some(("c2", long_b.as_str())));
        // The same names every time (prompt caches).
        let mut again = Map::default();
        again.add("c2", "linear", [long_a.as_str(), long_b.as_str()]);
        assert_eq!(again, m);
    }

    #[test]
    fn two_connectors_tools_of_one_name_never_meet() {
        let mut m = Map::default();
        m.add("c1", "github", ["search"]);
        m.add("c2", "linear", ["search"]);
        assert_eq!(m.wire_name("c1", "search"), Some("github__search"));
        assert_eq!(m.wire_name("c2", "search"), Some("linear__search"));
        assert!(!provider_safe("1abc") && !provider_safe("") && !provider_safe("a.b") && !provider_safe(&"a".repeat(65)));
    }
}
