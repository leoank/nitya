//! The Google Calendar driver (ADR-0017, ADR-0018): one channel is one calendar, its
//! items one per Google event, kept as iCalendar with Google's event id and ETag.
//!
//! Reading follows EXP-20261127-mqzh92: `events.list` with `singleEvents=false` lists
//! masters, exceptions and cancelled instances, ending in a sync token that is the
//! channel's cursor; a 410 asks for a full sync; a cancelled event with no
//! `recurringEventId` was deleted; a changed instance re-sends its whole series, which
//! the engine skips by ETag. Writing follows EXP-20261123-bhfzfs's contract: what Google
//! cannot hold (floating time, `RANGE=THISANDFUTURE`, more than one event, a zone it does
//! not know) is refused before it is queued, never changed silently; updates and deletes
//! carry `If-Match`, so a change made at Google meanwhile is a conflict, not overwritten.
//!
//! The credential is `{"refresh_token", "calendar_id"}`, sealed under `tokens`; access
//! tokens live only in memory.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result, anyhow, bail};
use chrono::{DateTime, Duration, NaiveDate, NaiveDateTime, TimeZone, Utc};
use chrono_tz::Tz;
use nitya_ical::{Comp, Prop, escape, fold, param_value};
use reqwest::Url;
use serde_json::{Map, Value, json};

use crate::sync::{Change, Driver, DriverError, Page, PushError, Write};

pub const PROVIDER: &str = "google";
pub const SCOPE: &str = "https://www.googleapis.com/auth/calendar";
const API: &str = "https://www.googleapis.com/calendar/v3";
const PAGE: &str = "250";

/// The Desktop OAuth client from Google's console (ADR-0009), as its JSON download.
#[derive(Clone)]
pub struct OAuthClient {
    pub client_id: String,
    pub client_secret: String,
    pub auth_uri: String,
    pub token_uri: String,
}

impl OAuthClient {
    pub fn load(path: &std::path::Path) -> Result<OAuthClient> {
        let v: Value = serde_json::from_slice(&std::fs::read(path).with_context(|| format!("reading {}", path.display()))?)?;
        let c = v.get("installed").context("not a Desktop client's JSON (no \"installed\")")?;
        let s = |k: &str| c.get(k).and_then(Value::as_str).map(str::to_string).with_context(|| format!("the client JSON has no {k}"));
        Ok(OAuthClient { client_id: s("client_id")?, client_secret: s("client_secret")?, auth_uri: s("auth_uri")?, token_uri: s("token_uri")? })
    }
}

/// One HTTP request's answer: status and body.
pub type Answer = Result<(u16, String), String>;

/// HTTP as the driver needs it; a fake in the tests.
#[tonic::async_trait]
pub trait Http: Send + Sync {
    async fn send(&self, method: &str, url: &str, headers: Vec<(&'static str, String)>, body: Option<(&'static str, String)>) -> Answer;
}

pub struct Reqwest(pub reqwest::Client);

#[tonic::async_trait]
impl Http for Reqwest {
    async fn send(&self, method: &str, url: &str, headers: Vec<(&'static str, String)>, body: Option<(&'static str, String)>) -> Answer {
        let m = reqwest::Method::from_bytes(method.as_bytes()).map_err(|e| e.to_string())?;
        let mut r = self.0.request(m, url).timeout(std::time::Duration::from_secs(60));
        for (k, v) in headers {
            r = r.header(k, v);
        }
        if let Some((ctype, b)) = body {
            r = r.header("content-type", ctype).body(b);
        }
        let resp = r.send().await.map_err(|e| format!("{e}"))?;
        let status = resp.status().as_u16();
        Ok((status, resp.text().await.map_err(|e| e.to_string())?))
    }
}

/// A calendar of a signed-in account, to choose from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Calendar {
    pub id: String,
    pub summary: String,
    pub primary: bool,
    pub role: String,
}

pub struct Google {
    oauth: OAuthClient,
    http: Arc<dyn Http>,
    /// Access tokens by refresh token, with their expiry (Unix seconds).
    access: Mutex<HashMap<String, (String, i64)>>,
}

struct Cred {
    refresh_token: String,
    calendar_id: String,
}

fn cred(b: &[u8]) -> Result<Cred, String> {
    let v: Value = serde_json::from_slice(b).map_err(|e| format!("the credential is not JSON: {e}"))?;
    let s = |k: &str| v.get(k).and_then(Value::as_str).map(str::to_string).ok_or_else(|| format!("the credential has no {k}"));
    Ok(Cred { refresh_token: s("refresh_token")?, calendar_id: s("calendar_id")? })
}

/// The credential of one calendar's channel.
pub fn credential(refresh_token: &str, calendar_id: &str) -> Vec<u8> {
    json!({ "refresh_token": refresh_token, "calendar_id": calendar_id }).to_string().into_bytes()
}

fn form(pairs: &[(&str, &str)]) -> String {
    let mut u = Url::parse("http://x/").expect("a URL");
    u.query_pairs_mut().extend_pairs(pairs);
    u.query().unwrap_or_default().to_string()
}

fn url(path: &[&str], query: &[(&str, &str)]) -> String {
    let mut u = Url::parse(API).expect("the API's URL");
    u.path_segments_mut().expect("a base").extend(path);
    if !query.is_empty() {
        u.query_pairs_mut().extend_pairs(query);
    }
    u.to_string()
}

/// Google's error message in an error body, or the body.
fn message(body: &str) -> String {
    serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| v.pointer("/error/message").and_then(Value::as_str).map(str::to_string).or_else(|| v.get("error_description").and_then(Value::as_str).map(str::to_string)))
        .unwrap_or_else(|| body.chars().take(200).collect())
}

enum Fail {
    Auth(String),
    Status(u16, String),
    Net(String),
}

impl std::fmt::Display for Fail {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Fail::Auth(m) | Fail::Net(m) => f.write_str(m),
            Fail::Status(s, m) => write!(f, "Google answered {s}: {m}"),
        }
    }
}

impl From<Fail> for DriverError {
    fn from(f: Fail) -> DriverError {
        match f {
            Fail::Auth(m) => DriverError::Auth(m),
            Fail::Status(410, _) => DriverError::CursorExpired,
            Fail::Status(s, m) => DriverError::Transient(format!("Google answered {s}: {m}")),
            Fail::Net(m) => DriverError::Transient(m),
        }
    }
}

impl From<Fail> for PushError {
    fn from(f: Fail) -> PushError {
        match f {
            Fail::Auth(m) => PushError::Auth(m),
            Fail::Status(412, _) => PushError::Conflict("changed at Google since this device read it: sync, then edit again".into()),
            Fail::Status(s @ (400 | 403 | 404), m) => PushError::Refused(format!("Google refused it ({s}): {m}")),
            Fail::Status(s, m) => PushError::Transient(format!("Google answered {s}: {m}")),
            Fail::Net(m) => PushError::Transient(m),
        }
    }
}

impl Google {
    pub fn new(oauth: OAuthClient, http: Arc<dyn Http>) -> Google {
        Google { oauth, http, access: Mutex::new(HashMap::new()) }
    }

    /// The sign-in page's URL for a loopback redirect with PKCE (RFC 8252, RFC 7636).
    pub fn auth_url(&self, redirect_uri: &str, code_challenge: &str, state: &str) -> String {
        let mut u = match Url::parse(&self.oauth.auth_uri) {
            Ok(u) => u,
            Err(_) => return String::new(),
        };
        u.query_pairs_mut().extend_pairs([
            ("client_id", self.oauth.client_id.as_str()),
            ("redirect_uri", redirect_uri),
            ("response_type", "code"),
            ("scope", SCOPE),
            ("code_challenge", code_challenge),
            ("code_challenge_method", "S256"),
            ("state", state),
            ("access_type", "offline"),
            ("prompt", "consent"),
        ]);
        u.to_string()
    }

