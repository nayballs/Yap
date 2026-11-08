//! **Connectors and the person's data** (Connectors wave 8): what Yap's
//! privacy features do with them. They describe and clear **what's
//! stored, whatever the preview's switch says**: someone who tried the
//! preview and turned it off still gets their sign-ins ended by Reset app,
//! and their connectors in Download your data. With the switch off they
//! read connectors.json and the vault's names only to find what's there,
//! start nothing, and make a request only to revoke a sign-in that's
//! stored; with nothing stored they add nothing (no file, no line).
//!
//! - **Delete some of your data / Reset app** (`privacy/reset.rs`'s
//!   "Connector sign-ins"): [`sign_ins`] counts the browser sign-ins,
//!   [`sign_out_all`] ends every one as Sign out does — revoked where the
//!   server offers it, the tokens gone from the vault whatever it answered.
//!   The connectors, their tool settings, a pasted token or headers, a
//!   program's secret variables, the person's own client and Yap's
//!   registrations stay: they're settings, set up on purpose (as config.json's
//!   API keys are), so signing in again is one click.
//! - **Download your data** (`privacy/export.rs`): [`export`], the
//!   connectors as an allow-list — each one's name, how it's reached (a
//!   link's address without its query, user name or fragment; a program's
//!   command line), its header and variable *names*, how it signs in, where
//!   Yap uses it and each tool's setting. Never a value from the vault: the
//!   export never reads it.
//! - **Report a problem** (`privacy/diagnostics.rs`): [`counts`], how many
//!   there are by kind (link, program), off, signed in, failing and from the
//!   catalogue. Never a name, an address, a command or a tool.

use std::time::Duration;

use serde::Serialize;
use serde_json::{json, Value};

use super::manager::Status;
use super::store::{AuthKind, Connector, Transport};
use super::{cache, enabled, oauth, store, vault};

/// A sign-out's revocations wait at most this long, all of them together.
const REVOKE_LIMIT: Duration = Duration::from_secs(10);

/// Whether connector `c` holds a browser sign-in: signed in, or tokens in
/// the vault (only a browser sign-in's connector can have them).
fn holds_sign_in(c: &Connector) -> bool {
    c.auth.kind == AuthKind::Oauth && (c.auth.signed_in || vault::load(&c.id).grant.is_some())
}

/// The browser sign-ins stored now: the connectors in `all` that hold one,
/// and sign-ins left in the vault under an id no connector has (its
/// connectors.json lost or set aside): `(connectors', left)`.
fn sign_ins_in(all: &[Connector]) -> (Vec<String>, Vec<String>) {
    let held: Vec<String> = all.iter().filter(|c| holds_sign_in(c)).map(|c| c.id.clone()).collect();
    let left: Vec<String> =
        (1..=store::MAX).map(|n| format!("c{n}")).filter(|id| !all.iter().any(|c| &c.id == id) && vault::load(id).grant.is_some()).collect();
    (held, left)
}

/// How many browser sign-ins are stored now (Delete some of your data's
/// count), whatever the preview says.
pub fn sign_ins() -> usize {
    let (held, left) = sign_ins_in(&store::list());
    held.len() + left.len()
}

/// Whether any connector is kept (connectors.json lists one), whatever the
/// preview says: what stays of them is then worth saying.
pub fn any_kept() -> bool {
    !store::list().is_empty()
}

/// What [`sign_out_all`] did (counts only).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SignedOut {
    /// Sign-ins ended here.
    pub signed_out: usize,
    /// Of them, the ones the server confirmed it revoked.
    pub confirmed: usize,
}

/// **Every browser sign-in ended** ("Connector sign-ins", Reset app),
/// whatever the preview says: any sign-in under way cancelled, each
/// connection with one stopped (if any was started), its grant revoked
/// where the server offers it (all at once, [`REVOKE_LIMIT`] in all), its
/// tokens gone from the vault and connectors.json saying it's signed out —
/// also for one whose server didn't answer in time; and a sign-in left in
/// the vault for a connector that's gone, its tokens deleted (there's no
/// server to tell). Nothing else changes.
pub async fn sign_out_all() -> SignedOut {
    let all = store::list();
    for c in &all {
        oauth::cancel(&c.id);
    }
    let (ids, left) = sign_ins_in(&all);
    for id in &ids {
        super::stop_if_started(id).await;
    }
    let revoked = futures_util::future::join_all(ids.iter().map(|id| oauth::sign_out(id)));
    let confirmed = match tokio::time::timeout(REVOKE_LIMIT, revoked).await {
        Ok(each) => each.into_iter().filter(|c| *c).count(),
        Err(_) => 0,
    };
    // A revocation still waiting when time ran out: its tokens go anyway.
    for id in &ids {
        oauth::forget(id);
        super::tell(id);
    }
    for id in &left {
        let _ = vault::update(id, |s| s.grant = None);
    }
    if !ids.is_empty() || !left.is_empty() {
        super::changed();
    }
    let signed_out = ids.len() + left.len();
    tracing::info!(signed_out, left = left.len(), confirmed, "connectors: every sign-in ended");
    SignedOut { signed_out, confirmed }
}

