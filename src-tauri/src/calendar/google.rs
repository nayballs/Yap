//! Google Calendar in one click, straight from this PC: OAuth 2.0 for
//! installed apps (RFC 8252) in the system browser with PKCE (S256), the
//! code handed back to a one-shot loopback listener on 127.0.0.1, then the
//! Calendar API read directly. No Yap server is involved.
//!
//! **Scopes**, the smallest pair that lists your meetings with their
//! attendees and conference links:
//! - `calendar.calendarlist.readonly` ("See the list of Google calendars
//!   you're subscribed to"): which calendars to read, and the account's
//!   address (the primary calendar's id), so no profile or email scope;
//! - `calendar.events.owned.readonly` ("See the events on Google calendars
//!   you own"): invitations land on your own calendars, so colleagues' or
//!   holiday calendars you merely subscribe to stay out of reach.
//!
//! **The client** is a Google Cloud "Desktop app" OAuth client whose id and
//! secret are baked in at build time (`YAP_GOOGLE_CALENDAR_CLIENT_ID` /
//! `YAP_GOOGLE_CALENDAR_CLIENT_SECRET`; Google treats an installed app's
//! secret as public, but it stays out of the repo). Without them the build
//! can't offer Google (`client()` is `None`). Debug builds also read both at
//! run time, and `YAP_GOOGLE_{AUTH,TOKEN,REVOKE,API}_URL` point them at a
//! local fake (the e2e suite's `support/fake-google.js`).

use std::collections::HashMap;
use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;

use chrono::DateTime;
use once_cell::sync::Lazy;
use serde_json::Value;
use tiny_http::{Header, Response, Server};

use super::model::{Draft, Person, Reply};

/// The read-only scopes Yap asks for (see the module docs).
pub const SCOPES: &str = "https://www.googleapis.com/auth/calendar.calendarlist.readonly \
                          https://www.googleapis.com/auth/calendar.events.owned.readonly";
const EVENTS_SCOPE: &str = "https://www.googleapis.com/auth/calendar.events.owned.readonly";
const LIST_SCOPE: &str = "https://www.googleapis.com/auth/calendar.calendarlist.readonly";

/// How long Yap waits for the browser sign-in before giving up.
pub const SIGN_IN_TTL: Duration = Duration::from_secs(10 * 60);
/// Calendars read per account, and result pages per calendar.
const MAX_CALENDARS: usize = 10;
const MAX_PAGES: usize = 4;

/// This build's OAuth client.
#[derive(Debug, Clone)]
pub struct Client {
    pub id: String,
    pub secret: Option<String>,
}

/// The OAuth client, if this build has one (see the module docs).
pub fn client() -> Option<Client> {
    let clean = |s: &str| Some(s.trim().to_string()).filter(|s| !s.is_empty());
    #[cfg(debug_assertions)]
    if let Some(id) = std::env::var("YAP_GOOGLE_CALENDAR_CLIENT_ID").ok().and_then(|v| clean(&v)) {
        let secret = std::env::var("YAP_GOOGLE_CALENDAR_CLIENT_SECRET").ok().and_then(|v| clean(&v));
        return Some(Client { id, secret });
    }
    let id = option_env!("YAP_GOOGLE_CALENDAR_CLIENT_ID").and_then(clean)?;
    let secret = option_env!("YAP_GOOGLE_CALENDAR_CLIENT_SECRET").and_then(clean);
    Some(Client { id, secret })
}

struct Endpoints {
    auth: String,
    token: String,
    revoke: String,
    api: String,
}