    /// Trades the sign-in's code for a refresh token.
    pub async fn exchange(&self, code: &str, verifier: &str, redirect_uri: &str) -> Result<String> {
        let body = form(&[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("code_verifier", verifier),
            ("redirect_uri", redirect_uri),
            ("client_id", &self.oauth.client_id),
            ("client_secret", &self.oauth.client_secret),
        ]);
        let (status, b) = self.http.send("POST", &self.oauth.token_uri, vec![], Some(("application/x-www-form-urlencoded", body))).await.map_err(|e| anyhow!(e))?;
        if status != 200 {
            bail!("Google refused the sign-in ({status}): {}", message(&b));
        }
        let v: Value = serde_json::from_str(&b)?;
        let refresh = v.get("refresh_token").and_then(Value::as_str).context("Google gave no refresh token")?;
        if let (Some(a), Some(e)) = (v.get("access_token").and_then(Value::as_str), v.get("expires_in").and_then(Value::as_i64)) {
            self.access.lock().expect("not poisoned").insert(refresh.to_string(), (a.to_string(), nitya_device::now() + e));
        }
        Ok(refresh.to_string())
    }

    /// The calendars the signed-in account can see.
    pub async fn calendars(&self, refresh_token: &str) -> Result<Vec<Calendar>> {
        let mut out = Vec::new();
        let mut page: Option<String> = None;
        loop {
            let mut q = vec![("maxResults", "250")];
            if let Some(p) = &page {
                q.push(("pageToken", p));
            }
            let v = self.call(refresh_token, "GET", &url(&["users", "me", "calendarList"], &q), None, None).await.map_err(|f| anyhow!(f.to_string()))?;
            for c in v.get("items").and_then(Value::as_array).into_iter().flatten() {
                out.push(Calendar {
                    id: str_of(c, "id"),
                    summary: c.get("summaryOverride").or(c.get("summary")).and_then(Value::as_str).unwrap_or_default().to_string(),
                    primary: c.get("primary").and_then(Value::as_bool).unwrap_or(false),
                    role: str_of(c, "accessRole"),
                });
            }
            page = v.get("nextPageToken").and_then(Value::as_str).map(str::to_string);
            if page.is_none() {
                return Ok(out);
            }
        }
    }

    async fn token(&self, refresh: &str) -> Result<String, Fail> {
        let now = nitya_device::now();
        if let Some((a, exp)) = self.access.lock().expect("not poisoned").get(refresh)
            && *exp > now + 60
        {
            return Ok(a.clone());
        }
        let body = form(&[("grant_type", "refresh_token"), ("refresh_token", refresh), ("client_id", &self.oauth.client_id), ("client_secret", &self.oauth.client_secret)]);
        let (status, b) = self.http.send("POST", &self.oauth.token_uri, vec![], Some(("application/x-www-form-urlencoded", body))).await.map_err(Fail::Net)?;
        if status == 400 || status == 401 {
            return Err(Fail::Auth(format!("Google refused the sign-in: {}; sign the account in again", message(&b))));
        }
        if status != 200 {
            return Err(Fail::Status(status, message(&b)));
        }
        let v: Value = serde_json::from_str(&b).map_err(|e| Fail::Net(e.to_string()))?;
        let a = v.get("access_token").and_then(Value::as_str).ok_or_else(|| Fail::Net("no access token".into()))?.to_string();
        let exp = now + v.get("expires_in").and_then(Value::as_i64).unwrap_or(3600);
        self.access.lock().expect("not poisoned").insert(refresh.to_string(), (a.clone(), exp));
        Ok(a)
    }

    /// One API call; 204 is `null`. A 401 forgets the access token (the next call gets a
    /// new one) and is transient.
    async fn call(&self, refresh: &str, method: &str, u: &str, if_match: Option<&str>, body: Option<&Value>) -> Result<Value, Fail> {
        let tok = self.token(refresh).await?;
        let mut h = vec![("authorization", format!("Bearer {tok}"))];
        if let Some(e) = if_match {
            h.push(("if-match", e.to_string()));
        }
        let (status, b) = self.http.send(method, u, h, body.map(|v| ("application/json", v.to_string()))).await.map_err(Fail::Net)?;
        if status == 401 {
            self.access.lock().expect("not poisoned").remove(refresh);
        }
        if !(200..300).contains(&status) {
            return Err(Fail::Status(status, message(&b)));
        }
        if b.trim().is_empty() {
            return Ok(Value::Null);
        }
        serde_json::from_str(&b).map_err(|e| Fail::Net(format!("Google's answer is not JSON: {e}")))
    }

    /// The instance of the series `uid` that starts at `rid`, for a new override.
    async fn instance(&self, c: &Cred, uid: &str, rid: &When) -> Result<Value, PushError> {
        let ev = url(&["calendars", &c.calendar_id, "events"], &[("iCalUID", uid), ("showDeleted", "true")]);
        let list = self.call(&c.refresh_token, "GET", &ev, None, None).await?;
        let master = list
            .get("items")
            .and_then(Value::as_array)
            .and_then(|a| a.iter().find(|e| e.get("recurringEventId").is_none() && str_of(e, "status") != "cancelled"))
            .ok_or_else(|| PushError::Refused(format!("no series with UID {uid} in this calendar to override")))?;
        let at = rid.instant().map_err(PushError::Refused)?;
        let (lo, hi) = ((at - Duration::days(1)).to_rfc3339(), (at + Duration::days(1)).to_rfc3339());
        let q = [("timeMin", lo.as_str()), ("timeMax", hi.as_str()), ("showDeleted", "true")];
        let inst = self.call(&c.refresh_token, "GET", &url(&["calendars", &c.calendar_id, "events", &str_of(master, "id"), "instances"], &q), None, None).await?;
        inst.get("items")
            .and_then(Value::as_array)
            .and_then(|a| a.iter().find(|i| i.get("originalStartTime").and_then(|t| g_when(t).ok()).and_then(|w| w.instant().ok()) == Some(at)))
            .cloned()
            .ok_or_else(|| PushError::Refused("the series has no occurrence at that RECURRENCE-ID".into()))
    }
}

fn str_of(v: &Value, k: &str) -> String {
    v.get(k).and_then(Value::as_str).unwrap_or_default().to_string()
}

