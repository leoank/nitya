//! The `nitya.v1.Channels` service (ADR-0017, ADR-0018): a device lists its account's
//! channels and syncs them now; signs a provider account in and makes channels of its
//! calendars; reads a channel's items, sealed, and writes its changes to one. Every call
//! goes through the guard (Cedar, then the audit log).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use nitya_device as dev;
use nitya_keys::{Delegate, KeySet};
use nitya_proto::v1::channels_server::Channels;
use nitya_proto::v1::{
    AddRequest, AddResponse, CalendarInfo, CertBody, ChannelInfo, ExchangeRequest, ExchangeResponse, ItemsRequest, ItemsResponse, ListChannelsRequest,
    ListChannelsResponse, PendingChange, SealedItem, SignInRequest, SignInResponse, StoredItem, SyncNowRequest, SyncNowResponse, SyncReport, WriteRequest,
    WriteResponse,
};
use nitya_store::{Channel, Pending};
use prost::Message;
use rand::RngCore;
use tonic::{Request, Response, Status};

use crate::audit::guard;
use crate::ca::{Ca, actor};
use crate::devices::caller;
use crate::google::{self, Google};
use crate::policy::{Policy, Resource};
use crate::sync::{self, Engine};

/// How long a sign-in's tokens wait in memory for the calendars to be chosen.
const SIGN_IN_SECS: i64 = 600;

/// An account signed in, not yet made into channels.
struct SignedIn {
    account: String,
    refresh_token: String,
    calendars: Vec<String>,
    expires: i64,
}

pub struct Service {
    pub ca: Arc<Ca>,
    pub policy: Arc<Policy>,
    pub engine: Arc<Engine>,
    pub delegate: Arc<Delegate>,
    pub google: Option<Arc<Google>>,
    signed_in: Mutex<HashMap<String, SignedIn>>,
}

impl Service {
    pub fn new(ca: Arc<Ca>, policy: Arc<Policy>, engine: Arc<Engine>, delegate: Arc<Delegate>, google: Option<Arc<Google>>) -> Service {
        Service { ca, policy, engine, delegate, google, signed_in: Mutex::new(HashMap::new()) }
    }

    fn google(&self, provider: &str) -> Result<&Google, Status> {
        if provider != google::PROVIDER {
            return Err(Status::invalid_argument(format!("no sign-in for {provider}: google only, for now")));
        }
        self.google.as_deref().ok_or_else(|| Status::failed_precondition("the server has no Google client: start it with --google-client"))
    }

    /// The caller's channel `id`, or not found.
    fn channel(&self, c: &CertBody, id: i64) -> Result<Channel, Status> {
        self.ca.store().channel_by_id(id).map_err(internal)?.filter(|ch| ch.account == c.account).ok_or_else(|| Status::not_found(format!("no channel {id} in this account")))
    }
}

fn internal(e: anyhow::Error) -> Status {
    Status::internal(format!("{e:#}"))
}

fn info(ch: Channel) -> ChannelInfo {
    ChannelInfo {
        id: ch.id,
        provider: ch.provider,
        class: ch.class,
        name: ch.name,
        stage: ch.stage,
        last_sync: ch.last_sync.unwrap_or(0),
        last_error: ch.last_error.unwrap_or_default(),
        next_attempt: ch.next_attempt,
    }
}

fn source(c: &CertBody, ch: &Channel) -> Resource {
    Resource::Source { id: ch.id.to_string(), user: c.account.clone(), class: ch.class.clone() }
}

/// Only calendars are read and written through nitya so far.
fn calendar_only(ch: &Channel) -> Result<(), Status> {
    if ch.class != "calendar" {
        return Err(Status::failed_precondition(format!("channel {} is {}: only calendar items are read and written so far", ch.id, ch.class)));
    }
    Ok(())
}

#[tonic::async_trait]
impl Channels for Service {
    async fn list(&self, req: Request<ListChannelsRequest>) -> Result<Response<ListChannelsResponse>, Status> {
        let (node, _) = caller(&req).await?;
        let body = req.get_ref().encode_to_vec();
        let c = self.ca.authorize(req.metadata(), "nitya.v1.Channels/List", &body, &node, dev::now())?;
        guard(&self.ca, &self.policy, &c, "channels.read", Resource::Channels { user: c.account.clone() }, "")?;
        let channels = self.ca.store().channels(&c.account).map_err(internal)?.into_iter().map(info).collect();
        Ok(Response::new(ListChannelsResponse { channels }))
    }

