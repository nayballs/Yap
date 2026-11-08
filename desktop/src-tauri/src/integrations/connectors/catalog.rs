//! **The catalogue** (Settings → Connectors → Discover): the connectors Yap
//! can use today, bundled with Yap (`catalog.json`, read into the binary:
//! nothing is fetched to show it). Only official servers that let any app
//! sign in by itself (dynamic client registration; Yap's client ID metadata
//! document once it's hosted), take a pasted token, or run on this PC; the
//! famous ones that don't yet (Google, Microsoft 365, Slack, Figma) are
//! "Coming later" cards that say why.
//!
//! An entry's id stays when its address changes (a connector added from it
//! keeps `origin: {kind: "catalog", catalog: <id>}`), and it names the
//! receipts' and the logs' word for that connector (`Connector::kind`: the
//! catalogue's name, never one the person typed). `tested` is the date
//! someone signed in and ran its tools with Yap: the page shows **Tested with
//! Yap** for it, and never a tick claiming anyone's review.
//!
//! A test run (debug, test mode) may point it at a file of its own
//! (`YAP_E2E_CONNECTOR_CATALOG`). Registry search isn't here: it needs a
//! hosted copy of the MCP Registry, which is the maintainer's to approve.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use super::store::Setting;

/// The bundled list.
const BUNDLED: &str = include_str!("catalog.json");

/// How a catalogue entry connects and signs in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SignIn {
    /// In the browser (the MCP authorization spec, `oauth.rs`).
    Oauth,
    /// A pasted token, sent as `Authorization: Bearer`.
    Token,
    /// A program on this PC (its command shown before it runs).
    Local,
}

/// Where to make a token, for an entry that takes one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenHelp {
    pub help: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

/// One connector in the catalogue.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub id: String,
    pub name: String,
    pub publisher: String,
    pub blurb: String,
    /// Its mark in `desktop/src/assets/connectors/` (`None`: its initial).
    #[serde(default)]
    pub logo: Option<String>,
    /// The initial's tile colour when there's no mark.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    /// A link's address (a program has none).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback_sse: Option<String>,
    /// The person gives the address (a server on their own network: Home
    /// Assistant), starting from `url`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub url_editable: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub registry_name: Option<String>,
    pub sign_in: SignIn,
    /// A token: the only way in (`signIn: "token"`), or another way beside
    /// the browser's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token: Option<TokenHelp>,
    /// A program's command and arguments (`signIn: "local"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    /// What a program needs installed ("Node.js").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub needs: Option<String>,
    /// When someone signed in and ran its tools with Yap (YYYY-MM-DD).
    #[serde(default)]
    pub tested: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    /// Tools that start stricter than their kind's default (only Ask first
    /// or Off: an entry never makes a tool Always allow).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub defaults: BTreeMap<String, Setting>,
    /// The vendor's page about its server.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub docs: Option<String>,
}

/// A "Coming later" card: a service Yap can't sign in to yet, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Later {
    pub id: String,
    pub name: String,
    pub blurb: String,
    #[serde(default)]
    pub logo: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    pub why: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Catalog {
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    pub connectors: Vec<Entry>,
    #[serde(default)]
    pub later: Vec<Later>,
}

impl Catalog {
    pub fn get(&self, id: &str) -> Option<&Entry> {
        self.connectors.iter().find(|e| e.id == id)
    }
}

/// `text` as a catalogue, every entry checked ([`check`]). Pure.
pub fn parse(text: &str) -> Result<Catalog, String> {
    let c: Catalog = serde_json::from_str(text).map_err(|e| format!("The connector catalogue isn't valid: {e}"))?;
    check(&c)?;
    Ok(c)
}