fn endpoints() -> Endpoints {
    let pick = |var: &str, default: &str| -> String {
        #[cfg(debug_assertions)]
        if let Ok(v) = std::env::var(var) {
            let v = v.trim().trim_end_matches('/');
            if !v.is_empty() {
                return v.to_string();
            }
        }
        let _ = var;
        default.to_string()
    };
    Endpoints {
        auth: pick("YAP_GOOGLE_AUTH_URL", "https://accounts.google.com/o/oauth2/v2/auth"),
        token: pick("YAP_GOOGLE_TOKEN_URL", "https://oauth2.googleapis.com/token"),
        revoke: pick("YAP_GOOGLE_REVOKE_URL", "https://oauth2.googleapis.com/revoke"),
        api: pick("YAP_GOOGLE_API_URL", "https://www.googleapis.com/calendar/v3"),
    }
}

static HTTP: Lazy<reqwest::Client> = Lazy::new(|| {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .user_agent(format!("Yap/{} (Windows)", env!("CARGO_PKG_VERSION")))
        .build()
        .unwrap_or_default()
});

// ---- sign-in ----------------------------------------------------------------------------------

/// What the browser handed back to the loopback listener.
pub enum Handback {
    /// A code to exchange, and where to tell the browser how it went (the
    /// account connected, or why not).
    Code(String, mpsc::Sender<Result<String, String>>),
    /// The person said no (or Google refused): Google's error code.
    Denied(String),
}

/// A sign-in in flight. Dropping it stops the loopback listener.
pub struct SignIn {
    pub url: String,
    pub verifier: String,
    pub redirect_uri: String,
    pub handback: mpsc::Receiver<Handback>,
    server: Arc<Server>,
}

impl Drop for SignIn {
    fn drop(&mut self) {
        self.server.unblock();
    }
}

/// Start a sign-in: a loopback listener on a free port, and the consent
/// page's address for the browser.
pub fn begin(client: &Client) -> Result<SignIn, String> {
    let server = Server::http("127.0.0.1:0")
        .map(Arc::new)
        .map_err(|e| format!("Couldn't start the sign-in listener: {e}"))?;
    let port = server.server_addr().to_ip().map(|a| a.port()).ok_or("Couldn't start the sign-in listener.")?;
    let redirect_uri = format!("http://127.0.0.1:{port}");
    let state = crate::auth::random_alnum(24)?;
    let (verifier, challenge) = crate::auth::pkce_pair()?;

    let mut url = url::Url::parse(&endpoints().auth).map_err(|e| e.to_string())?;
    url.query_pairs_mut()
        .append_pair("client_id", &client.id)
        .append_pair("redirect_uri", &redirect_uri)
        .append_pair("response_type", "code")
        .append_pair("scope", SCOPES)
        .append_pair("code_challenge", &challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("state", &state)
        // A refresh token, every time (a reconnect after a disconnect too).
        .append_pair("access_type", "offline")
        .append_pair("prompt", "consent");

    let (tx, rx) = mpsc::channel();
    let srv = server.clone();
    std::thread::Builder::new()
        .name("yap-calendar-loopback".into())
        .spawn(move || {
            for request in srv.incoming_requests() {
                if handle(request, &state, &tx) {
                    break;
                }
            }
        })
        .map_err(|e| e.to_string())?;
    Ok(SignIn { url: url.to_string(), verifier, redirect_uri, handback: rx, server })
}

/// One request to the loopback listener; `true` once the sign-in is over.
fn handle(request: tiny_http::Request, state: &str, tx: &mpsc::Sender<Handback>) -> bool {
    let from_loopback = request.remote_addr().is_some_and(|a| a.ip().is_loopback());
    let query = request.url().split_once('?').map(|(_, q)| q.to_string()).unwrap_or_default();
    let params: HashMap<String, String> = url::form_urlencoded::parse(query.as_bytes()).into_owned().collect();
    let ours = params.get("state").is_some_and(|s| crate::auth::same_secret(s, state));
    if !from_loopback || !ours {
        let _ = request.respond(Response::empty(404));
        return false;
    }
    if let Some(error) = params.get("error") {
        let _ = request.respond(page(false, "Google Calendar wasn't connected. You can close this tab."));
        let _ = tx.send(Handback::Denied(error.clone()));
        return true;
    }
    let Some(code) = params.get("code").filter(|c| !c.is_empty()) else {
        let _ = request.respond(Response::empty(400));
        return false;
    };
    // Tell the browser how it went once Yap has exchanged the code.
    let (done_tx, done_rx) = mpsc::channel();
    if tx.send(Handback::Code(code.clone(), done_tx)).is_err() {
        let _ = request.respond(page(false, "Yap stopped waiting for this sign-in. Start again from Yap."));
        return true;
    }
    let reply = match done_rx.recv_timeout(Duration::from_secs(45)) {
        Ok(Ok(account)) => page(true, &format!("Yap can now see the meetings on {account}. You can close this tab.")),
        Ok(Err(e)) => page(false, &e),
        Err(_) => page(true, "Yap is finishing the connection. You can close this tab."),
    };
    let _ = request.respond(reply);
    true
}

/// The little page the browser shows after the consent screen.
fn page(ok: bool, message: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    let title = if ok { "Calendar connected" } else { "Not connected" };
    let html = format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><title>Yap · {title}</title>\
         <meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"></head>\
         <body style=\"margin:0;min-height:100vh;display:flex;align-items:center;justify-content:center;\
         background:#f6f3ee;color:#26231c;font:16px/1.5 'Segoe UI',system-ui,sans-serif\">\
         <main style=\"max-width:420px;padding:32px;border-radius:18px;background:#fff;\
         box-shadow:0 8px 30px rgba(38,35,28,.08);text-align:center\">\
         <h1 style=\"margin:0 0 8px;font:600 26px Georgia,serif\">{title}</h1><p style=\"margin:0;color:#6b6457\">{}</p>\
         </main></body></html>",
        escape_html(message),
    );
    Response::from_data(html.into_bytes())
        .with_header(Header::from_bytes(&b"Content-Type"[..], &b"text/html; charset=utf-8"[..]).expect("static header"))
        .with_header(Header::from_bytes(&b"Cache-Control"[..], &b"no-store"[..]).expect("static header"))
}

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// What the token endpoint returned.
pub struct Tokens {
    pub access: String,
    pub expires_in: i64,
    pub refresh: Option<String>,
    /// The scopes granted (the consent screen lets people untick one).
    pub scope: String,
}