/// A link without what can carry a secret: its user name and password, its
/// query and its fragment. Pure.
fn bare_link(s: &str) -> String {
    match url::Url::parse(s.trim()) {
        Ok(mut u) => {
            u.set_query(None);
            u.set_fragment(None);
            let _ = u.set_username("");
            let _ = u.set_password(None);
            u.to_string()
        }
        Err(_) => "(an address Yap couldn't read)".to_string(),
    }
}

/// A word a person typed into a command line or a variable, or "(left out:
/// it looks like a key)" when it has a key's shape (`diagnostics`' shapes).
/// Pure.
fn plain(s: &str) -> String {
    if crate::privacy::diagnostics::PREFIXED_KEY.is_match(s) {
        "(left out: it looks like a key)".to_string()
    } else {
        s.to_string()
    }
}

fn kind_word(k: cache::Kind) -> &'static str {
    match k {
        cache::Kind::Reads => "reads",
        cache::Kind::Changes => "changes things",
        cache::Kind::Deletes => "can delete",
    }
}

/// One connector for the export: an allow-list of what connectors.json
/// holds (and its tools' kinds, as last listed). Never a vault value: the
/// vault isn't read. Pure but for the tool list's cache.
pub fn export_one(c: &Connector) -> Value {
    let reached = match &c.transport {
        Transport::Remote { url, fallback_sse, sse, headers } => json!({
            "kind": "link",
            "address": bare_link(url),
            "olderAddress": fallback_sse.as_deref().map(bare_link),
            "olderHttpSse": sse,
            // Their values are in Windows' Credential Manager, never here.
            "headerNames": headers.iter().map(|h| h.name.clone()).collect::<Vec<_>>(),
        }),
        Transport::Local { command, resolved, args, env, .. } => json!({
            "kind": "program",
            "command": plain(command),
            "resolved": plain(resolved),
            "args": args.iter().map(|a| plain(a)).collect::<Vec<_>>(),
            "variables": env
                .iter()
                .map(|v| match (&v.value, v.secret) {
                    (Some(value), false) => json!({ "name": v.name, "value": plain(value) }),
                    _ => json!({ "name": v.name, "value": "(kept in Windows Credential Manager)" }),
                })
                .collect::<Vec<_>>(),
        }),
    };
    let signs_in = match c.auth.kind {
        AuthKind::None => json!({ "how": "nothing" }),
        AuthKind::Bearer => json!({ "how": "a pasted token (kept in Windows Credential Manager)" }),
        AuthKind::Header => json!({ "how": "request headers (their values kept in Windows Credential Manager)" }),
        AuthKind::Oauth => json!({
            "how": "in the browser",
            "signedIn": c.auth.signed_in,
            "ended": c.auth.expired,
            "server": c.auth.issuer.as_deref().map(bare_link),
            "scopes": c.auth.scopes,
        }),
    };
    let listed = cache::get(&c.id);
    let kind_of = |tool: &str| listed.as_ref().and_then(|l| l.tools.iter().find(|t| t.name == tool)).map(|t| kind_word(t.kind));
    let tools: Vec<Value> = c.tools.iter().map(|(name, s)| json!({ "name": name, "setting": s.setting, "kind": kind_of(name) })).collect();
    json!({
        "id": c.id,
        "name": c.name,
        "on": c.enabled,
        "from": c.origin.kind,
        "catalogEntry": c.origin.extra.get("catalog"),
        "reached": reached,
        "signsIn": signs_in,
        "usedIn": c.where_,
        "tools": tools,
        "added": c.added,
        "lastUsed": c.last_used,
    })
}

