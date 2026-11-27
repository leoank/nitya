//! The `nitya.v1.Channels` service (ADR-0017): a device lists its account's channels and
//! syncs them now, through the guard (Cedar, then the audit log).

use std::sync::Arc;

use nitya_device as dev;
use nitya_proto::v1::channels_server::Channels;
use nitya_proto::v1::{ChannelInfo, ListChannelsRequest, ListChannelsResponse, SyncNowRequest, SyncNowResponse, SyncReport};
use prost::Message;
use tonic::{Request, Response, Status};

use crate::audit::guard;
use crate::ca::Ca;
use crate::devices::caller;
use crate::policy::{Policy, Resource};
use crate::sync::Engine;

pub struct Service {
    pub ca: Arc<Ca>,
    pub policy: Arc<Policy>,
    pub engine: Arc<Engine>,
}

fn internal(e: anyhow::Error) -> Status {
    Status::internal(format!("{e:#}"))
}

#[tonic::async_trait]
impl Channels for Service {
    async fn list(&self, req: Request<ListChannelsRequest>) -> Result<Response<ListChannelsResponse>, Status> {
        let (node, _) = caller(&req).await?;
        let body = req.get_ref().encode_to_vec();
        let c = self.ca.authorize(req.metadata(), "nitya.v1.Channels/List", &body, &node, dev::now())?;
        guard(&self.ca, &self.policy, &c, "channels.read", Resource::Channels { user: c.account.clone() }, "")?;
        let channels = self
            .ca
            .store()
            .channels(&c.account)
            .map_err(internal)?
            .into_iter()
            .map(|ch| ChannelInfo {
                id: ch.id,
                provider: ch.provider,
                class: ch.class,
                name: ch.name,
                stage: ch.stage,
                last_sync: ch.last_sync.unwrap_or(0),
                last_error: ch.last_error.unwrap_or_default(),
                next_attempt: ch.next_attempt,
            })
            .collect();
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
            reports.push(SyncReport { channel_id: r.channel, stage: r.stage, full: r.full, upserts: r.upserts, deletes: r.deletes, error: r.error.unwrap_or_default() });
        }
        Ok(Response::new(SyncNowResponse { reports }))
    }
}
