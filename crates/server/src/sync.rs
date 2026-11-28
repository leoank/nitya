//! The channel engine (ADR-0003, ADR-0017): each channel is a provider account and data
//! class, synced by its driver from an opaque cursor the driver owns. A channel runs only
//! while its owner has consented to the server using `tokens` and the channel's class
//! (ADR-0005); every run that changes something or fails is in the audit log.
//!
//! Stages: `new`, `full_sync` and `incremental` while things go well; `waiting_for_consent`,
//! `needs_auth` (the credential was refused or is missing), `failed` (retried with backoff),
//! `no_driver`. A run holds the channel's lease in the store, so the poller and a sync-now
//! never run one channel twice at once, and a run that died is taken over after
//! `STALE_SECS`.
//!
//! A run first writes the changes devices queued (ADR-0018), oldest first, then fetches.
//! A change the provider refuses (a conflict, something it cannot hold) stays in the
//! outbox with the reason, and is not retried; one that fails for a passing reason stops
//! the run and is retried with it.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use nitya_device as dev;
use nitya_keys::{Delegate, KeySet};
use nitya_proto::v1::SealedItem;
use nitya_store::{Channel, Item, Outcome, Pending, Run};
use prost::Message;

use crate::ca::{Ca, Record};

pub const NEW: &str = "new";
pub const FULL: &str = "full_sync";
pub const INCREMENTAL: &str = "incremental";
pub const CONSENT: &str = "waiting_for_consent";
pub const AUTH: &str = "needs_auth";
pub const FAILED: &str = "failed";
pub const NO_DRIVER: &str = "no_driver";

/// A run holding a lease longer than this died with the process.
pub const STALE_SECS: i64 = 600;
/// The longest wait between retries of a failing channel.
const MAX_BACKOFF_SECS: i64 = 3600;
/// How many pages one run fetches at most; the rest waits for the next run.
const MAX_PAGES: usize = 1000;

/// One change at the provider.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    /// The item as the provider sent it (raw iCalendar, raw MIME), with its etag.
    Upsert { id: String, etag: Option<String>, data: Vec<u8> },
    Delete { id: String },
}

/// What a driver returns for one request.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Page {
    pub changes: Vec<Change>,
    /// Where the next request starts; `None` keeps the current one.
    pub cursor: Option<Vec<u8>>,
    /// More pages follow at once.
    pub more: bool,
    /// The credential, when the provider rotated it (Microsoft's refresh tokens).
    pub credential: Option<Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DriverError {
    /// The provider no longer knows the cursor (410, `syncStateNotFound`): start over.
    CursorExpired,
    /// The provider refused the credential: a person has to sign in again.
    Auth(String),
    /// Anything else: retried later.
    Transient(String),
}

/// A device's change to one item, for the provider: `data` is the item as it should be
/// (`None`: delete it); `provider_id` is `None` for a new item; `base_etag` is the version
/// the device changed, so the provider refuses the write if it changed meanwhile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Write {
    pub provider_id: Option<String>,
    pub base_etag: Option<String>,
    pub data: Option<Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PushError {
    /// The item changed at the provider since the device read it.
    Conflict(String),
    /// The provider cannot hold it, or will not take it.
    Refused(String),
    Auth(String),
    Transient(String),
}

/// A provider's side of a channel.
#[tonic::async_trait]
pub trait Driver: Send + Sync {
    /// Changes since `cursor` (from the start when `None`).
    async fn fetch(&self, credential: &[u8], cursor: Option<&[u8]>) -> Result<Page, DriverError>;
    /// Writes one change; the item as the provider now holds it (an upsert with its id and
    /// etag), or the delete.
    async fn push(&self, credential: &[u8], w: &Write) -> Result<Change, PushError>;
    /// Why the provider could not hold `data`, before it is queued.
    fn check(&self, data: &[u8]) -> Result<(), String>;
}

/// What one run did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    pub channel: i64,
    pub stage: String,
    pub full: bool,
    pub upserts: u32,
    pub deletes: u32,
    /// Queued changes the provider took (the outbox id and the item's provider id), and
    /// how many it refused.
    pub written: Vec<(i64, String)>,
    pub refused: u32,
    pub error: Option<String>,
}