/// **Download your data's** `connectors/connectors.json`: every connector
/// ([`export_one`]) — while the preview is on, or while any is kept with it
/// off (what's stored, not what's switched on); else `None` (no file).
pub fn export() -> Option<Value> {
    let all = store::list();
    if !enabled() && all.is_empty() {
        return None;
    }
    let list: Vec<Value> = all.iter().map(export_one).collect();
    Some(json!({
        "note": "Your connectors and their tools' settings. Tokens, secrets and header values stay in Windows Credential Manager and aren't in this file.",
        "connectors": list,
    }))
}

/// Whether a status is a failing one (Report a problem's count).
fn failing(s: &Status) -> bool {
    matches!(s, Status::Failed { .. } | Status::Stopped)
}

/// Report a problem's counts, from the connectors and their statuses. Pure.
pub fn counts_of(list: &[Connector], status: &dyn Fn(&str) -> Status) -> Value {
    let n = |f: &dyn Fn(&Connector) -> bool| list.iter().filter(|c| f(c)).count();
    json!({
        "count": list.len(),
        "links": n(&|c| !c.transport.is_local()),
        "programs": n(&|c| c.transport.is_local()),
        "off": n(&|c| !c.enabled),
        "signInInBrowser": n(&|c| c.auth.kind == AuthKind::Oauth),
        "signedIn": n(&|c| c.auth.kind == AuthKind::Oauth && c.auth.signed_in),
        "signInEnded": n(&|c| c.auth.kind == AuthKind::Oauth && c.auth.expired),
        "withTokenOrHeaders": n(&|c| matches!(c.auth.kind, AuthKind::Bearer | AuthKind::Header)),
        "fromCatalogue": n(&|c| c.origin.kind == "catalog"),
        "failing": n(&|c| failing(&status(&c.id))),
    })
}