impl Tokens {
    pub fn can_read_events(&self) -> bool {
        self.scope.split_whitespace().any(|s| s == EVENTS_SCOPE)
    }
    pub fn can_list_calendars(&self) -> bool {
        self.scope.split_whitespace().any(|s| s == LIST_SCOPE)
    }
}

/// Why a Google call failed.
#[derive(Debug, PartialEq)]
pub enum Failure {
    /// Couldn't reach Google.
    Offline,
    /// Access was removed or expired: connect again (`invalid_grant`).
    Revoked,
    /// The access token wasn't accepted (refresh and try again).
    Unauthorized,
    /// Anything else: the status code.
    Http(u16),
}

impl Failure {
    pub fn message(&self) -> String {
        match self {
            Failure::Offline => "Couldn't reach Google. Yap tries again in a few minutes.".into(),
            Failure::Revoked | Failure::Unauthorized => {
                "Google ended Yap's access to this calendar. Connect it again.".into()
            }
            Failure::Http(code) => format!("Google Calendar had a problem (error {code}). Yap tries again in a few minutes."),
        }
    }
}

/// Swap the code for tokens (with the PKCE verifier).
pub async fn exchange(client: &Client, code: &str, verifier: &str, redirect_uri: &str) -> Result<Tokens, Failure> {
    let mut form = vec![
        ("grant_type", "authorization_code"),
        ("code", code),
        ("code_verifier", verifier),
        ("redirect_uri", redirect_uri),
        ("client_id", client.id.as_str()),
    ];
    if let Some(secret) = &client.secret {
        form.push(("client_secret", secret));
    }
    token_call(&form).await
}

