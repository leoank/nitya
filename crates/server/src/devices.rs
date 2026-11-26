//! The `nitya.v1.Devices` service (ADR-0011): enrollment, renewal and who a caller is,
//! each tied to the tailnet node the call comes from.

use std::sync::Arc;

use nitya_device as dev;
use nitya_proto::v1::devices_server::Devices;
use nitya_proto::v1::{Cert, EnrollRequest, EnrollResponse, RenewRequest, WhoAmIRequest, WhoAmIResponse};
use nitya_tailnet::Remote;
use prost::Message;
use tonic::{Request, Response, Status};

use crate::ca::Ca;

pub struct Service {
    pub ca: Arc<Ca>,
}

/// The tailnet node and hostname of the caller, from the server's own tailnet status.
async fn caller<T>(req: &Request<T>) -> Result<(String, String), Status> {
    let remote = req.extensions().get::<Remote>().ok_or_else(|| Status::unauthenticated("not a tailnet connection"))?;
    let peer = remote.peer().await.ok_or_else(|| Status::unauthenticated("the tailnet does not name the caller"))?;
    Ok((peer.node_id, peer.host))
}

#[tonic::async_trait]
impl Devices for Service {
    async fn enroll(&self, req: Request<EnrollRequest>) -> Result<Response<EnrollResponse>, Status> {
        let (node, host) = caller(&req).await?;
        let r = req.get_ref();
        let cert = self.ca.enroll(&r.code, &r.device_key, &r.proof, &node, &host, dev::now())?;
        eprintln!("nitya-server: enrolled {host} (node {node})");
        Ok(Response::new(EnrollResponse { cert: Some(cert), ca_key: self.ca.public_key().to_vec() }))
    }

    async fn renew(&self, req: Request<RenewRequest>) -> Result<Response<Cert>, Status> {
        let (node, _) = caller(&req).await?;
        let body = req.get_ref().encode_to_vec();
        Ok(Response::new(self.ca.renew(req.metadata(), "nitya.v1.Devices/Renew", &body, &node, dev::now())?))
    }

    async fn who_am_i(&self, req: Request<WhoAmIRequest>) -> Result<Response<WhoAmIResponse>, Status> {
        let (node, _) = caller(&req).await?;
        let body = req.get_ref().encode_to_vec();
        let c = self.ca.authorize(req.metadata(), "nitya.v1.Devices/WhoAmI", &body, &node, dev::now())?;
        Ok(Response::new(WhoAmIResponse {
            account: c.account,
            kind: c.kind,
            serial: c.serial,
            node_id: c.node_id,
            hostname: c.hostname,
            not_after: c.not_after,
        }))
    }
}