/// **Report a problem's** connectors: [`counts_of`] this run's — while the
/// preview is on, or while any is kept with it off; else `None`.
pub fn counts() -> Option<Value> {
    let all = store::list();
    if !enabled() && all.is_empty() {
        return None;
    }
    Some(counts_of(&all, &super::status_if_started))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::super::store::{EnvVar, Header, New, Setting, ToolSetting};
    use super::super::vault::{Grant, Secrets};
    use super::*;

    // Made-up secrets only.
    const SECRETS: &[&str] = &[
        "ghp_FAKEexport0123456789",
        "hdr-FAKE-value-0123",
        "env-FAKE-secret-9876",
        "at-FAKE-access-1111",
        "rt-FAKE-refresh-2222",
        "cs-FAKE-registered-3333",
        "cs-FAKE-own-4444",
        "qk-FAKE-query-5555",
        "sk-FAKEpastedINanARG0123456789",
    ];

    fn link(name: &str, url: &str, auth: AuthKind, headers: &[&str]) -> Connector {
        store::add(New {
            name: name.into(),
            transport: Transport::Remote { url: url.into(), fallback_sse: None, sse: false, headers: headers.iter().map(|h| Header { name: (*h).into() }).collect() },
            auth,
        })
        .unwrap()
    }

    /// A link with a token and a header, one signed in in the browser, and a
    /// program with a secret variable and a key pasted into its arguments.
    fn seeded() -> Vec<Connector> {
        super::super::switch_for_test(true);
        let token = link("Issue tracker", &format!("https://robin:pw@mcp.example.com/mcp?key={}#frag", SECRETS[7]), AuthKind::Header, &["X-Api-Key"]);
        vault::save(&token.id, &Secrets { token: Some(SECRETS[0].into()), headers: BTreeMap::from([("X-Api-Key".into(), SECRETS[1].into())]), ..Default::default() }).unwrap();
        let browser = link("Robin's notes", "https://notes.example.com/mcp", AuthKind::Oauth, &[]);
        store::update(&browser.id, |c| {
            c.auth.signed_in = true;
            c.auth.issuer = Some("https://auth.example.com".into());
            c.auth.scopes = vec!["read".into()];
            c.tools.insert("search".into(), ToolSetting { setting: Setting::Allow, hash: "1a".into() });
            c.tools.insert("create_page".into(), ToolSetting { setting: Setting::Ask, hash: "2b".into() });
        })
        .unwrap();
        vault::save(
            &browser.id,
            &Secrets {
                grant: Some(Grant {
                    access: SECRETS[3].into(),
                    refresh: Some(SECRETS[4].into()),
                    issuer: "https://auth.example.com".into(),
                    resource: "https://notes.example.com/mcp".into(),
                    client_id: "dcr-1".into(),
                    ..Default::default()
                }),
                registered_secret: Some(SECRETS[5].into()),
                own_secret: Some(SECRETS[6].into()),
                ..Default::default()
            },
        )
        .unwrap();
        let program = store::add(New {
            name: "Files".into(),
            transport: Transport::Local {
                command: "npx".into(),
                resolved: "C:\\Program Files\\nodejs\\npx.cmd".into(),
                args: vec!["-y".into(), "@modelcontextprotocol/server-filesystem".into(), "--api-key".into(), SECRETS[8].into()],
                env: vec![EnvVar { name: "GITHUB_TOKEN".into(), value: None, secret: true }, EnvVar { name: "LOG".into(), value: Some("1".into()), secret: false }],
                approved: "abc".into(),
            },
            auth: AuthKind::None,
        })
        .unwrap();
        vault::save(&program.id, &Secrets { env: BTreeMap::from([("GITHUB_TOKEN".into(), SECRETS[2].into())]), ..Default::default() }).unwrap();
        store::list()
    }

    #[test]
    fn the_export_lists_the_connectors_and_their_settings_and_no_vault_value_reaches_the_zip() {
        let list = seeded();
        let v = export().expect("the preview is on");
        let text = v.to_string();
        for s in SECRETS {
            assert!(!text.contains(s), "{s} in {text}");
        }
        let cs = v["connectors"].as_array().unwrap();
        assert_eq!(cs.len(), list.len());
        // A link: its address without its query, user name or fragment; its header's name.
        assert_eq!(cs[0]["reached"]["address"], "https://mcp.example.com/mcp");
        assert_eq!(cs[0]["reached"]["headerNames"], json!(["X-Api-Key"]));
        assert!(!text.contains("robin:pw") && !text.contains("#frag"));
        // The browser sign-in: signed in, its server and scopes; its tools' settings.
        assert_eq!(cs[1]["signsIn"]["how"], "in the browser");
        assert_eq!(cs[1]["signsIn"]["signedIn"], true);
        assert_eq!(cs[1]["tools"], json!([{ "name": "create_page", "setting": "ask", "kind": null }, { "name": "search", "setting": "allow", "kind": null }]));
        assert_eq!(cs[1]["name"], "Robin's notes", "the person's own name for it: it's their data");
        // A program: its command line, a plain variable's value, a secret one's name only.
        assert_eq!(cs[2]["reached"]["command"], "npx");
        assert_eq!(cs[2]["reached"]["args"][3], "(left out: it looks like a key)");
        assert_eq!(cs[2]["reached"]["variables"][0], json!({ "name": "GITHUB_TOKEN", "value": "(kept in Windows Credential Manager)" }));
        assert_eq!(cs[2]["reached"]["variables"][1], json!({ "name": "LOG", "value": "1" }));
        // Through Download your data's own file: nowhere in any of its files.
        let files = crate::privacy::export::connectors_files(Some(&v));
        assert_eq!(files.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(), ["connectors/connectors.json"]);
        for (name, bytes) in &files {
            let t = String::from_utf8_lossy(bytes);
            for s in SECRETS {
                assert!(!t.contains(s), "{s} in {name}");
            }
        }
        // And the export never reads the vault at all: neither its part here
        // nor Download your data's own code.
        assert!(!between(include_str!("privacy.rs"), "pub fn export_one", "fn failing(").contains("vault"), "the connectors' export reads the vault");
        let export_rs = include_str!("../../privacy/export.rs").split("#[cfg(test)]").next().unwrap();
        assert!(!export_rs.contains("vault::"), "Download your data reads the vault");
    }

    /// `src` from `from` to `to`.
    fn between<'a>(src: &'a str, from: &str, to: &str) -> &'a str {
        let a = src.find(from).unwrap_or(0);
        let b = src[a..].find(to).map_or(src.len(), |b| a + b);
        &src[a..b]
    }

    #[test]
    fn report_a_problem_counts_them_and_never_names_one() {
        let list = seeded();
        let status = |id: &str| if id == "c3" { Status::Stopped } else { Status::Idle };
        let v = counts_of(&list, &status);
        assert_eq!(
            v,
            json!({ "count": 3, "links": 2, "programs": 1, "off": 0, "signInInBrowser": 1, "signedIn": 1, "signInEnded": 0, "withTokenOrHeaders": 1, "fromCatalogue": 0, "failing": 1 })
        );
        let text = v.to_string();
        for word in ["Issue tracker", "Robin", "Files", "example.com", "npx", "X-Api-Key", "GITHUB_TOKEN", "search"] {
            assert!(!text.contains(word), "{word} in {text}");
        }
        // The preview off: what's stored is still counted (and exported).
        super::super::tests::set_switch(false);
        assert_eq!(counts().unwrap()["count"], 3);
        assert_eq!(export().unwrap()["connectors"].as_array().unwrap().len(), 3);
        // Nothing stored and the preview off: nothing at all.
        for c in &list {
            vault::delete(&c.id);
        }
        super::super::switch_for_test(false);
        assert_eq!(counts(), None);
        assert_eq!(export(), None);
        assert_eq!(sign_ins(), 0);
        assert!(!any_kept());
    }

    /// Someone tried the preview and turned it off: Reset app still ends
    /// their sign-ins (and Delete some of your data counts them) — also one
    /// left in the vault for a connector that's gone.
    #[tokio::test]
    async fn with_the_preview_off_a_stored_sign_in_is_still_counted_and_ended() {
        let list = seeded();
        super::super::tests::set_switch(false);
        assert!(!enabled());
        assert_eq!(sign_ins(), 1);
        assert!(any_kept());
        // A sign-in left behind by a connector that's gone (its file lost).
        let gone = Secrets { grant: Some(Grant { access: "at-FAKE-left-7777".into(), issuer: "https://auth.example.com".into(), ..Default::default() }), ..Default::default() };
        vault::save("c9", &gone).unwrap();
        assert_eq!(sign_ins(), 2);
        let done = sign_out_all().await;
        assert_eq!(done, SignedOut { signed_out: 2, confirmed: 0 });
        assert_eq!(sign_ins(), 0);
        assert!(vault::load(&list[1].id).grant.is_none() && vault::load("c9").grant.is_none(), "both sign-ins' tokens gone");
        assert_eq!(vault::load(&list[0].id).token.as_deref(), Some(SECRETS[0]), "a pasted token stays");
        assert!(!store::get(&list[1].id).unwrap().auth.signed_in);
        assert_eq!(store::list().len(), 3, "the connectors stay");
        vault::delete("c9");
    }

    #[tokio::test]
    async fn signing_every_connector_out_keeps_the_connectors_and_their_tokens() {
        let list = seeded();
        assert_eq!(sign_ins(), 1);
        let done = sign_out_all().await;
        // The seeded sign-in has no revocation endpoint: ended here, not confirmed.
        assert_eq!(done, SignedOut { signed_out: 1, confirmed: 0 });
        assert_eq!(sign_ins(), 0);
        assert_eq!(store::list().len(), list.len(), "the connectors stay");
        let browser = &list[1].id;
        let kept = vault::load(browser);
        assert!(kept.grant.is_none(), "the sign-in's tokens are gone");
        assert_eq!(kept.own_secret.as_deref(), Some(SECRETS[6]), "the person's own client stays");
        assert_eq!(kept.registered_secret.as_deref(), Some(SECRETS[5]), "Yap's registration stays");
        let c = store::get(browser).unwrap();
        assert!(!c.auth.signed_in && c.auth.scopes.is_empty());
        assert_eq!(c.tools.len(), 2, "its tools' settings stay");
        assert_eq!(vault::load(&list[0].id).token.as_deref(), Some(SECRETS[0]), "a pasted token stays");
        assert_eq!(vault::load(&list[2].id).env.get("GITHUB_TOKEN").map(String::as_str), Some(SECRETS[2]), "a program's secret variable stays");
        // Again: nothing to do.
        assert_eq!(sign_out_all().await, SignedOut::default());
        // The preview off and no sign-in stored: nothing changes either.
        super::super::switch_for_test(false);
        assert_eq!(sign_out_all().await, SignedOut::default());
        assert_eq!(vault::load(&list[0].id).token.as_deref(), Some(SECRETS[0]));
    }

    #[test]
    fn links_lose_what_can_carry_a_secret() {
        assert_eq!(bare_link("https://u:p@mcp.example.com/a/b?key=1#x"), "https://mcp.example.com/a/b");
        assert_eq!(bare_link("not a link"), "(an address Yap couldn't read)");
        assert_eq!(plain("--verbose"), "--verbose");
        assert_eq!(plain("gsk_FAKE0123456789abcdef"), "(left out: it looks like a key)");
    }
}