/// A fresh access token from the refresh token.
pub async fn refresh(client: &Client, refresh_token: &str) -> Result<Tokens, Failure> {
    let mut form = vec![
        ("grant_type", "refresh_token"),
        ("refresh_token", refresh_token),
        ("client_id", client.id.as_str()),
    ];
    if let Some(secret) = &client.secret {
        form.push(("client_secret", secret));
    }
    token_call(&form).await
}

async fn token_call(form: &[(&str, &str)]) -> Result<Tokens, Failure> {
    let resp = HTTP.post(endpoints().token).form(form).send().await.map_err(|_| Failure::Offline)?;
    let status = resp.status().as_u16();
    // Bodies hold tokens: never logged.
    let body: Value = resp.json().await.unwrap_or(Value::Null);
    if status != 200 {
        let error = body.get("error").and_then(Value::as_str).unwrap_or("");
        tracing::warn!(status, error, "calendar: Google token endpoint said no");
        return Err(if error == "invalid_grant" { Failure::Revoked } else { Failure::Http(status) });
    }
    let text = |k: &str| body.get(k).and_then(Value::as_str).map(str::to_string);
    Ok(Tokens {
        access: text("access_token").ok_or(Failure::Http(status))?,
        expires_in: body.get("expires_in").and_then(Value::as_i64).unwrap_or(3_600),
        refresh: text("refresh_token"),
        scope: text("scope").unwrap_or_default(),
    })
}

/// Let go of the access (a disconnect). Best effort.
pub async fn revoke(token: &str) {
    let _ = HTTP.post(endpoints().revoke).form(&[("token", token)]).send().await;
}

async fn get(access: &str, url: url::Url) -> Result<Value, Failure> {
    let resp = HTTP.get(url).bearer_auth(access).send().await.map_err(|_| Failure::Offline)?;
    match resp.status().as_u16() {
        200 => resp.json().await.map_err(|_| Failure::Http(200)),
        401 => Err(Failure::Unauthorized),
        status => Err(Failure::Http(status)),
    }
}

fn api_url(path: &str) -> Result<url::Url, Failure> {
    url::Url::parse(&format!("{}{path}", endpoints().api)).map_err(|_| Failure::Http(0))
}

/// The account's address and the calendars to read: the ones you own that
/// are shown in Google Calendar (and always the primary one).
pub async fn calendars(access: &str, can_list: bool) -> Result<(String, Vec<String>), Failure> {
    if can_list {
        let mut url = api_url("/users/me/calendarList")?;
        url.query_pairs_mut()
            .append_pair("minAccessRole", "owner")
            .append_pair("maxResults", "50")
            .append_pair("fields", "items(id,primary,selected)");
        let body = get(access, url).await?;
        let items = body.get("items").and_then(Value::as_array).cloned().unwrap_or_default();
        let flag = |c: &Value, k: &str| c.get(k).and_then(Value::as_bool).unwrap_or(false);
        let id = |c: &Value| c.get("id").and_then(Value::as_str).unwrap_or("").to_string();
        let account = items.iter().find(|c| flag(c, "primary")).map(id).unwrap_or_default();
        let mut ids: Vec<String> = items
            .iter()
            .filter(|c| flag(c, "primary") || flag(c, "selected"))
            .map(id)
            .filter(|i| !i.is_empty())
            .collect();
        ids.truncate(MAX_CALENDARS);
        if !ids.is_empty() {
            return Ok((account, ids));
        }
    }
    // Without the calendar list (unticked on the consent screen): the
    // primary calendar, whose title is the account's address.
    let mut url = api_url("/calendars/primary/events")?;
    url.query_pairs_mut().append_pair("maxResults", "1").append_pair("fields", "summary");
    let body = get(access, url).await?;
    let account = body.get("summary").and_then(Value::as_str).unwrap_or("").to_string();
    Ok((account, vec!["primary".to_string()]))
}