#[tonic::async_trait]
impl Driver for Google {
    async fn fetch(&self, credential: &[u8], cursor: Option<&[u8]>) -> Result<Page, DriverError> {
        let c = cred(credential).map_err(DriverError::Auth)?;
        let cur: Value = cursor.and_then(|b| serde_json::from_slice(b).ok()).unwrap_or(Value::Null);
        let (sync, page) = (cur.get("sync").and_then(Value::as_str), cur.get("page").and_then(Value::as_str));
        let mut q = vec![("maxResults", PAGE), ("singleEvents", "false")];
        if let Some(s) = sync {
            q.push(("syncToken", s));
        }
        if let Some(p) = page {
            q.push(("pageToken", p));
        }
        let v = self.call(&c.refresh_token, "GET", &url(&["calendars", &c.calendar_id, "events"], &q), None, None).await?;
        let items: Vec<Value> = v.get("items").and_then(Value::as_array).cloned().unwrap_or_default();
        // A cancelled instance's stub names its series but not the series' UID: from the
        // master in the same page, else asked for once.
        let mut uids: HashMap<String, Option<String>> = HashMap::new();
        for e in &items {
            let uid = e.get("iCalUID").and_then(Value::as_str).map(str::to_string);
            match e.get("recurringEventId").and_then(Value::as_str) {
                // an exception carries its series' UID
                Some(m) if uid.is_some() => {
                    uids.entry(m.to_string()).or_insert(uid);
                }
                Some(_) => {}
                None => {
                    let live = str_of(e, "status") != "cancelled";
                    uids.insert(str_of(e, "id"), uid.filter(|_| live));
                }
            }
        }
        let mut changes = Vec::new();
        for e in &items {
            let id = str_of(e, "id");
            let cancelled = str_of(e, "status") == "cancelled";
            let Some(master) = e.get("recurringEventId").and_then(Value::as_str) else {
                if cancelled {
                    changes.push(Change::Delete { id });
                } else {
                    changes.push(Change::Upsert { id, etag: e.get("etag").and_then(Value::as_str).map(str::to_string), data: to_ical(e, None).map_err(DriverError::Transient)?.into_bytes() });
                }
                continue;
            };
            let uid = match e.get("iCalUID").and_then(Value::as_str) {
                Some(u) => Some(u.to_string()),
                None => match uids.get(master) {
                    Some(u) => u.clone(),
                    None => {
                        let m = self.call(&c.refresh_token, "GET", &url(&["calendars", &c.calendar_id, "events", master], &[]), None, None).await;
                        let u = match m {
                            Ok(m) if str_of(&m, "status") != "cancelled" => m.get("iCalUID").and_then(Value::as_str).map(str::to_string),
                            Ok(_) | Err(Fail::Status(404 | 410, _)) => None,
                            Err(f) => return Err(f.into()),
                        };
                        uids.insert(master.to_string(), u.clone());
                        u
                    }
                },
            };
            // An instance of a deleted series goes with it.
            match uid {
                None => changes.push(Change::Delete { id }),
                Some(u) => changes.push(Change::Upsert { id, etag: e.get("etag").and_then(Value::as_str).map(str::to_string), data: to_ical(e, Some(&u)).map_err(DriverError::Transient)?.into_bytes() }),
            }
        }
        let next_page = v.get("nextPageToken").and_then(Value::as_str);
        let cursor = match (next_page, v.get("nextSyncToken").and_then(Value::as_str)) {
            (Some(p), _) => json!({ "sync": sync, "page": p }),
            (None, Some(s)) => json!({ "sync": s }),
            (None, None) => return Err(DriverError::Transient("Google ended a listing without a sync token".into())),
        };
        Ok(Page { changes, cursor: Some(cursor.to_string().into_bytes()), more: next_page.is_some(), credential: None })
    }

    async fn push(&self, credential: &[u8], w: &Write) -> Result<Change, PushError> {
        let c = cred(credential).map_err(PushError::Auth)?;
        let ev = |id: &str| url(&["calendars", &c.calendar_id, "events", id], &[("sendUpdates", "none"), ("supportsAttachments", "true")]);
        let Some(data) = &w.data else {
            let id = w.provider_id.as_deref().ok_or_else(|| PushError::Refused("a delete names its item".into()))?;
            return match self.call(&c.refresh_token, "DELETE", &ev(id), w.base_etag.as_deref(), None).await {
                Ok(_) | Err(Fail::Status(404 | 410, _)) => Ok(Change::Delete { id: id.into() }),
                Err(f) => Err(f.into()),
            };
        };
        let text = std::str::from_utf8(data).map_err(|_| PushError::Refused("not UTF-8".into()))?;
        let m = from_ical(text).map_err(PushError::Refused)?;
        // An update names the event by its id; its UID is Google's already.
        let mut put = m.body.clone();
        if let Some(o) = put.as_object_mut() {
            o.remove("iCalUID");
        }
        let (got, uid) = match (&w.provider_id, &m.recurrence_id) {
            (Some(id), _) => (self.call(&c.refresh_token, "PUT", &ev(id), w.base_etag.as_deref(), Some(&put)).await?, m.uid.clone()),
            (None, Some(rid)) => {
                let uid = m.uid.clone().ok_or_else(|| PushError::Refused("an override needs its series' UID".into()))?;
                let inst = self.instance(&c, &uid, rid).await?;
                (self.call(&c.refresh_token, "PUT", &ev(&str_of(&inst, "id")), inst.get("etag").and_then(Value::as_str), Some(&put)).await?, Some(uid))
            }
            (None, None) => {
                let u = url(&["calendars", &c.calendar_id, "events"], &[("sendUpdates", "none"), ("supportsAttachments", "true")]);
                (self.call(&c.refresh_token, "POST", &u, None, Some(&m.body)).await?, m.uid.clone())
            }
        };
        let id = str_of(&got, "id");
        let data = to_ical(&got, uid.as_deref()).map_err(PushError::Transient)?.into_bytes();
        Ok(Change::Upsert { id, etag: got.get("etag").and_then(Value::as_str).map(str::to_string), data })
    }

    fn check(&self, data: &[u8]) -> Result<(), String> {
        from_ical(std::str::from_utf8(data).map_err(|_| "not UTF-8".to_string())?).map(|_| ())
    }
}

// ------------------------------------------------------------------ times

/// A DTSTART-like value.
#[derive(Debug, Clone, PartialEq, Eq)]
enum When {
    Date(NaiveDate),
    Utc(DateTime<Utc>),
    Zoned(NaiveDateTime, String),
}

impl When {
    fn of(p: &Prop) -> Result<When, String> {
        let v = p.value.trim();
        if p.param("VALUE") == Some("DATE") || v.len() == 8 {
            return NaiveDate::parse_from_str(v, "%Y%m%d").map(When::Date).map_err(|_| format!("{}: {v} is not a date", p.name));
        }
        let (local, utc) = match v.strip_suffix('Z') {
            Some(x) => (x, true),
            None => (v, false),
        };
        let t = NaiveDateTime::parse_from_str(local, "%Y%m%dT%H%M%S").map_err(|_| format!("{}: {v} is not a date-time", p.name))?;
        if utc {
            return Ok(When::Utc(Utc.from_utc_datetime(&t)));
        }
        match p.param("TZID") {
            Some(z) => {
                z.parse::<Tz>().map_err(|_| format!("{}: Google knows only IANA zones, not {z}", p.name))?;
                Ok(When::Zoned(t, z.to_string()))
            }
            None => Err(format!("{}: Google has no floating time; give it a TZID or a UTC time", p.name)),
        }
    }

    fn prop(&self, name: &str) -> String {
        match self {
            When::Date(d) => format!("{name};VALUE=DATE:{}", d.format("%Y%m%d")),
            When::Utc(t) => format!("{name}:{}", t.format("%Y%m%dT%H%M%SZ")),
            When::Zoned(t, z) => format!("{name};TZID={}:{}", param_value(z), t.format("%Y%m%dT%H%M%S")),
        }
    }

    fn google(&self) -> Value {
        match self {
            When::Date(d) => json!({ "date": d.format("%Y-%m-%d").to_string() }),
            When::Utc(t) => json!({ "dateTime": t.format("%Y-%m-%dT%H:%M:%SZ").to_string(), "timeZone": "UTC" }),
            When::Zoned(t, z) => json!({ "dateTime": t.format("%Y-%m-%dT%H:%M:%S").to_string(), "timeZone": z }),
        }
    }