pub struct Engine {
    ca: Arc<Ca>,
    delegate: Arc<Delegate>,
    drivers: HashMap<String, Arc<dyn Driver>>,
    poll_secs: i64,
}

fn item_ref(channel: i64, provider_id: &str) -> String {
    format!("channel:{channel}:{provider_id}")
}

fn credential_ref(channel: i64) -> String {
    format!("channel:{channel}:credential")
}

/// What a device seals a queued change under.
pub fn change_ref(channel: i64) -> String {
    format!("channel:{channel}:change")
}

impl Engine {
    pub fn new(ca: Arc<Ca>, delegate: Arc<Delegate>, drivers: HashMap<String, Arc<dyn Driver>>, poll_secs: i64) -> Engine {
        Engine { ca, delegate, drivers, poll_secs }
    }

    /// Polls in the background: every few seconds it runs the channels whose time has
    /// come (each channel's next attempt is `poll_secs` after its last run).
    pub fn spawn(self: Arc<Self>) {
        tokio::spawn(async move {
            loop {
                if let Err(e) = self.run_due(dev::now()).await {
                    eprintln!("nitya-server: sync: {e:#}");
                }
                tokio::time::sleep(Duration::from_secs(self.poll_secs.clamp(1, 15) as u64)).await;
            }
        });
    }

    /// Runs every channel that is due, one after the other.
    pub async fn run_due(&self, now: i64) -> Result<Vec<Report>> {
        let channels = self.ca.store().all_channels()?;
        let mut out = Vec::new();
        for c in channels.iter().filter(|c| c.next_attempt <= now && c.stage != AUTH) {
            out.push(self.sync_channel(c.id, now).await?);
        }
        Ok(out)
    }

    /// Syncs one channel now, whatever its schedule.
    pub async fn sync_channel(&self, id: i64, now: i64) -> Result<Report> {
        let c = self.ca.store().channel_by_id(id)?.with_context(|| format!("no channel {id}"))?;
        if !self.ca.store().claim_channel(id, now, STALE_SECS)? {
            return Ok(Report { channel: id, stage: c.stage, error: Some("a run of this channel is in progress".into()), ..Default::default() });
        }
        let (report, outcome) = self.run(&c, now).await;
        self.ca.store().release_channel(id, &outcome, dev::now())?;
        let changed = report.upserts + report.deletes + report.written.len() as u32 + report.refused > 0;
        if changed || report.error.is_some() {
            self.ca.store().record_run(&Run {
                channel_id: id,
                started: now,
                finished: dev::now(),
                full: report.full,
                upserts: report.upserts.into(),
                deletes: report.deletes.into(),
                error: report.error.clone(),
            })?;
        }
        // The delegate's use is logged when it did something or could not; a quiet poll
        // every minute is not (ADR-0017). Entering `waiting_for_consent` is logged once.
        let entered_wait = report.stage == CONSENT && c.stage != CONSENT;
        if changed || (report.error.is_some() && report.stage != CONSENT) || entered_wait {
            let detail = match &report.error {
                Some(e) => format!("{}: {e}", report.stage),
                None => {
                    let mut d = format!("{} sync, {} upserted, {} deleted", if report.full { "full" } else { "incremental" }, report.upserts, report.deletes);
                    if !report.written.is_empty() || report.refused > 0 {
                        d += &format!("; {} written, {} refused", report.written.len(), report.refused);
                    }
                    d
                }
            };
            self.ca.record(
                Record {
                    chain: c.account.clone(),
                    actor: "system:sync-delegate".into(),
                    action: "sync.run".into(),
                    resource: format!("channel:{id}"),
                    allowed: report.stage != CONSENT,
                    policies: vec![],
                    detail,
                },
                dev::now(),
            )?;
        }
        Ok(report)
    }

    fn backoff(&self, failures: i64) -> i64 {
        (self.poll_secs.max(1) << failures.min(12)).min(MAX_BACKOFF_SECS)
    }