/// The fields Yap reads from an event (and nothing else).
const EVENT_FIELDS: &str = "items(id,status,eventType,summary,description,location,start,end,hangoutLink,\
    attendees(email,displayName,self,resource,responseStatus),organizer(email,displayName,self),\
    conferenceData(entryPoints(entryPointType,uri))),nextPageToken";

/// One calendar's events between `from` and `to` (unix seconds), each
/// recurring meeting expanded into its occurrences by Google.
pub async fn events(access: &str, calendar: &str, from: i64, to: i64) -> Result<Vec<Value>, Failure> {
    let rfc3339 = |t: i64| {
        DateTime::from_timestamp(t, 0)
            .unwrap_or_default()
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
    };
    let mut out = Vec::new();
    let mut page: Option<String> = None;
    for _ in 0..MAX_PAGES {
        let mut url = api_url("/calendars/")?;
        url.path_segments_mut().map_err(|_| Failure::Http(0))?.push(calendar).push("events");
        url.query_pairs_mut()
            .append_pair("singleEvents", "true")
            .append_pair("orderBy", "startTime")
            .append_pair("timeMin", &rfc3339(from))
            .append_pair("timeMax", &rfc3339(to))
            .append_pair("maxResults", "250")
            .append_pair("fields", EVENT_FIELDS);
        if let Some(token) = &page {
            url.query_pairs_mut().append_pair("pageToken", token);
        }
        let body = get(access, url).await?;
        out.extend(body.get("items").and_then(Value::as_array).cloned().unwrap_or_default());
        page = body.get("nextPageToken").and_then(Value::as_str).map(str::to_string);
        if page.is_none() {
            break;
        }
    }
    Ok(out)
}

/// A Google event as a draft (see `model::finish`). All-day events, Google's
/// out-of-office, focus-time, working-location and birthday entries, and
/// cancelled occurrences aren't meetings: `None`.
pub fn draft_from_item(connection: &str, item: &Value) -> Option<Draft> {
    let text = |v: &Value, k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    if text(item, "status") == "cancelled" {
        return None;
    }
    if matches!(text(item, "eventType").as_str(), "outOfOffice" | "focusTime" | "workingLocation" | "birthday") {
        return None;
    }
    let id = text(item, "id");
    if id.is_empty() {
        return None;
    }
    let start_text = item.get("start")?.get("dateTime")?.as_str()?; // a "date": all-day
    let start = DateTime::parse_from_rfc3339(start_text).ok()?.timestamp();
    let end = item
        .get("end")
        .and_then(|e| e.get("dateTime"))
        .and_then(Value::as_str)
        .and_then(|e| DateTime::parse_from_rfc3339(e).ok())
        .map_or(start, |e| e.timestamp());
    let flag = |v: &Value, k: &str| v.get(k).and_then(Value::as_bool).unwrap_or(false);
    let person = |p: &Value| Person {
        name: text(p, "displayName"),
        email: text(p, "email").to_ascii_lowercase(),
        is_self: flag(p, "self"),
        resource: flag(p, "resource"),
        reply: Reply::parse(&text(p, "responseStatus")),
    };
    let mut link_sources: Vec<String> = item
        .pointer("/conferenceData/entryPoints")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|p| text(p, "entryPointType") == "video")
        .map(|p| text(p, "uri"))
        .collect();
    link_sources.push(text(item, "hangoutLink"));
    link_sources.push(text(item, "location"));
    Some(Draft {
        key: format!("{connection}:{id}"),
        connection: connection.to_string(),
        title: text(item, "summary"),
        start,
        end,
        people: item.get("attendees").and_then(Value::as_array).map(|a| a.iter().map(person).collect()).unwrap_or_default(),
        organizer: item.get("organizer").map(person),
        link_sources,
        description: text(item, "description"),
        cancelled: false,
    })
}