/// What every entry must be: a unique id, a link Yap may connect to (or a
/// program with its command), a token's help when it takes one, defaults
/// that only make tools stricter, a tested date that is one. Pure.
pub fn check(c: &Catalog) -> Result<(), String> {
    let mut ids = std::collections::BTreeSet::new();
    for e in &c.connectors {
        let bad = |why: &str| Err(format!("Catalogue entry {:?}: {why}", e.id));
        if e.id.is_empty() || !e.id.chars().all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-') || !ids.insert(e.id.clone()) {
            return bad("its id must be unique, in a-z, 0-9 and -");
        }
        if e.name.trim().is_empty() || e.publisher.trim().is_empty() || e.blurb.trim().is_empty() {
            return bad("it needs a name, a publisher and a line");
        }
        match e.sign_in {
            SignIn::Local => {
                if e.command.as_deref().is_none_or(|c| c.trim().is_empty()) || e.url.is_some() {
                    return bad("a program has a command and no address");
                }
            }
            SignIn::Oauth | SignIn::Token => {
                let Some(url) = &e.url else { return bad("a link needs its address") };
                let (u, place) = super::remote::check_url(url).map_err(|why| format!("Catalogue entry {:?}: {why}", e.id))?;
                if place == crate::privacy::net::Place::Internet && u.scheme() != "https" {
                    return bad("a server on the internet needs https");
                }
                if let Some(f) = &e.fallback_sse {
                    let (fu, _) = super::remote::check_url(f).map_err(|why| format!("Catalogue entry {:?}: {why}", e.id))?;
                    if fu.origin() != u.origin() {
                        return bad("its older transport's address must be on the same server");
                    }
                }
            }
        }
        if e.sign_in == SignIn::Token && e.token.is_none() {
            return bad("a token connector says where to make one");
        }
        if e.defaults.values().any(|s| *s == Setting::Allow) {
            return bad("an entry never makes a tool Always allow");
        }
        if let Some(t) = &e.tested {
            let ok = t.len() == 10 && t.chars().enumerate().all(|(i, ch)| if i == 4 || i == 7 { ch == '-' } else { ch.is_ascii_digit() });
            if !ok {
                return bad("its tested date is YYYY-MM-DD");
            }
        }
        for link in [e.docs.as_deref(), e.token.as_ref().and_then(|t| t.url.as_deref())].into_iter().flatten() {
            if !link.starts_with("https://") {
                return bad("its links are https://");
            }
        }
    }
    for l in &c.later {
        if l.id.is_empty() || ids.contains(&l.id) || l.why.trim().is_empty() {
            return Err(format!("Catalogue card {:?}: a unique id and why it's later", l.id));
        }
    }
    Ok(())
}

/// The catalogue this run shows: the bundled one, or a test run's
/// (`YAP_E2E_CONNECTOR_CATALOG`, a debug build in test mode). Read once.
pub fn get() -> &'static Catalog {
    static CATALOG: OnceLock<Catalog> = OnceLock::new();
    CATALOG.get_or_init(|| {
        #[cfg(debug_assertions)]
        if crate::shell::e2e::active() {
            if let Some(path) = std::env::var_os("YAP_E2E_CONNECTOR_CATALOG").filter(|p| !p.is_empty()) {
                match std::fs::read_to_string(&path).map_err(|e| e.to_string()).and_then(|t| parse(&t)) {
                    Ok(c) => return c,
                    Err(_) => tracing::warn!("connectors: the test run's catalogue couldn't be read"),
                }
            }
        }
        parse(BUNDLED).unwrap_or_default()
    })
}

/// The entry connector `origin` came from, if it came from the catalogue.
pub fn of(origin: &super::store::Origin) -> Option<&'static Entry> {
    if origin.kind != "catalog" {
        return None;
    }
    let id = origin.extra.get("catalog").and_then(serde_json::Value::as_str)?;
    get().get(id)
}