    /// The instant it names; a date's midnight in UTC.
    fn instant(&self) -> Result<DateTime<Utc>, String> {
        match self {
            When::Date(d) => Ok(Utc.from_utc_datetime(&d.and_hms_opt(0, 0, 0).expect("midnight"))),
            When::Utc(t) => Ok(*t),
            When::Zoned(t, z) => {
                let tz: Tz = z.parse().map_err(|_| format!("unknown zone {z}"))?;
                tz.from_local_datetime(t).earliest().map(|x| x.with_timezone(&Utc)).ok_or_else(|| format!("{t} does not exist in {z}"))
            }
        }
    }
}

/// Google's `{date}` or `{dateTime, timeZone}`: in its zone when it names one, else UTC.
fn g_when(t: &Value) -> Result<When, String> {
    if let Some(d) = t.get("date").and_then(Value::as_str) {
        return NaiveDate::parse_from_str(d, "%Y-%m-%d").map(When::Date).map_err(|e| format!("{d}: {e}"));
    }
    let s = t.get("dateTime").and_then(Value::as_str).ok_or("a time with neither date nor dateTime")?;
    let at = DateTime::parse_from_rfc3339(s).map_err(|e| format!("{s}: {e}"))?;
    match t.get("timeZone").and_then(Value::as_str).filter(|z| *z != "UTC" && *z != "Etc/UTC") {
        Some(z) => {
            let tz: Tz = z.parse().map_err(|_| format!("Google named an unknown zone {z}"))?;
            Ok(When::Zoned(at.with_timezone(&tz).naive_local(), z.to_string()))
        }
        None => Ok(When::Utc(at.with_timezone(&Utc))),
    }
}

/// `-PT15M`, `-P1D`, `-PT1H30M`: minutes before the start.
fn trigger_minutes(t: &str) -> Result<i64, String> {
    let bad = || format!("alarm trigger {t}: Google holds only a time before the start");
    let rest = t.strip_prefix("-P").ok_or_else(bad)?;
    let (mut total, mut num, mut in_time) = (0i64, String::new(), false);
    for ch in rest.chars() {
        match ch {
            '0'..='9' => num.push(ch),
            'T' => in_time = true,
            u => {
                let n: i64 = num.parse().map_err(|_| bad())?;
                num.clear();
                total += match (u, in_time) {
                    ('W', false) => n * 10080,
                    ('D', false) => n * 1440,
                    ('H', true) => n * 60,
                    ('M', true) => n,
                    ('S', true) => n / 60,
                    _ => return Err(bad()),
                };
            }
        }
    }
    if !num.is_empty() {
        return Err(bad());
    }
    Ok(total)
}

fn minutes_trigger(m: i64) -> String {
    if m == 0 {
        "-PT0M".into()
    } else if m % 1440 == 0 {
        format!("-P{}D", m / 1440)
    } else if m % 60 == 0 {
        format!("-PT{}H", m / 60)
    } else {
        format!("-PT{m}M")
    }
}

const STATUS: [(&str, &str); 3] = [("CONFIRMED", "confirmed"), ("TENTATIVE", "tentative"), ("CANCELLED", "cancelled")];
const PARTSTAT: [(&str, &str); 4] = [("NEEDS-ACTION", "needsAction"), ("ACCEPTED", "accepted"), ("DECLINED", "declined"), ("TENTATIVE", "tentative")];

/// Properties Google has no field for, kept as private extended properties (a choice of
/// EXP-20261123-bhfzfs's driver): their names are the keys, their lines the values.
fn kept_as_extended(name: &str) -> bool {
    name.starts_with("X-") || name == "CATEGORIES" || name == "PRIORITY"
}

// ------------------------------------------------------------------ Google to iCalendar

/// A Google event as a calendar file with one VEVENT. `uid` stands in for a cancelled
/// instance's stub, which carries none.
pub fn to_ical(e: &Value, uid: Option<&str>) -> Result<String, String> {
    let uid = e.get("iCalUID").and_then(Value::as_str).or(uid).ok_or("an event without a UID")?;
    let mut l = vec!["BEGIN:VCALENDAR".to_string(), "VERSION:2.0".into(), "PRODID:-//nitya//google//EN".into(), "BEGIN:VEVENT".into(), format!("UID:{uid}")];
    let stamp = e.get("updated").and_then(Value::as_str).and_then(|u| DateTime::parse_from_rfc3339(u).ok()).map(|t| t.with_timezone(&Utc)).unwrap_or_default();
    l.push(format!("DTSTAMP:{}", stamp.format("%Y%m%dT%H%M%SZ")));
    let rid = e.get("originalStartTime").map(g_when).transpose()?;
    if let Some(r) = &rid {
        l.push(r.prop("RECURRENCE-ID"));
    }
    match (e.get("start"), e.get("end"), &rid) {
        (Some(s), Some(en), _) => {
            l.push(g_when(s)?.prop("DTSTART"));
            l.push(g_when(en)?.prop("DTEND"));
        }
        // A cancelled instance's stub: the occurrence it cancels.
        (_, _, Some(r)) => l.push(r.prop("DTSTART")),
        _ => return Err("an event without a start".into()),
    }
    if rid.is_none() {
        l.extend(e.get("recurrence").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).map(str::to_string));
    }
    for (k, name) in [("summary", "SUMMARY"), ("description", "DESCRIPTION"), ("location", "LOCATION")] {
        if let Some(v) = e.get(k).and_then(Value::as_str) {
            l.push(format!("{name}:{}", escape(v)));
        }
    }
    let status = str_of(e, "status");
    if let Some((s, _)) = STATUS.iter().find(|(_, g)| *g == status)
        && *s != "CONFIRMED"
    {
        l.push(format!("STATUS:{s}"));
    }
    if str_of(e, "transparency") == "transparent" {
        l.push("TRANSP:TRANSPARENT".into());
    }
    if let v @ ("private" | "confidential" | "public") = str_of(e, "visibility").as_str() {
        l.push(format!("CLASS:{}", v.to_uppercase()));
    }
    let attendees = e.get("attendees").and_then(Value::as_array).cloned().unwrap_or_default();
    // Google names an organizer for every event; iCalendar only for meetings.
    if let Some(o) = e.get("organizer").filter(|_| !attendees.is_empty()) {
        l.push(format!("ORGANIZER;CN={}:mailto:{}", param_value(&str_of(o, "displayName")), str_of(o, "email")));
    }
    for a in &attendees {
        let role = if a.get("optional").and_then(Value::as_bool) == Some(true) { "OPT-PARTICIPANT" } else { "REQ-PARTICIPANT" };
        let cut = if a.get("resource").and_then(Value::as_bool) == Some(true) { ";CUTYPE=ROOM" } else { "" };
        let rs = a.get("responseStatus").and_then(Value::as_str).unwrap_or("needsAction");
        let ps = PARTSTAT.iter().find(|(_, g)| *g == rs).map_or("NEEDS-ACTION", |(i, _)| i);
        l.push(format!("ATTENDEE;CN={}{cut};ROLE={role};PARTSTAT={ps}:mailto:{}", param_value(&str_of(a, "displayName")), str_of(a, "email")));
    }
    if let Some(u) = e.pointer("/source/url").and_then(Value::as_str) {
        l.push(format!("URL:{u}"));
    }
    for a in e.get("attachments").and_then(Value::as_array).into_iter().flatten() {
        l.push(format!("ATTACH;FMTTYPE={}:{}", param_value(&str_of(a, "mimeType")), str_of(a, "fileUrl")));
    }
    if let Some(p) = e.pointer("/extendedProperties/private").and_then(Value::as_object) {
        let mut keys: Vec<&String> = p.keys().filter(|k| kept_as_extended(k)).collect();
        keys.sort();
        for k in keys {
            if let Some(line) = p[k].as_str().filter(|v| v.to_ascii_uppercase().starts_with(k.as_str())) {
                l.push(line.to_string());
            }
        }
    }
    if let Some(s) = e.get("sequence").and_then(Value::as_i64).filter(|s| *s != 0) {
        l.push(format!("SEQUENCE:{s}"));
    }
    for r in e.pointer("/reminders/overrides").and_then(Value::as_array).into_iter().flatten() {
        let action = if str_of(r, "method") == "email" { "EMAIL" } else { "DISPLAY" };
        let m = r.get("minutes").and_then(Value::as_i64).unwrap_or(0);
        l.extend(["BEGIN:VALARM".into(), format!("ACTION:{action}"), "DESCRIPTION:Reminder".into(), format!("TRIGGER:{}", minutes_trigger(m)), "END:VALARM".into()]);
    }
    l.extend(["END:VEVENT".into(), "END:VCALENDAR".into()]);
    Ok(fold(&l))
}