#[cfg(test)]
mod tests {
    use super::super::model::finish;
    use super::*;
    use serde_json::json;

    fn item(extra: Value) -> Value {
        let mut base = json!({
            "id": "abc_20261005T140000Z",
            "status": "confirmed",
            "summary": "Roadmap review",
            "start": { "dateTime": "2026-10-05T15:00:00+01:00" },
            "end": { "dateTime": "2026-10-05T15:30:00+01:00" },
            "attendees": [
                { "email": "Tester@Example.com", "self": true, "responseStatus": "accepted" },
                { "email": "alex.chen@example.com", "displayName": "Alex Chen", "responseStatus": "needsAction" },
                { "email": "room@resource.calendar.google.com", "displayName": "Room 4", "resource": true }
            ],
            "hangoutLink": "https://meet.google.com/abc-defg-hij",
            "description": "<b>Agenda</b><br>Q4 roadmap"
        });
        if let (Some(b), Some(e)) = (base.as_object_mut(), extra.as_object()) {
            for (k, v) in e {
                b.insert(k.clone(), v.clone());
            }
        }
        base
    }

    #[test]
    fn a_google_event_becomes_a_meeting() {
        let ev = finish(draft_from_item("1", &item(json!({}))).unwrap()).unwrap();
        assert_eq!(ev.key, "1:abc_20261005T140000Z");
        assert_eq!(ev.start, DateTime::parse_from_rfc3339("2026-10-05T14:00:00Z").unwrap().timestamp());
        assert_eq!(ev.end - ev.start, 1_800);
        assert_eq!(ev.attendees.iter().map(|a| a.name.as_str()).collect::<Vec<_>>(), ["Alex Chen"]);
        assert_eq!(ev.service.as_deref(), Some("meet"));
        assert_eq!(ev.description, "Agenda\nQ4 roadmap");
    }

    #[test]
    fn conference_data_wins_over_the_location() {
        let ev = item(json!({
            "hangoutLink": "",
            "location": "https://zoom.us/j/999",
            "conferenceData": { "entryPoints": [
                { "entryPointType": "phone", "uri": "tel:+1-555" },
                { "entryPointType": "video", "uri": "https://teams.microsoft.com/l/meetup-join/19%3a1" }
            ]}
        }));
        let ev = finish(draft_from_item("1", &ev).unwrap()).unwrap();
        assert_eq!(ev.service.as_deref(), Some("teams"));
    }

    #[test]
    fn all_day_out_of_office_cancelled_and_declined_events_are_not_meetings() {
        assert!(draft_from_item("1", &item(json!({ "start": { "date": "2026-10-05" }, "end": { "date": "2026-10-06" } }))).is_none());
        assert!(draft_from_item("1", &item(json!({ "eventType": "outOfOffice" }))).is_none());
        assert!(draft_from_item("1", &item(json!({ "status": "cancelled" }))).is_none());
        let declined = item(json!({ "attendees": [
            { "email": "tester@example.com", "self": true, "responseStatus": "declined" },
            { "email": "alex@example.com" }
        ]}));
        assert!(finish(draft_from_item("1", &declined).unwrap()).is_none());
        // Just you, no link: not a meeting.
        let solo = item(json!({ "attendees": [], "hangoutLink": "" }));
        assert!(finish(draft_from_item("1", &solo).unwrap()).is_none());
    }

    #[test]
    fn the_scopes_are_the_two_read_only_ones() {
        let scopes: Vec<&str> = SCOPES.split_whitespace().collect();
        assert_eq!(scopes, [LIST_SCOPE, EVENTS_SCOPE]);
        assert!(scopes.iter().all(|s| s.ends_with(".readonly")));
        let granted = Tokens { access: String::new(), expires_in: 0, refresh: None, scope: EVENTS_SCOPE.into() };
        assert!(granted.can_read_events() && !granted.can_list_calendars());
    }
}
