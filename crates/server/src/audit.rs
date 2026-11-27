//! The `nitya.v1.Audit` service (ADR-0007, ADR-0015): devices read their account's chain
//! and hand back the head they verified. And the guard every call goes through: Cedar
//! decides, the decision goes into the log, and only then does the call act.

use std::sync::Arc;

use nitya_device as dev;
use nitya_proto::v1::audit_server::Audit;
use nitya_proto::v1::{CertBody, Checkpoint, SignedEntry, SinceRequest, SinceResponse, SubmitResponse};
use prost::Message;
use tonic::{Request, Response, Status};

use crate::ca::{Ca, Record, actor};
use crate::devices::caller;
use crate::policy::{Policy, Principal, Resource};

fn internal(e: anyhow::Error) -> Status {
    Status::internal(format!("{e:#}"))
}

fn resource_name(r: &Resource) -> String {
    match r {
        Resource::KeySet { .. } => "keys".into(),
        Resource::AuditLog { .. } => "audit".into(),
        Resource::Plan { id, .. } => format!("plan:{id}"),
        Resource::Source { id, .. } => format!("source:{id}"),
        Resource::Item { id, source, .. } => format!("item:{source}/{id}"),
    }
}

/// Asks Cedar whether the caller may do `action` on `resource`, writes the decision with
/// its policy ids to the account's audit log, and refuses unless it is allowed and logged.
pub fn guard(ca: &Ca, policy: &Policy, c: &CertBody, action: &str, resource: Resource, detail: &str) -> Result<(), Status> {
    let v = policy.decide(&Principal::of(c), action, &resource, false).map_err(internal)?;
    let r = Record {
        chain: c.account.clone(),
        actor: actor(c),
        action: action.into(),
        resource: resource_name(&resource),
        allowed: v.allowed,
        policies: v.policies.clone(),
        detail: detail.into(),
    };
    ca.record(r, dev::now()).map_err(internal)?;
    if v.allowed {
        return Ok(());
    }
    let why = if v.policies.is_empty() { "no policy permits it".to_string() } else { v.policies.join(", ") };
    Err(Status::permission_denied(format!("{action} refused: {why}")))
}

pub struct Service {
    pub ca: Arc<Ca>,
    pub policy: Arc<Policy>,
}

#[tonic::async_trait]
impl Audit for Service {
    async fn since(&self, req: Request<SinceRequest>) -> Result<Response<SinceResponse>, Status> {
        let (node, _) = caller(&req).await?;
        let body = req.get_ref().encode_to_vec();
        let c = self.ca.authorize(req.metadata(), "nitya.v1.Audit/Since", &body, &node, dev::now())?;
        // Reading the log is itself logged: each check adds one entry, which the next
        // check verifies.
        guard(&self.ca, &self.policy, &c, "audit.read", Resource::AuditLog { user: c.account.clone() }, "")?;
        let store = self.ca.store();
        let entries = store
            .audit_since(&c.account, req.get_ref().from)
            .map_err(internal)?
            .into_iter()
            .map(|r| SignedEntry { entry: r.entry, detail: r.detail.unwrap_or_default(), hash: r.hash, signature: r.signature })
            .collect();
        let checkpoints = store
            .checkpoints(&c.account)
            .map_err(internal)?
            .iter()
            .filter_map(|b| Checkpoint::decode(b.as_slice()).ok())
            .collect();
        Ok(Response::new(SinceResponse { entries, audit_key: self.ca.audit_public_key().to_vec(), checkpoints }))
    }

    async fn submit(&self, req: Request<Checkpoint>) -> Result<Response<SubmitResponse>, Status> {
        let (node, _) = caller(&req).await?;
        let body = req.get_ref().encode_to_vec();
        let c = self.ca.authorize(req.metadata(), "nitya.v1.Audit/Submit", &body, &node, dev::now())?;
        let cp = req.into_inner();
        let detail = format!("head {}", cp.seq);
        guard(&self.ca, &self.policy, &c, "audit.checkpoint", Resource::AuditLog { user: c.account.clone() }, &detail)?;
        if cp.device_key != c.device_key || cp.chain != c.account {
            return Err(Status::invalid_argument("a checkpoint is of the caller's own account, signed by its own key"));
        }
        nitya_audit::verify_checkpoint(&cp).map_err(|e| Status::invalid_argument(format!("{e:#}")))?;
        let store = self.ca.store();
        let at = store.audit_since(&c.account, cp.seq).map_err(internal)?;
        if at.first().map(|r| (r.seq, &r.hash)) != Some((cp.seq, &cp.hash)) {
            return Err(Status::failed_precondition("the checkpoint is not of an entry in this log"));
        }
        store.put_checkpoint(&c.account, &cp.device_key, &cp.encode_to_vec()).map_err(internal)?;
        Ok(Response::new(SubmitResponse {}))
    }
}