    async fn run(&self, c: &Channel, now: i64) -> (Report, Outcome) {
        let mut r = Report { channel: c.id, stage: c.stage.clone(), ..Default::default() };
        let stop = |r: &mut Report, stage: &str, error: String, next: i64, failures: i64| {
            r.stage = stage.into();
            r.error = Some(error.clone());
            Outcome { stage: stage.into(), failures, next_attempt: next, last_sync: None, last_error: Some(error) }
        };
        let keys = match self.ca.store().key_set(&c.account).ok().flatten().and_then(|k| KeySet::decode(k.keys.as_slice()).ok()) {
            Some(k) if self.delegate.consented(&k, "tokens") && self.delegate.consented(&k, &c.class) => k,
            _ => {
                let o = stop(&mut r, CONSENT, format!("waiting for consent to tokens and {}", c.class), now + self.poll_secs, 0);
                return (r, o);
            }
        };
        let Some(driver) = self.drivers.get(&c.provider) else {
            let o = stop(&mut r, NO_DRIVER, format!("no driver for {}", c.provider), now + MAX_BACKOFF_SECS, 0);
            return (r, o);
        };
        let credential = match c.credential.as_deref().map(SealedItem::decode) {
            Some(Ok(s)) => match self.delegate.open_item(&c.account, &keys, "tokens", &credential_ref(c.id), &s) {
                Ok(p) => p,
                Err(e) => {
                    let o = stop(&mut r, FAILED, format!("opening the credential: {e:#}"), now + self.backoff(c.failures), c.failures + 1);
                    return (r, o);
                }
            },
            _ => {
                let o = stop(&mut r, AUTH, "no credential: sign the account in".into(), now + MAX_BACKOFF_SECS, 0);
                return (r, o);
            }
        };
        let mut credential = credential.to_vec();
        // Devices' changes first, so the fetch brings back what the provider made of them.
        let queue = match self.ca.store().pending(c.id) {
            Ok(q) => q,
            Err(e) => {
                let o = stop(&mut r, FAILED, format!("reading the outbox: {e:#}"), now + self.backoff(c.failures), c.failures + 1);
                return (r, o);
            }
        };
        for p in queue.iter().filter(|p| p.error.is_none()) {
            match self.push_one(c, &keys, driver.as_ref(), &credential, p, now).await {
                Ok(Some(id)) => r.written.push((p.id, id)),
                Ok(None) => r.refused += 1,
                Err(PushError::Auth(m)) => {
                    let o = stop(&mut r, AUTH, m, now + MAX_BACKOFF_SECS, 0);
                    return (r, o);
                }
                Err(e) => {
                    let m = match e {
                        PushError::Transient(m) | PushError::Conflict(m) | PushError::Refused(m) | PushError::Auth(m) => m,
                    };
                    let o = stop(&mut r, FAILED, format!("writing a change: {m}"), now + self.backoff(c.failures), c.failures + 1);
                    return (r, o);
                }
            }
        }
        // What the provider sends again unchanged (a series re-sent for one changed
        // instance, EXP-20261127-mqzh92) is not sealed and stored again.
        let known: HashMap<String, Option<String>> = match self.ca.store().items(c.id) {
            Ok(all) => all.into_iter().filter(|i| !i.deleted).map(|i| (i.provider_id, i.etag)).collect(),
            Err(e) => {
                let o = stop(&mut r, FAILED, format!("reading items: {e:#}"), now + self.backoff(c.failures), c.failures + 1);
                return (r, o);
            }
        };
        r.full = c.cursor.is_none() || c.stage == FULL || c.stage == NEW;
        let mut cursor = if r.full { None } else { c.cursor.clone() };
        let mut restarted = false;
        let mut seen: HashSet<String> = HashSet::new();
        for _ in 0..MAX_PAGES {
            let page = match driver.fetch(&credential, cursor.as_deref()).await {
                Ok(p) => p,
                Err(DriverError::CursorExpired) if !restarted => {
                    restarted = true;
                    r.full = true;
                    cursor = None;
                    seen.clear();
                    continue;
                }
                Err(DriverError::Auth(m)) => {
                    let o = stop(&mut r, AUTH, m, now + MAX_BACKOFF_SECS, 0);
                    return (r, o);
                }
                Err(e) => {
                    let m = match e {
                        DriverError::Transient(m) => m,
                        _ => "the provider forgot the cursor twice".into(),
                    };
                    let o = stop(&mut r, FAILED, m, now + self.backoff(c.failures), c.failures + 1);
                    return (r, o);
                }
            };
            if let Some(new) = &page.credential {
                // Stored before anything else: a rotated token that is lost locks the
                // channel out.
                let sealed = self.delegate.seal_item(&c.account, &keys, "tokens", &credential_ref(c.id), new);
                if let Err(e) = sealed.and_then(|s| self.ca.store().set_credential(c.id, &s.encode_to_vec(), now)) {
                    let o = stop(&mut r, FAILED, format!("keeping the rotated credential: {e:#}"), now + self.backoff(c.failures), c.failures + 1);
                    return (r, o);
                }
                credential = new.clone();
            }
            let mut items = Vec::new();
            for ch in &page.changes {
                match ch {
                    Change::Upsert { id, etag, data } => {
                        if etag.is_some() && known.get(id) == Some(etag) {
                            seen.insert(id.clone());
                            continue;
                        }
                        let s = match self.delegate.seal_item(&c.account, &keys, &c.class, &item_ref(c.id, id), data) {
                            Ok(s) => s,
                            Err(e) => {
                                let o = stop(&mut r, FAILED, format!("sealing: {e:#}"), now + self.backoff(c.failures), c.failures + 1);
                                return (r, o);
                            }
                        };
                        seen.insert(id.clone());
                        r.upserts += 1;
                        items.push(Item { id: 0, channel_id: c.id, provider_id: id.clone(), etag: etag.clone(), key_version: s.version, sealed: s.encode_to_vec(), updated: now, deleted: false });
                    }
                    Change::Delete { id } => {
                        seen.remove(id);
                        r.deletes += 1;
                        items.push(Item { id: 0, channel_id: c.id, provider_id: id.clone(), etag: None, key_version: 0, sealed: vec![], updated: now, deleted: true });
                    }
                }
            }
            if page.cursor.is_some() {
                cursor = page.cursor.clone();
            }
            let stage = if r.full { FULL } else { INCREMENTAL };
            if let Err(e) = self.ca.store().sync_batch(c.id, &items, cursor.as_deref(), stage, now) {
                let o = stop(&mut r, FAILED, format!("storing: {e:#}"), now + self.backoff(c.failures), c.failures + 1);
                return (r, o);
            }
            if !page.more {
                break;
            }
        }
        // A full sync lists everything: what it did not list was deleted meanwhile.
        if r.full {
            let gone: Vec<Item> = match self.ca.store().items(c.id) {
                Ok(all) => all
                    .into_iter()
                    .filter(|i| !i.deleted && !seen.contains(&i.provider_id))
                    .map(|i| Item { sealed: vec![], key_version: 0, etag: None, deleted: true, updated: now, ..i })
                    .collect(),
                Err(e) => {
                    let o = stop(&mut r, FAILED, format!("reading items: {e:#}"), now + self.backoff(c.failures), c.failures + 1);
                    return (r, o);
                }
            };
            r.deletes += gone.len() as u32;
            if let Err(e) = self.ca.store().sync_batch(c.id, &gone, cursor.as_deref(), INCREMENTAL, now) {
                let o = stop(&mut r, FAILED, format!("storing: {e:#}"), now + self.backoff(c.failures), c.failures + 1);
                return (r, o);
            }
        }
        r.stage = INCREMENTAL.into();
        (r, Outcome { stage: INCREMENTAL.into(), failures: 0, next_attempt: now + self.poll_secs, last_sync: Some(now), last_error: None })
    }
}