    async fn sync_now(&self, req: Request<SyncNowRequest>) -> Result<Response<SyncNowResponse>, Status> {
        let (node, _) = caller(&req).await?;
        let body = req.get_ref().encode_to_vec();
        let c = self.ca.authorize(req.metadata(), "nitya.v1.Channels/SyncNow", &body, &node, dev::now())?;
        let want = req.get_ref().channel_id;
        let detail = if want == 0 { "all".to_string() } else { format!("channel:{want}") };
        guard(&self.ca, &self.policy, &c, "channels.sync", Resource::Channels { user: c.account.clone() }, &detail)?;
        let ids: Vec<i64> = self.ca.store().channels(&c.account).map_err(internal)?.into_iter().map(|ch| ch.id).filter(|id| want == 0 || *id == want).collect();
        if want != 0 && ids.is_empty() {
            return Err(Status::not_found(format!("no channel {want} in this account")));
        }
        let mut reports = Vec::new();
        for id in ids {
            let r = self.engine.sync_channel(id, dev::now()).await.map_err(internal)?;
            reports.push(SyncReport {
                channel_id: r.channel,
                stage: r.stage,
                full: r.full,
                upserts: r.upserts,
                deletes: r.deletes,
                error: r.error.unwrap_or_default(),
                written: r.written.len() as u32,
                refused: r.refused,
            });
        }
        Ok(Response::new(SyncNowResponse { reports }))
    }

    async fn sign_in(&self, req: Request<SignInRequest>) -> Result<Response<SignInResponse>, Status> {
        let (node, _) = caller(&req).await?;
        let body = req.get_ref().encode_to_vec();
        let c = self.ca.authorize(req.metadata(), "nitya.v1.Channels/SignIn", &body, &node, dev::now())?;
        let r = req.into_inner();
        let g = self.google(&r.provider)?;
        guard(&self.ca, &self.policy, &c, "channels.add", Resource::Channels { user: c.account.clone() }, &format!("{}: sign-in", r.provider))?;
        Ok(Response::new(SignInResponse { url: g.auth_url(&r.redirect_uri, &r.code_challenge, &r.state) }))
    }

    async fn exchange(&self, req: Request<ExchangeRequest>) -> Result<Response<ExchangeResponse>, Status> {
        let (node, _) = caller(&req).await?;
        let body = req.get_ref().encode_to_vec();
        let c = self.ca.authorize(req.metadata(), "nitya.v1.Channels/Exchange", &body, &node, dev::now())?;
        let r = req.into_inner();
        let g = self.google(&r.provider)?;
        guard(&self.ca, &self.policy, &c, "channels.add", Resource::Channels { user: c.account.clone() }, &format!("{}: exchange", r.provider))?;
        let refresh = g.exchange(&r.code, &r.code_verifier, &r.redirect_uri).await.map_err(|e| Status::permission_denied(format!("{e:#}")))?;
        let cals = g.calendars(&refresh).await.map_err(|e| Status::unavailable(format!("listing the calendars: {e:#}")))?;
        let have: HashMap<String, i64> =
            self.ca.store().channels(&c.account).map_err(internal)?.into_iter().filter(|ch| ch.provider == google::PROVIDER).map(|ch| (ch.name, ch.id)).collect();
        let mut h = [0u8; 16];
        rand::thread_rng().fill_bytes(&mut h);
        let handle: String = h.iter().map(|b| format!("{b:02x}")).collect();
        let now = dev::now();
        let mut pending = self.signed_in.lock().expect("not poisoned");
        pending.retain(|_, s| s.expires > now);
        pending.insert(handle.clone(), SignedIn { account: c.account.clone(), refresh_token: refresh, calendars: cals.iter().map(|k| k.id.clone()).collect(), expires: now + SIGN_IN_SECS });
        let calendars = cals
            .into_iter()
            .map(|k| CalendarInfo { channel_id: have.get(&k.id).copied().unwrap_or(0), id: k.id, summary: k.summary, primary: k.primary, role: k.role })
            .collect();
        Ok(Response::new(ExchangeResponse { handle, calendars }))
    }

    async fn add(&self, req: Request<AddRequest>) -> Result<Response<AddResponse>, Status> {
        let (node, _) = caller(&req).await?;
        let body = req.get_ref().encode_to_vec();
        let c = self.ca.authorize(req.metadata(), "nitya.v1.Channels/Add", &body, &node, dev::now())?;
        let r = req.into_inner();
        guard(&self.ca, &self.policy, &c, "channels.add", Resource::Channels { user: c.account.clone() }, &format!("google: {} calendar(s)", r.calendar_ids.len()))?;
        let now = dev::now();
        let s = {
            let mut pending = self.signed_in.lock().expect("not poisoned");
            match pending.remove(&r.handle) {
                Some(s) if s.account == c.account && s.expires > now => s,
                _ => return Err(Status::failed_precondition("no such sign-in, or it expired: sign in again")),
            }
        };
        let mut channels = Vec::new();
        for id in &r.calendar_ids {
            if !s.calendars.contains(id) {
                return Err(Status::invalid_argument(format!("{id} is not a calendar of the account signed in")));
            }
            let cred = google::credential(&s.refresh_token, id);
            let ch = sync::add_channel(&self.ca, &self.delegate, &c.account, google::PROVIDER, "calendar", id, &cred, now).map_err(|e| Status::failed_precondition(format!("{e:#}")))?;
            channels.push(info(self.ca.store().channel_by_id(ch).map_err(internal)?.ok_or_else(|| Status::internal("the channel just made"))?));
        }
        Ok(Response::new(AddResponse { channels }))
    }