/// The settings `entry` starts the `added` tools at (stricter than their
/// defaults only; a tool that's Off stays Off). Pure.
pub fn start_settings(entry: &Entry, added: &[String], settings: &mut BTreeMap<String, super::store::ToolSetting>) -> usize {
    let mut n = 0;
    for name in added {
        let (Some(want), Some(s)) = (entry.defaults.get(name), settings.get_mut(name)) else { continue };
        let stricter = matches!((s.setting, want), (Setting::Allow, Setting::Ask | Setting::Off) | (Setting::Ask, Setting::Off));
        if stricter {
            s.setting = *want;
            n += 1;
        }
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::integrations::connectors::store::ToolSetting;

    #[test]
    fn the_bundled_catalogue_is_valid_and_honest() {
        let c = parse(BUNDLED).expect("catalog.json parses and passes its checks");
        assert!(c.connectors.len() >= 10, "{}", c.connectors.len());
        assert!(c.later.len() >= 4);
        // Nobody has signed in to these with Yap yet: no "Tested with Yap"
        // until someone has (a date, by hand, after a real sign-in).
        assert!(c.connectors.iter().all(|e| e.tested.is_none()), "a tested date is set only after a real sign-in with Yap");
        // A server on the person's own network: its address is theirs to give.
        for e in &c.connectors {
            if let Some(u) = &e.url {
                let (_, place) = super::super::remote::check_url(u).unwrap();
                assert!(place == crate::privacy::net::Place::Internet || e.url_editable, "{}: its address is the person's", e.id);
            }
        }
        // The famous ones that need a deal are cards that say why, never entries.
        for id in ["google", "microsoft-365", "slack", "figma"] {
            assert!(c.get(id).is_none(), "{id} can't be signed in to yet");
            assert!(c.later.iter().any(|l| l.id == id), "{id}'s card");
        }
        // GitHub and Zapier take a token; Home Assistant's address is the person's.
        assert_eq!(c.get("github").unwrap().sign_in, SignIn::Token);
        assert_eq!(c.get("zapier").unwrap().sign_in, SignIn::Token);
        let ha = c.get("home-assistant").unwrap();
        assert!(ha.url_editable && ha.sign_in == SignIn::Token);
        assert_eq!(c.get("files").unwrap().sign_in, SignIn::Local);
        assert!(c.get("stripe").unwrap().defaults.values().all(|s| *s == Setting::Off));
    }

    fn entry(json: serde_json::Value) -> Result<Catalog, String> {
        parse(&serde_json::json!({ "version": 1, "connectors": [json] }).to_string())
    }

    #[test]
    fn an_entry_that_claims_too_much_is_refused() {
        let base = serde_json::json!({ "id": "x", "name": "X", "publisher": "X Co", "blurb": "A thing.", "url": "https://mcp.example.com/mcp", "signIn": "oauth" });
        assert!(entry(base.clone()).is_ok());
        let with = |k: &str, v: serde_json::Value| {
            let mut e = base.clone();
            e[k] = v;
            entry(e)
        };
        assert!(with("url", "http://mcp.example.com/mcp".into()).unwrap_err().contains("https"));
        assert!(with("url", "http://192.168.1.20:8123/api/mcp".into()).is_ok(), "a server on the network may use http");
        assert!(with("defaults", serde_json::json!({ "do_it": "allow" })).unwrap_err().contains("Always allow"));
        assert!(with("tested", "soon".into()).unwrap_err().contains("YYYY-MM-DD"));
        assert!(with("signIn", "token".into()).unwrap_err().contains("where to make one"));
        assert!(with("docs", "http://example.com".into()).unwrap_err().contains("https"));
        assert!(with("fallbackSse", "https://other.example.com/sse".into()).unwrap_err().contains("same server"));
        assert!(with("id", "Not An Id".into()).is_err());
        assert!(with("signIn", "local".into()).unwrap_err().contains("command"));
    }

    #[test]
    fn its_defaults_only_make_tools_stricter() {
        let c = entry(serde_json::json!({ "id": "x", "name": "X", "publisher": "X Co", "blurb": "A thing.", "url": "https://mcp.example.com/mcp", "signIn": "oauth",
            "defaults": { "refund": "off", "search": "ask", "already_off": "ask" } }))
        .unwrap();
        let e = &c.connectors[0];
        let mut s: BTreeMap<String, ToolSetting> = BTreeMap::new();
        let t = |setting| ToolSetting { setting, hash: "h".into() };
        s.insert("refund".into(), t(Setting::Ask));
        s.insert("search".into(), t(Setting::Allow));
        s.insert("already_off".into(), t(Setting::Off));
        s.insert("other".into(), t(Setting::Allow));
        let added: Vec<String> = s.keys().cloned().collect();
        assert_eq!(start_settings(e, &added, &mut s), 2);
        assert_eq!(s["refund"].setting, Setting::Off);
        assert_eq!(s["search"].setting, Setting::Ask);
        assert_eq!(s["already_off"].setting, Setting::Off, "never loosened");
        assert_eq!(s["other"].setting, Setting::Allow, "not in its defaults");
        // Only tools that are new: one the person set is theirs.
        s.get_mut("search").unwrap().setting = Setting::Allow;
        assert_eq!(start_settings(e, &[], &mut s), 0);
        assert_eq!(s["search"].setting, Setting::Allow);
    }
}