impl Engine {
    /// Writes one queued change: the item's provider id when the provider took it (the
    /// item as it now holds it is stored and the change leaves the outbox), `None` when it
    /// refused it (the change stays, with why); an error stops the run.
    async fn push_one(&self, c: &Channel, keys: &KeySet, driver: &dyn Driver, credential: &[u8], p: &Pending, now: i64) -> Result<Option<String>, PushError> {
        let fail = |e: anyhow::Error| PushError::Transient(format!("{e:#}"));
        let data = if p.sealed.is_empty() {
            None
        } else {
            let s = SealedItem::decode(p.sealed.as_slice()).map_err(|e| fail(e.into()))?;
            Some(self.delegate.open_item(&c.account, keys, &c.class, &change_ref(c.id), &s).map_err(fail)?.to_vec())
        };
        let w = Write { provider_id: p.provider_id.clone(), base_etag: p.base_etag.clone(), data };
        let item = match driver.push(credential, &w).await {
            Ok(Change::Upsert { id, etag, data }) => {
                let s = self.delegate.seal_item(&c.account, keys, &c.class, &item_ref(c.id, &id), &data).map_err(fail)?;
                Item { id: 0, channel_id: c.id, provider_id: id, etag, key_version: s.version, sealed: s.encode_to_vec(), updated: now, deleted: false }
            }
            Ok(Change::Delete { id }) => Item { id: 0, channel_id: c.id, provider_id: id, etag: None, key_version: 0, sealed: vec![], updated: now, deleted: true },
            Err(PushError::Conflict(m) | PushError::Refused(m)) => {
                self.ca.store().change_refused(p.id, &m).map_err(fail)?;
                return Ok(None);
            }
            Err(e) => return Err(e),
        };
        self.ca.store().change_written(p.id, &item, now).map_err(fail)?;
        Ok(Some(item.provider_id))
    }