// ------------------------------------------------------------------ iCalendar to Google

/// What one VEVENT becomes at Google.
struct Mapped {
    body: Value,
    uid: Option<String>,
    recurrence_id: Option<When>,
}

fn mailto(v: &str) -> String {
    let t = v.trim();
    if t.len() > 7 && t[..7].eq_ignore_ascii_case("mailto:") { t[7..].to_string() } else { t.to_string() }
}

/// The body Google takes for a calendar file with one VEVENT, or why it cannot hold it.
fn from_ical(text: &str) -> Result<Mapped, String> {
    let root = nitya_ical::parse(text);
    let events = root.find("VEVENT");
    let ev: &Comp = match events.as_slice() {
        [e] => e,
        [] => return Err("no VEVENT".into()),
        _ => return Err("one VEVENT per item: Google keeps a series' overrides as events of their own".into()),
    };
    let mut b = Map::new();
    let start = When::of(ev.get("DTSTART").ok_or("no DTSTART")?)?;
    if ev.get("DURATION").is_some() {
        return Err("give DTEND rather than DURATION".into());
    }
    let end = When::of(ev.get("DTEND").ok_or("no DTEND")?)?;
    b.insert("start".into(), start.google());
    b.insert("end".into(), end.google());
    let recurrence_id = match ev.get("RECURRENCE-ID") {
        Some(p) if p.param("RANGE").is_some() => return Err("RECURRENCE-ID;RANGE=THISANDFUTURE: Google has no range; split the series instead".into()),
        Some(p) => Some(When::of(p)?),
        None => None,
    };
    if let Some(r) = &recurrence_id {
        b.insert("originalStartTime".into(), r.google());
    } else {
        let rec: Vec<Value> = ev.props.iter().filter(|p| matches!(p.name.as_str(), "RRULE" | "RDATE" | "EXDATE")).map(|p| Value::from(p.line.clone())).collect();
        if !rec.is_empty() {
            b.insert("recurrence".into(), rec.into());
        }
    }
    let uid = ev.get("UID").map(|p| p.value.trim().to_string());
    if let Some(u) = &uid {
        b.insert("iCalUID".into(), u.clone().into());
    }
    for (name, k) in [("SUMMARY", "summary"), ("DESCRIPTION", "description"), ("LOCATION", "location")] {
        if let Some(v) = ev.text(name) {
            b.insert(k.into(), v.into());
        }
    }
    let status = ev.get("STATUS").map_or("CONFIRMED".to_string(), |p| p.value.trim().to_ascii_uppercase());
    let g = STATUS.iter().find(|(i, _)| *i == status).ok_or_else(|| format!("STATUS:{status} is not an event's status"))?.1;
    b.insert("status".into(), g.into());
    let transparent = ev.get("TRANSP").is_some_and(|p| p.value.trim().eq_ignore_ascii_case("TRANSPARENT"));
    b.insert("transparency".into(), (if transparent { "transparent" } else { "opaque" }).into());
    if let Some(c) = ev.get("CLASS") {
        let v = c.value.trim().to_ascii_lowercase();
        if !matches!(v.as_str(), "private" | "confidential" | "public") {
            return Err(format!("CLASS:{}: Google holds PUBLIC, PRIVATE or CONFIDENTIAL", c.value));
        }
        b.insert("visibility".into(), v.into());
    }
    let attendees: Vec<Value> = ev
        .all("ATTENDEE")
        .map(|a| {
            let ps = a.param("PARTSTAT").unwrap_or("NEEDS-ACTION");
            let rs = PARTSTAT.iter().find(|(i, _)| *i == ps).map_or("needsAction", |(_, g)| g);
            let mut o = json!({ "email": mailto(&a.value), "responseStatus": rs, "optional": a.param("ROLE") == Some("OPT-PARTICIPANT") });
            if let Some(cn) = a.param("CN").filter(|c| !c.is_empty()) {
                o["displayName"] = cn.into();
            }
            if a.param("CUTYPE") == Some("ROOM") {
                o["resource"] = true.into();
            }
            o
        })
        .collect();
    if !attendees.is_empty() {
        b.insert("attendees".into(), attendees.into());
    }
    let alarms = ev.children.iter().filter(|c| c.kind == "VALARM").map(|a| {
        let t = a.get("TRIGGER").ok_or("a VALARM without a TRIGGER")?;
        if t.param("RELATED") == Some("END") || t.param("VALUE") == Some("DATE-TIME") {
            return Err(format!("alarm trigger {}: Google holds only a time before the start", t.line));
        }
        let method = if a.text("ACTION").as_deref() == Some("EMAIL") { "email" } else { "popup" };
        Ok(json!({ "method": method, "minutes": trigger_minutes(t.value.trim())? }))
    });
    let overrides: Vec<Value> = alarms.collect::<Result<_, String>>()?;
    // No alarm is the calendar's default reminders.
    b.insert("reminders".into(), if overrides.is_empty() { json!({ "useDefault": true }) } else { json!({ "useDefault": false, "overrides": overrides }) });
    if let Some(u) = ev.get("URL") {
        b.insert("source".into(), json!({ "url": u.value.trim(), "title": "URL" }));
    }
    let attach: Vec<Value> = ev
        .all("ATTACH")
        .map(|a| json!({ "fileUrl": a.value.trim(), "mimeType": a.param("FMTTYPE").unwrap_or(""), "title": a.value.rsplit('/').next().unwrap_or("") }))
        .collect();
    if !attach.is_empty() {
        b.insert("attachments".into(), attach.into());
    }
    let mut extra = Map::new();
    for p in ev.props.iter().filter(|p| kept_as_extended(&p.name)) {
        if extra.insert(p.name.clone(), p.line.clone().into()).is_some() {
            return Err(format!("{} twice: Google keeps one value of a property it has no field for", p.name));
        }
    }
    if !extra.is_empty() {
        b.insert("extendedProperties".into(), json!({ "private": extra }));
    }
    if let Some(s) = ev.get("SEQUENCE") {
        b.insert("sequence".into(), s.value.trim().parse::<i64>().map_err(|_| "SEQUENCE is not a number")?.into());
    }
    Ok(Mapped { body: Value::Object(b), uid, recurrence_id })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    /// A request as the fake saw it: method, URL, If-Match, body.
    type Asked = (String, String, Option<String>, Option<String>);

    /// A scripted Google: answers in order, and keeps what it was asked.
    #[derive(Default)]
    struct Fake {
        answers: Mutex<VecDeque<(u16, Value)>>,
        asked: Mutex<Vec<Asked>>,
    }

    #[tonic::async_trait]
    impl Http for Fake {
        async fn send(&self, method: &str, url: &str, headers: Vec<(&'static str, String)>, body: Option<(&'static str, String)>) -> Answer {
            let if_match = headers.iter().find(|(k, _)| *k == "if-match").map(|(_, v)| v.clone());
            self.asked.lock().unwrap().push((method.into(), url.into(), if_match, body.map(|b| b.1)));
            let (s, v) = self.answers.lock().unwrap().pop_front().expect("an answer for every request");
            Ok((s, if v.is_null() { String::new() } else { v.to_string() }))
        }
    }

    fn google(answers: Vec<(u16, Value)>) -> (Arc<Fake>, Google) {
        let mut a: VecDeque<_> = answers.into();
        a.push_front((200, json!({ "access_token": "at", "expires_in": 3600 })));
        let fake = Arc::new(Fake { answers: Mutex::new(a), ..Default::default() });
        let oauth = OAuthClient { client_id: "cid".into(), client_secret: "cs".into(), auth_uri: "https://accounts.example/auth".into(), token_uri: "https://oauth.example/token".into() };
        (fake.clone(), Google::new(oauth, fake))
    }

    fn cred() -> Vec<u8> {
        credential("rt", "cal@example.org")
    }

    fn ev(id: &str) -> Value {
        json!({
            "id": id, "etag": format!("\"{id}-1\""), "status": "confirmed", "iCalUID": format!("{id}@google.com"),
            "updated": "2026-11-27T10:00:00.000Z", "summary": "Standup, daily",
            "start": { "dateTime": "2027-03-10T09:00:00Z", "timeZone": "Europe/London" },
            "end": { "dateTime": "2027-03-10T09:30:00Z", "timeZone": "Europe/London" },
        })
    }

    /// Unfolded, for asserting on whole lines.
    fn flat(ics: &str) -> String {
        nitya_ical::unfold(ics).into_iter().map(|l| l + "\r\n").collect()
    }

    fn text(c: &Change) -> String {
        match c {
            Change::Upsert { data, .. } => String::from_utf8(data.clone()).unwrap(),
            Change::Delete { id } => format!("delete {id}"),
        }
    }

    #[test]
    fn an_event_maps_to_icalendar_and_back_to_the_same_body() {
        let mut e = ev("a");
        e["recurrence"] = json!(["RRULE:FREQ=WEEKLY;COUNT=6", "EXDATE;TZID=Europe/London:20270317T090000"]);
        e["start"] = json!({ "dateTime": "2027-04-07T09:00:00+01:00", "timeZone": "Europe/London" });
        e["end"] = json!({ "dateTime": "2027-04-07T10:00:00+01:00", "timeZone": "Europe/London" });
        e["attendees"] = json!([{ "email": "a@example.org", "displayName": "Doe, A", "responseStatus": "accepted" }, { "email": "room@example.org", "resource": true, "optional": true }]);
        e["organizer"] = json!({ "email": "o@example.org", "displayName": "O" });
        e["reminders"] = json!({ "useDefault": false, "overrides": [{ "method": "popup", "minutes": 15 }, { "method": "email", "minutes": 1440 }] });
        e["extendedProperties"] = json!({ "private": { "X-FOO": "X-FOO;A=b:c", "CATEGORIES": "CATEGORIES:x,y", "other-app": "kept out" } });
        e["visibility"] = "private".into();
        e["transparency"] = "transparent".into();
        let ics = to_ical(&e, None).unwrap();
        let ics = flat(&ics);
        // Zoned times in their zone, the recurrence lines as Google holds them.
        assert!(ics.contains("DTSTART;TZID=Europe/London:20270407T090000\r\n"), "{ics}");
        assert!(ics.contains("EXDATE;TZID=Europe/London:20270317T090000\r\n"));
        assert!(ics.contains("ATTENDEE;CN=\"Doe, A\";ROLE=REQ-PARTICIPANT;PARTSTAT=ACCEPTED:mailto:a@example.org"));
        assert!(ics.contains("X-FOO;A=b:c\r\n") && !ics.contains("kept out"));
        assert!(ics.contains("TRIGGER:-P1D") && ics.contains("TRIGGER:-PT15M"));
        let m = from_ical(&ics).unwrap();
        assert_eq!(m.body["start"], json!({ "dateTime": "2027-04-07T09:00:00", "timeZone": "Europe/London" }));
        assert_eq!(m.body["recurrence"], e["recurrence"]);
        assert_eq!(m.body["iCalUID"], "a@google.com");
        assert_eq!(m.body["summary"], "Standup, daily");
        assert_eq!(m.body["attendees"][0], json!({ "email": "a@example.org", "displayName": "Doe, A", "responseStatus": "accepted", "optional": false }));
        assert_eq!(m.body["attendees"][1]["resource"], true);
        assert_eq!(m.body["reminders"], e["reminders"]);
        assert_eq!(m.body["extendedProperties"]["private"], json!({ "X-FOO": "X-FOO;A=b:c", "CATEGORIES": "CATEGORIES:x,y" }));
        assert_eq!((m.body["visibility"].as_str(), m.body["transparency"].as_str()), (Some("private"), Some("transparent")));
        assert!(m.body.get("organizer").is_none(), "Google sets the organizer itself");
    }

    #[test]
    fn all_day_utc_and_instances_map_and_a_stub_becomes_a_cancelled_occurrence() {
        let mut d = ev("d");
        d["start"] = json!({ "date": "2027-03-01" });
        d["end"] = json!({ "date": "2027-03-02" });
        let ics = to_ical(&d, None).unwrap();
        assert!(ics.contains("DTSTART;VALUE=DATE:20270301\r\nDTEND;VALUE=DATE:20270302\r\n"));
        assert_eq!(from_ical(&ics).unwrap().body["start"], json!({ "date": "2027-03-01" }));
        let mut u = ev("u");
        u["start"] = json!({ "dateTime": "2027-03-10T09:00:00Z" });
        u["end"] = json!({ "dateTime": "2027-03-10T10:00:00Z" });
        assert!(to_ical(&u, None).unwrap().contains("DTSTART:20270310T090000Z"));
        let stub = json!({ "id": "s_1", "status": "cancelled", "recurringEventId": "s", "originalStartTime": { "dateTime": "2027-03-31T09:00:00+01:00", "timeZone": "Europe/London" } });
        let ics = to_ical(&stub, Some("s@google.com")).unwrap();
        assert!(ics.contains("UID:s@google.com\r\n"));
        assert!(ics.contains("RECURRENCE-ID;TZID=Europe/London:20270331T090000\r\nDTSTART;TZID=Europe/London:20270331T090000\r\n"));
        assert!(ics.contains("STATUS:CANCELLED"));
    }

    #[test]
    fn what_google_cannot_hold_is_refused_before_writing() {
        let wrap = |body: &str| format!("BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:x\r\n{body}END:VEVENT\r\nEND:VCALENDAR\r\n");
        let ok = "DTSTART;TZID=Europe/London:20270301T090000\r\nDTEND;TZID=Europe/London:20270301T100000\r\n";
        assert!(from_ical(&wrap(ok)).is_ok());
        let refuse = |body: String, why: &str| {
            let e = from_ical(&body).err().unwrap_or_default();
            assert!(e.contains(why), "{e:?} should say {why}");
        };
        refuse(wrap("DTSTART:20270301T090000\r\nDTEND:20270301T100000\r\n"), "floating");
        refuse(wrap(&format!("{ok}RECURRENCE-ID;RANGE=THISANDFUTURE;TZID=Europe/London:20270301T090000\r\n")), "THISANDFUTURE");
        refuse(wrap("DTSTART;TZID=W. Europe Standard Time:20270301T090000\r\nDTEND;TZID=Europe/London:20270301T100000\r\n"), "IANA");
        refuse(wrap("DTSTART;TZID=Europe/London:20270301T090000\r\nDURATION:PT1H\r\n"), "DURATION");
        refuse(format!("{}{}", wrap(ok), wrap(ok)), "one VEVENT");
        refuse(wrap(&format!("{ok}X-A:1\r\nX-A:2\r\n")), "twice");
        refuse(wrap(&format!("{ok}BEGIN:VALARM\r\nTRIGGER;RELATED=END:-PT5M\r\nEND:VALARM\r\n")), "before the start");
    }

    #[tokio::test]
    async fn a_full_listing_pages_to_a_sync_token_and_maps_deletes_and_stubs() {
        let mut moved = ev("s_2");
        moved["recurringEventId"] = "s".into();
        moved["iCalUID"] = "s@google.com".into();
        moved["originalStartTime"] = json!({ "dateTime": "2027-03-10T09:00:00Z", "timeZone": "Europe/London" });
        let stub_here = json!({ "id": "s_3", "status": "cancelled", "recurringEventId": "s", "originalStartTime": { "dateTime": "2027-03-17T09:00:00Z" } });
        let stub_away = json!({ "id": "t_3", "status": "cancelled", "recurringEventId": "t", "originalStartTime": { "dateTime": "2027-03-17T09:00:00Z" } });
        let stub_gone = json!({ "id": "g_3", "status": "cancelled", "recurringEventId": "g", "originalStartTime": { "dateTime": "2027-03-17T09:00:00Z" } });
        let deleted = json!({ "id": "b", "status": "cancelled" });
        let (fake, g) = google(vec![
            (200, json!({ "items": [ev("a"), ev("s")], "nextPageToken": "p2" })),
            (200, json!({ "items": [moved, stub_here, stub_away, stub_gone, deleted], "nextSyncToken": "s1" })),
            // the stubs' series not in the page: asked for once each
            (200, json!({ "id": "t", "status": "confirmed", "iCalUID": "t@google.com" })),
            (404, json!({ "error": { "message": "Not Found" } })),
        ]);
        let p1 = g.fetch(&cred(), None).await.unwrap();
        assert!(p1.more);
        assert_eq!(p1.changes.len(), 2);
        let p2 = g.fetch(&cred(), p1.cursor.as_deref()).await.unwrap();
        assert!(!p2.more);
        assert_eq!(serde_json::from_slice::<Value>(p2.cursor.as_deref().unwrap()).unwrap(), json!({ "sync": "s1" }));
        let t: Vec<String> = p2.changes.iter().map(text).collect();
        assert!(t[0].contains("RECURRENCE-ID") && t[0].contains("UID:s@google.com"));
        // s's UID from its moved instance in the same page; t's asked for; g is gone
        assert!(t[1].contains("UID:s@google.com") && t[1].contains("STATUS:CANCELLED"), "{t:?}");
        assert!(t[2].contains("UID:t@google.com"));
        assert_eq!(t[3], "delete g_3");
        assert_eq!(t[4], "delete b");
        let asked = fake.asked.lock().unwrap();
        // the access token once; the second page carries the first's page token
        assert_eq!(asked.len(), 5);
        assert!(asked[2].1.contains("pageToken=p2") && !asked[2].1.contains("syncToken"));
        assert!(asked[2].1.contains("singleEvents=false"));
    }

    #[tokio::test]
    async fn an_expired_token_asks_for_a_full_sync_and_a_refused_sign_in_needs_one() {
        let (_, g) = google(vec![(410, json!({ "error": { "code": 410, "message": "Sync token is no longer valid" } }))]);
        let cur = json!({ "sync": "old" }).to_string().into_bytes();
        assert_eq!(g.fetch(&cred(), Some(&cur)).await, Err(DriverError::CursorExpired));
        let fake = Arc::new(Fake { answers: Mutex::new(vec![(400, json!({ "error": "invalid_grant", "error_description": "Token has been expired or revoked." }))].into()), ..Default::default() });
        let g = Google::new(OAuthClient { client_id: "c".into(), client_secret: "s".into(), auth_uri: "a".into(), token_uri: "https://t".into() }, fake);
        match g.fetch(&cred(), None).await {
            Err(DriverError::Auth(m)) => assert!(m.contains("expired or revoked"), "{m}"),
            other => panic!("{other:?}"),
        }
    }

    #[tokio::test]
    async fn writes_carry_if_match_and_a_conflict_is_not_overwritten() {
        let ics = to_ical(&ev("a"), None).unwrap().into_bytes();
        let mut v2 = ev("a");
        v2["etag"] = "\"a-2\"".into();
        let (fake, g) = google(vec![(200, v2), (412, json!({ "error": { "message": "Precondition Failed" } })), (204, Value::Null), (200, ev("n"))]);
        let w = |id: Option<&str>, data: Option<Vec<u8>>| Write { provider_id: id.map(str::to_string), base_etag: Some("\"a-1\"".into()), data };
        match g.push(&cred(), &w(Some("a"), Some(ics.clone()))).await.unwrap() {
            Change::Upsert { id, etag, .. } => assert_eq!((id.as_str(), etag.as_deref()), ("a", Some("\"a-2\""))),
            c => panic!("{c:?}"),
        }
        assert!(matches!(g.push(&cred(), &w(Some("a"), Some(ics.clone()))).await, Err(PushError::Conflict(_))));
        assert_eq!(g.push(&cred(), &w(Some("a"), None)).await.unwrap(), Change::Delete { id: "a".into() });
        match g.push(&cred(), &w(None, Some(ics))).await.unwrap() {
            Change::Upsert { id, .. } => assert_eq!(id, "n"),
            c => panic!("{c:?}"),
        }
        let asked = fake.asked.lock().unwrap();
        assert_eq!((asked[1].0.as_str(), asked[1].2.as_deref()), ("PUT", Some("\"a-1\"")));
        assert!(asked[1].1.contains("sendUpdates=none"));
        assert_eq!((asked[3].0.as_str(), asked[3].2.as_deref()), ("DELETE", Some("\"a-1\"")));
        assert_eq!(asked[4].0, "POST");
        assert!(asked[4].3.as_deref().unwrap().contains("\"iCalUID\":\"a@google.com\""));
    }

    #[tokio::test]
    async fn a_new_override_finds_its_occurrence_in_the_series() {
        let body = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:s@google.com\r\nRECURRENCE-ID;TZID=Europe/London:20270331T090000\r\nDTSTART;TZID=Europe/London:20270401T090000\r\nDTEND;TZID=Europe/London:20270401T100000\r\nSUMMARY:moved\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let other = json!({ "id": "s_0330", "etag": "e0", "originalStartTime": { "dateTime": "2027-03-30T09:00:00+01:00" } });
        let this = json!({ "id": "s_0331", "etag": "e1", "originalStartTime": { "dateTime": "2027-03-31T08:00:00Z", "timeZone": "UTC" } });
        let mut done = ev("s_0331");
        done["iCalUID"] = "s@google.com".into();
        done["recurringEventId"] = "s".into();
        done["originalStartTime"] = json!({ "dateTime": "2027-03-31T09:00:00+01:00", "timeZone": "Europe/London" });
        let (fake, g) = google(vec![(200, json!({ "items": [ev("s")] })), (200, json!({ "items": [other, this] })), (200, done)]);
        let w = Write { provider_id: None, base_etag: None, data: Some(body.as_bytes().to_vec()) };
        match g.push(&cred(), &w).await.unwrap() {
            Change::Upsert { id, .. } => assert_eq!(id, "s_0331"),
            c => panic!("{c:?}"),
        }
        let asked = fake.asked.lock().unwrap();
        assert!(asked[1].1.contains("iCalUID=s%40google.com"));
        assert!(asked[2].1.contains("/events/s/instances"));
        assert_eq!((asked[3].0.as_str(), asked[3].2.as_deref()), ("PUT", Some("e1")));
        assert!(asked[3].1.contains("/events/s_0331"));
    }

    /// The driver against Google itself, on a throwaway calendar of the account whose
    /// tokens `NITYA_GOOGLE_CLIENT` (the Desktop client JSON) and `NITYA_GOOGLE_TOKEN`
    /// (nitya-consent's token JSON) name; prints nothing secret. Run by hand:
    ///   cargo test -p nitya-server against_google -- --ignored --nocapture
    #[tokio::test]
    #[ignore]
    async fn against_google() {
        let (Ok(client), Ok(token)) = (std::env::var("NITYA_GOOGLE_CLIENT"), std::env::var("NITYA_GOOGLE_TOKEN")) else {
            panic!("set NITYA_GOOGLE_CLIENT and NITYA_GOOGLE_TOKEN");
        };
        let t: Value = serde_json::from_slice(&std::fs::read(token).unwrap()).unwrap();
        let rt = t["refresh_token"].as_str().unwrap().to_string();
        let g = Arc::new(Google::new(OAuthClient::load(std::path::Path::new(&client)).unwrap(), Arc::new(Reqwest(reqwest::Client::new()))));
        let cal = g.call(&rt, "POST", &url(&["calendars"], &[]), None, Some(&json!({ "summary": "nitya-driver-test", "timeZone": "Europe/London" }))).await.ok().unwrap();
        let cal = str_of(&cal, "id");
        let (g2, rt2, cal2) = (g.clone(), rt.clone(), cal.clone());
        let run = async move {
            let (g, rt, cal) = (g2, rt2, cal2);
            let cr = credential(&rt, &cal);
            let ics = |extra: &str| format!("BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\n{extra}END:VEVENT\r\nEND:VCALENDAR\r\n");
            let full = g.fetch(&cr, None).await.unwrap();
            assert!(full.changes.is_empty() && !full.more);
            let mut cursor = full.cursor.unwrap();
            let single = ics("UID:nitya-test-1@nitya.invalid\r\nDTSTART;TZID=America/New_York:20270310T090000\r\nDTEND;TZID=America/New_York:20270310T100000\r\nSUMMARY:one, with a comma\r\nBEGIN:VALARM\r\nACTION:DISPLAY\r\nTRIGGER:-PT10M\r\nEND:VALARM\r\n");
            let w = |id: Option<&str>, etag: Option<&str>, data: Option<String>| Write { provider_id: id.map(str::to_string), base_etag: etag.map(str::to_string), data: data.map(String::into_bytes) };
            let Change::Upsert { id: id1, etag: e1, data } = g.push(&cr, &w(None, None, Some(single.clone()))).await.unwrap() else { panic!() };
            let back = flat(&String::from_utf8(data).unwrap());
            assert!(back.contains("UID:nitya-test-1@nitya.invalid\r\n") && back.contains("DTSTART;TZID=America/New_York:20270310T090000\r\n") && back.contains("TRIGGER:-PT10M"), "{back}");
            let edited = single.replace("one, with a comma", "one, edited");
            let Change::Upsert { etag: e2, .. } = g.push(&cr, &w(Some(&id1), e1.as_deref(), Some(edited.clone()))).await.unwrap() else { panic!() };
            assert_ne!(e1, e2);
            let stale = g.push(&cr, &w(Some(&id1), e1.as_deref(), Some(edited))).await;
            assert!(matches!(stale, Err(PushError::Conflict(_))), "{stale:?}");
            let series = ics("UID:nitya-test-2@nitya.invalid\r\nDTSTART;TZID=Europe/London:20270322T090000\r\nDTEND;TZID=Europe/London:20270322T093000\r\nRRULE:FREQ=WEEKLY;COUNT=4\r\nSUMMARY:weekly\r\n");
            let Change::Upsert { id: id2, .. } = g.push(&cr, &w(None, None, Some(series))).await.unwrap() else { panic!() };
            // the occurrence after the clocks change (BST from 28 March), moved a day
            let moved = ics("UID:nitya-test-2@nitya.invalid\r\nRECURRENCE-ID;TZID=Europe/London:20270329T090000\r\nDTSTART;TZID=Europe/London:20270330T090000\r\nDTEND;TZID=Europe/London:20270330T093000\r\nSUMMARY:weekly, moved\r\n");
            let Change::Upsert { id: inst, data, .. } = g.push(&cr, &w(None, None, Some(moved))).await.unwrap() else { panic!() };
            assert!(flat(&String::from_utf8(data).unwrap()).contains("RECURRENCE-ID;TZID=Europe/London:20270329T090000\r\n"));
            let floating = ics("DTSTART:20270310T090000\r\nDTEND:20270310T100000\r\n");
            assert!(g.check(floating.as_bytes()).unwrap_err().contains("floating"));
            let inc = g.fetch(&cr, Some(&cursor)).await.unwrap();
            let ids: Vec<String> = inc.changes.iter().map(|c| match c { Change::Upsert { id, .. } | Change::Delete { id } => id.clone() }).collect();
            for want in [&id1, &id2, &inst] {
                assert!(ids.contains(want), "{want} not in {ids:?}");
            }
            cursor = inc.cursor.unwrap();
            assert_eq!(g.push(&cr, &w(Some(&id1), e2.as_deref(), None)).await.unwrap(), Change::Delete { id: id1.clone() });
            // Google now and then answers a fresh token with 410 (once in six runs on
            // 2026-11-27); the engine then lists in full, where the event is simply gone.
            match g.fetch(&cr, Some(&cursor)).await {
                Ok(inc) => assert!(inc.changes.contains(&Change::Delete { id: id1.clone() }), "{:?}", inc.changes),
                Err(DriverError::CursorExpired) => {
                    let all = g.fetch(&cr, None).await.unwrap();
                    assert!(!all.changes.iter().any(|c| matches!(c, Change::Upsert { id, .. } if *id == id1)));
                    println!("Google asked for a full sync after the delete; the full listing has no {id1}");
                }
                Err(e) => panic!("{e:?}"),
            }
            let bad = json!({ "sync": "garbled" }).to_string().into_bytes();
            assert_eq!(g.fetch(&cr, Some(&bad)).await, Err(DriverError::CursorExpired));
            println!("against Google: insert, edit, stale edit refused, series, moved occurrence, incremental, delete, expired cursor: all as expected");
        };
        let out = tokio::spawn(run).await;
        let _ = g.call(&rt, "DELETE", &url(&["calendars", &cal], &[]), None, None).await;
        out.unwrap();
    }

    #[test]
    fn the_sign_in_url_asks_for_a_refresh_token_with_pkce() {
        let (_, g) = google(vec![]);
        let u = g.auth_url("http://127.0.0.1:5000/", "chal", "st");
        for part in ["client_id=cid", "redirect_uri=http%3A%2F%2F127.0.0.1%3A5000%2F", "code_challenge=chal", "code_challenge_method=S256", "state=st", "access_type=offline", "prompt=consent", "scope=https%3A%2F%2Fwww.googleapis.com%2Fauth%2Fcalendar"] {
            assert!(u.contains(part), "{u} lacks {part}");
        }
    }
}