    async fn items(&self, req: Request<ItemsRequest>) -> Result<Response<ItemsResponse>, Status> {
        let (node, _) = caller(&req).await?;
        let body = req.get_ref().encode_to_vec();
        let c = self.ca.authorize(req.metadata(), "nitya.v1.Channels/Items", &body, &node, dev::now())?;
        let ch = self.channel(&c, req.get_ref().channel_id)?;
        calendar_only(&ch)?;
        guard(&self.ca, &self.policy, &c, "calendar.read", source(&c, &ch), &format!("channel:{}", ch.id))?;
        let store = self.ca.store();
        let items = store
            .items(ch.id)
            .map_err(internal)?
            .into_iter()
            .filter(|i| !i.deleted)
            .map(|i| Ok(StoredItem { sealed: Some(SealedItem::decode(i.sealed.as_slice()).map_err(|e| internal(e.into()))?), provider_id: i.provider_id, etag: i.etag.unwrap_or_default() }))
            .collect::<Result<_, Status>>()?;
        let pending = store
            .pending(ch.id)
            .map_err(internal)?
            .into_iter()
            .map(|p| PendingChange { id: p.id, delete: p.sealed.is_empty(), provider_id: p.provider_id.unwrap_or_default(), error: p.error.unwrap_or_default() })
            .collect();
        Ok(Response::new(ItemsResponse { class: ch.class, items, pending }))
    }

    async fn write(&self, req: Request<WriteRequest>) -> Result<Response<WriteResponse>, Status> {
        let (node, _) = caller(&req).await?;
        let body = req.get_ref().encode_to_vec();
        let c = self.ca.authorize(req.metadata(), "nitya.v1.Channels/Write", &body, &node, dev::now())?;
        let r = req.into_inner();
        let ch = self.channel(&c, r.channel_id)?;
        calendar_only(&ch)?;
        let what = match (&r.sealed, r.provider_id.as_str()) {
            (None, "") => return Err(Status::invalid_argument("a delete names its item")),
            (None, id) => format!("delete {id}"),
            (Some(_), "") => "new".to_string(),
            (Some(_), id) => format!("edit {id}"),
        };
        guard(&self.ca, &self.policy, &c, "calendar.write", source(&c, &ch), &format!("channel:{}: {what}", ch.id))?;
        let driver = self.engine.driver(&ch.provider).ok_or_else(|| Status::failed_precondition(format!("the server has no {} driver", ch.provider)))?;
        // What the provider could not hold is refused now, not after it is queued; that
        // needs the server to open it, as writing it will.
        if let Some(s) = &r.sealed {
            let keys = self.ca.store().key_set(&c.account).map_err(internal)?.and_then(|k| KeySet::decode(k.keys.as_slice()).ok());
            let keys = keys.filter(|k| self.delegate.consented(k, &ch.class)).ok_or_else(|| Status::failed_precondition(format!("the server writes {} only with consent: `keys consent {}`", ch.class, ch.class)))?;
            let data = self.delegate.open_item(&c.account, &keys, &ch.class, &sync::change_ref(ch.id), s).map_err(|e| Status::invalid_argument(format!("opening the change: {e:#}")))?;
            driver.check(&data).map_err(|why| Status::invalid_argument(format!("{} cannot hold it: {why}", ch.provider)))?;
        }
        let provider_id = Some(r.provider_id.clone()).filter(|p| !p.is_empty());
        let pending = Pending {
            id: 0,
            channel_id: ch.id,
            provider_id: provider_id.clone(),
            base_etag: Some(r.base_etag).filter(|e| !e.is_empty()),
            sealed: r.sealed.map(|s| s.encode_to_vec()).unwrap_or_default(),
            actor: actor(&c),
            created: dev::now(),
            error: None,
        };
        let id = self.ca.store().queue_change(&pending).map_err(internal)?;
        let report = self.engine.sync_channel(ch.id, dev::now()).await.map_err(internal)?;
        if let Some((_, pid)) = report.written.iter().find(|w| w.0 == id) {
            return Ok(Response::new(WriteResponse { state: "written".into(), provider_id: pid.clone(), error: String::new() }));
        }
        let left = self.ca.store().pending(ch.id).map_err(internal)?.into_iter().find(|p| p.id == id);
        Ok(Response::new(match left {
            Some(p) if p.error.is_some() => WriteResponse { state: "refused".into(), provider_id: provider_id.unwrap_or_default(), error: p.error.unwrap_or_default() },
            Some(_) => WriteResponse { state: "queued".into(), provider_id: provider_id.unwrap_or_default(), error: report.error.unwrap_or_default() },
            // Written by a run that held the channel meanwhile.
            None => WriteResponse { state: "written".into(), provider_id: provider_id.unwrap_or_default(), error: String::new() },
        }))
    }
}