    /// The driver of `provider`, if the server has one.
    pub fn driver(&self, provider: &str) -> Option<&Arc<dyn Driver>> {
        self.drivers.get(provider)
    }
}

/// Adds a channel for `account` with its driver's credential, sealed under the account's
/// `tokens` key; needs the owner's consent to `tokens`. Signing an account in again
/// replaces the credential of a channel it already has and lets it run again.
#[allow(clippy::too_many_arguments)]
pub fn add_channel(ca: &Ca, delegate: &Delegate, account: &str, provider: &str, class: &str, name: &str, credential: &[u8], now: i64) -> Result<i64> {
    let row = ca.store().key_set(account)?.context("the account has no keys yet")?;
    let keys = KeySet::decode(row.keys.as_slice())?;
    anyhow::ensure!(delegate.consented(&keys, "tokens"), "the owner has not consented to the server holding tokens: `nitya-node keys consent tokens`");
    let id = ca.store().channel(account, provider, class, name, now)?;
    let sealed = delegate.seal_item(account, &keys, "tokens", &credential_ref(id), credential)?;
    ca.store().set_credential(id, &sealed.encode_to_vec(), now)?;
    let ch = ca.store().channel_by_id(id)?.context("the channel just made")?;
    if ch.stage == AUTH {
        let stage = if ch.cursor.is_some() { INCREMENTAL } else { NEW };
        ca.store().release_channel(id, &Outcome { stage: stage.into(), failures: 0, next_attempt: 0, last_sync: None, last_error: None }, now)?;
    }
    Ok(id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    const T: i64 = 1_800_000_000;

    /// The credential and cursor of one request.
    type Asked = (Vec<u8>, Option<Vec<u8>>);

    /// A provider that answers from a script, and records what it was asked.
    #[derive(Default)]
    struct Fake {
        script: Mutex<Vec<Result<Page, DriverError>>>,
        asked: Mutex<Vec<Asked>>,
        pushes: Mutex<Vec<Result<Change, PushError>>>,
        written: Mutex<Vec<Write>>,
    }

    #[tonic::async_trait]
    impl Driver for Fake {
        async fn fetch(&self, credential: &[u8], cursor: Option<&[u8]>) -> Result<Page, DriverError> {
            self.asked.lock().unwrap().push((credential.to_vec(), cursor.map(<[u8]>::to_vec)));
            let mut s = self.script.lock().unwrap();
            if s.is_empty() { Ok(Page::default()) } else { s.remove(0) }
        }

        async fn push(&self, _: &[u8], w: &Write) -> Result<Change, PushError> {
            self.written.lock().unwrap().push(w.clone());
            self.pushes.lock().unwrap().remove(0)
        }

        fn check(&self, _: &[u8]) -> Result<(), String> {
            Ok(())
        }
    }

    fn up(id: &str, data: &str) -> Change {
        Change::Upsert { id: id.into(), etag: Some(format!("e-{data}")), data: data.as_bytes().to_vec() }
    }

    fn page(changes: Vec<Change>, cursor: &str, more: bool) -> Result<Page, DriverError> {
        Ok(Page { changes, cursor: Some(cursor.as_bytes().to_vec()), more, credential: None })
    }

    struct Fixture {
        _dir: tempfile::TempDir,
        ca: Arc<Ca>,
        delegate: Arc<Delegate>,
        fake: Arc<Fake>,
        engine: Engine,
        uk: nitya_keys::Unlocked,
    }

    /// An account with keys, consent to `tokens` and `classes`, and the fake driver.
    fn fixture(classes: &[&str]) -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let ca = Arc::new(Ca::open(dir.path()).unwrap());
        let delegate = Arc::new(Delegate::load_or_generate(&dir.path().join("delegate.key")).unwrap());
        let wrap = crate::ca::tests::wrap_key();
        let mut uk = nitya_keys::create("ank", &[1; 32], &wrap, "pass").unwrap();
        for c in classes {
            uk.consent(c, &delegate.public()).unwrap();
        }
        ca.store().put_key_set("ank", 0, &uk.keys.encode_to_vec(), false, T).unwrap();
        let fake = Arc::new(Fake::default());
        let drivers: HashMap<String, Arc<dyn Driver>> = HashMap::from([("fake".to_string(), fake.clone() as Arc<dyn Driver>)]);
        let engine = Engine::new(ca.clone(), delegate.clone(), drivers, 60);
        Fixture { _dir: dir, ca, delegate, fake, engine, uk }
    }

    fn channel(f: &Fixture) -> i64 {
        add_channel(&f.ca, &f.delegate, "ank", "fake", "calendar", "primary", b"token-1", T).unwrap()
    }

    fn live(f: &Fixture, ch: i64) -> Vec<(String, bool)> {
        f.ca.store().items(ch).unwrap().into_iter().map(|i| (i.provider_id, i.deleted)).collect()
    }

    #[test]
    fn a_channel_needs_consent_to_tokens_to_be_added() {
        let f = fixture(&["calendar"]);
        assert!(add_channel(&f.ca, &f.delegate, "ank", "fake", "calendar", "primary", b"t", T).is_err());
    }

    #[tokio::test]
    async fn without_consent_to_the_class_a_channel_waits_and_says_so_once() {
        let f = fixture(&["tokens"]);
        let ch = channel(&f);
        let r = f.engine.sync_channel(ch, T).await.unwrap();
        assert_eq!(r.stage, CONSENT);
        f.engine.sync_channel(ch, T + 60).await.unwrap();
        assert!(f.fake.asked.lock().unwrap().is_empty());
        let log = f.ca.store().audit_since("ank", 0).unwrap();
        let waits = log.iter().filter(|e| String::from_utf8_lossy(&e.detail.clone().unwrap_or_default()).starts_with("waiting_for_consent")).count();
        assert_eq!(waits, 1);
    }

    #[tokio::test]
    async fn a_full_sync_then_incremental_ones_from_the_drivers_cursor() {
        let f = fixture(&["tokens", "calendar"]);
        let ch = channel(&f);
        *f.fake.script.lock().unwrap() = vec![page(vec![up("a", "A1"), up("b", "B1")], "c1", true), page(vec![up("c", "C1")], "c2", false)];
        let r = f.engine.sync_channel(ch, T).await.unwrap();
        assert_eq!((r.full, r.upserts, r.stage.as_str(), r.error), (true, 3, INCREMENTAL, None));
        *f.fake.script.lock().unwrap() = vec![page(vec![up("a", "A2"), Change::Delete { id: "b".into() }], "c3", false)];
        let r = f.engine.sync_channel(ch, T + 60).await.unwrap();
        assert_eq!((r.full, r.upserts, r.deletes), (false, 1, 1));
        let asked = f.fake.asked.lock().unwrap().clone();
        assert_eq!(asked.iter().map(|a| a.1.clone()).collect::<Vec<_>>(), vec![None, Some(b"c1".to_vec()), Some(b"c2".to_vec())]);
        assert!(asked.iter().all(|a| a.0 == b"token-1"));
        assert_eq!(live(&f, ch), vec![("a".into(), false), ("b".into(), true), ("c".into(), false)]);
        // The items are sealed under the calendar key: a device reads them, bound to their place.
        let a = f.ca.store().items(ch).unwrap().remove(0);
        let s = SealedItem::decode(a.sealed.as_slice()).unwrap();
        assert_eq!(&f.uk.open_item("calendar", &item_ref(ch, "a"), &s).unwrap()[..], b"A2");
        let c = f.ca.store().channel_by_id(ch).unwrap().unwrap();
        assert_eq!((c.cursor, c.next_attempt, c.last_sync), (Some(b"c3".to_vec()), T + 60 + 60, Some(T + 60)));
        assert_eq!(f.ca.store().runs(ch).unwrap().len(), 2);
    }

    #[tokio::test]
    async fn a_forgotten_cursor_means_a_full_sync_that_tombstones_what_is_gone() {
        let f = fixture(&["tokens", "calendar"]);
        let ch = channel(&f);
        *f.fake.script.lock().unwrap() = vec![page(vec![up("a", "A"), up("b", "B")], "c1", false)];
        f.engine.sync_channel(ch, T).await.unwrap();
        *f.fake.script.lock().unwrap() = vec![Err(DriverError::CursorExpired), page(vec![up("a", "A")], "c9", false)];
        let r = f.engine.sync_channel(ch, T + 60).await.unwrap();
        // a is listed again unchanged (its etag): kept as it is, not stored again
        assert_eq!((r.full, r.upserts, r.deletes, r.error), (true, 0, 1, None));
        assert_eq!(live(&f, ch), vec![("a".into(), false), ("b".into(), true)]);
    }

    fn queue(f: &Fixture, ch: i64, id: Option<&str>, data: &[u8]) -> i64 {
        let sealed = if data.is_empty() { vec![] } else { f.uk.seal_item("calendar", &change_ref(ch), data).unwrap().encode_to_vec() };
        let p = Pending { id: 0, channel_id: ch, provider_id: id.map(str::to_string), base_etag: id.map(|_| "e-A".into()), sealed, actor: "node:x".into(), created: T, error: None };
        f.ca.store().queue_change(&p).unwrap()
    }

    #[tokio::test]
    async fn queued_changes_are_written_first_and_a_refused_one_stays_with_why() {
        let f = fixture(&["tokens", "calendar"]);
        let ch = channel(&f);
        *f.fake.script.lock().unwrap() = vec![page(vec![up("a", "A"), up("b", "B")], "c1", false)];
        f.engine.sync_channel(ch, T).await.unwrap();
        queue(&f, ch, None, b"NEW");
        queue(&f, ch, Some("a"), b"A-edited");
        queue(&f, ch, Some("b"), b"");
        *f.fake.pushes.lock().unwrap() = vec![Ok(up("n", "NEW")), Err(PushError::Conflict("changed at the provider".into())), Ok(Change::Delete { id: "b".into() })];
        let r = f.engine.sync_channel(ch, T + 60).await.unwrap();
        assert_eq!((r.written.iter().map(|w| w.1.as_str()).collect::<Vec<_>>(), r.refused, r.error), (vec!["n", "b"], 1, None));
        // the server opened what the device sealed, and wrote it in order
        let w = f.fake.written.lock().unwrap().clone();
        assert_eq!(w.iter().map(|w| w.data.clone()).collect::<Vec<_>>(), vec![Some(b"NEW".to_vec()), Some(b"A-edited".to_vec()), None]);
        assert_eq!(w[1].base_etag.as_deref(), Some("e-A"));
        assert_eq!(live(&f, ch), vec![("a".into(), false), ("b".into(), true), ("n".into(), false)]);
        let left = f.ca.store().pending(ch).unwrap();
        assert_eq!((left.len(), left[0].error.as_deref()), (1, Some("changed at the provider")));
        // a refused change is not tried again
        f.engine.sync_channel(ch, T + 120).await.unwrap();
        assert_eq!(f.fake.written.lock().unwrap().len(), 3);
        let log = f.ca.store().audit_since("ank", 0).unwrap();
        assert!(log.iter().any(|e| String::from_utf8_lossy(&e.detail.clone().unwrap_or_default()).contains("; 2 written, 1 refused")));
    }

    #[tokio::test]
    async fn a_change_that_fails_for_now_stops_the_run_and_waits_for_the_next() {
        let f = fixture(&["tokens", "calendar"]);
        let ch = channel(&f);
        queue(&f, ch, None, b"NEW");
        *f.fake.pushes.lock().unwrap() = vec![Err(PushError::Transient("503".into()))];
        let r = f.engine.sync_channel(ch, T).await.unwrap();
        assert_eq!(r.stage, FAILED);
        assert!(f.fake.asked.lock().unwrap().is_empty(), "no fetch after a failed write");
        let left = f.ca.store().pending(ch).unwrap();
        assert_eq!((left.len(), left[0].error.clone()), (1, None));
    }

    #[tokio::test]
    async fn failures_back_off_and_a_refused_credential_waits_for_a_person() {
        let f = fixture(&["tokens", "calendar"]);
        let ch = channel(&f);
        *f.fake.script.lock().unwrap() = vec![Err(DriverError::Transient("503".into())), Err(DriverError::Transient("503".into()))];
        let r = f.engine.sync_channel(ch, T).await.unwrap();
        assert_eq!((r.stage.as_str(), r.error.as_deref()), (FAILED, Some("503")));
        f.engine.sync_channel(ch, T + 60).await.unwrap();
        let c = f.ca.store().channel_by_id(ch).unwrap().unwrap();
        assert_eq!((c.failures, c.next_attempt), (2, T + 60 + 120));
        // Not due yet, then due.
        assert!(f.engine.run_due(T + 100).await.unwrap().is_empty());
        *f.fake.script.lock().unwrap() = vec![Err(DriverError::Auth("invalid_grant".into()))];
        let r = f.engine.run_due(T + 200).await.unwrap();
        assert_eq!(r[0].stage, AUTH);
        // The poller leaves it alone until someone signs in again.
        assert!(f.engine.run_due(T + 100_000).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_rotated_credential_is_kept_sealed_and_used_next_time() {
        let f = fixture(&["tokens", "calendar"]);
        let ch = channel(&f);
        *f.fake.script.lock().unwrap() = vec![Ok(Page { credential: Some(b"token-2".to_vec()), cursor: Some(b"c1".to_vec()), ..Default::default() })];
        f.engine.sync_channel(ch, T).await.unwrap();
        f.engine.sync_channel(ch, T + 60).await.unwrap();
        assert_eq!(f.fake.asked.lock().unwrap()[1].0, b"token-2");
        let c = f.ca.store().channel_by_id(ch).unwrap().unwrap();
        assert!(!String::from_utf8_lossy(&c.credential.unwrap()).contains("token-2"));
    }

    #[tokio::test]
    async fn a_channel_runs_once_at_a_time_and_a_dead_run_is_taken_over() {
        let f = fixture(&["tokens", "calendar"]);
        let ch = channel(&f);
        assert!(f.ca.store().claim_channel(ch, T, STALE_SECS).unwrap());
        let r = f.engine.sync_channel(ch, T + 1).await.unwrap();
        assert_eq!(r.error.as_deref(), Some("a run of this channel is in progress"));
        let r = f.engine.sync_channel(ch, T + STALE_SECS + 1).await.unwrap();
        assert_eq!((r.stage.as_str(), r.error), (INCREMENTAL, None));
    }

    #[tokio::test]
    async fn runs_that_change_something_are_in_a_verifiable_audit_chain() {
        let f = fixture(&["tokens", "calendar"]);
        let ch = channel(&f);
        *f.fake.script.lock().unwrap() = vec![page(vec![up("a", "A")], "c1", false)];
        f.engine.sync_channel(ch, T).await.unwrap();
        // A quiet poll is not logged.
        f.engine.sync_channel(ch, T + 60).await.unwrap();
        let rows = f.ca.store().audit_since("ank", 0).unwrap();
        let entries: Vec<_> = rows
            .into_iter()
            .map(|r| nitya_audit::SignedEntry { entry: r.entry, detail: r.detail.unwrap_or_default(), hash: r.hash, signature: r.signature })
            .collect();
        let key = ed25519_dalek::VerifyingKey::from_bytes(&f.ca.audit_public_key()).unwrap();
        let (_, log) = nitya_audit::verify(&key, "ank", None, &entries).unwrap();
        let syncs: Vec<_> = log.iter().filter(|e| e.action == "sync.run").map(|e| (e.actor.as_str(), e.resource.clone())).collect();
        assert_eq!(syncs, vec![("system:sync-delegate", format!("channel:{ch}"))]);
        assert_eq!(String::from_utf8_lossy(&entries.last().unwrap().detail), "full sync, 1 upserted, 0 deleted");
    }
}
