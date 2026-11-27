//! The `nitya.v1.Keys` service (ADR-0013): devices read their account's key set and write
//! it back whole. The server cannot check what is wrapped inside; it checks that a write
//! keeps every key version, wraps only to the account's unrevoked devices, keeps the
//! recovery key, and rotates when a revoked device held a wrap.

use std::sync::Arc;

use nitya_device as dev;
use nitya_keys::{CLASSES, KeySet};
use nitya_proto::v1::keys_server::Keys;
use nitya_proto::v1::{GetKeysRequest, GetKeysResponse, Member, PutKeysRequest, PutKeysResponse};
use prost::Message;
use tonic::{Request, Response, Status};

use crate::ca::Ca;
use crate::devices::caller;

pub struct Service {
    pub ca: Arc<Ca>,
    /// The server's sync-delegate key, which devices wrap consented DEKs to.
    pub delegate_key: String,
}

fn internal(e: anyhow::Error) -> Status {
    Status::internal(format!("{e:#}"))
}

impl Service {
    fn members(&self, account: &str) -> Result<Vec<Member>, Status> {
        Ok(self
            .ca
            .devices()
            .map_err(internal)?
            .into_iter()
            .filter(|(c, _)| c.account == account)
            .map(|(c, revoked)| Member { serial: c.serial, kind: c.kind, hostname: c.hostname, device_key: c.device_key, wrap_key: c.wrap_key, revoked })
            .collect())
    }
}

fn latest<'a>(versions: impl Iterator<Item = (&'a str, u32)>, class: &str) -> u32 {
    versions.filter(|(c, _)| *c == class).map(|(_, v)| v).max().unwrap_or(0)
}

/// Whether `new` may replace `old` (none yet: the account's first keys), and whether the
/// account still needs a rotation after it.
pub fn check_put(old: Option<&KeySet>, pending: bool, new: &KeySet, members: &[Member]) -> Result<bool, String> {
    if new.uk_version == 0 || new.recovery_public.is_empty() || new.recovery_secret.is_empty() || new.recovery_wrap.is_empty() {
        return Err("the keys need a user key version and a recovery key".into());
    }
    for c in CLASSES {
        if !new.deks.iter().any(|k| k.class == c) {
            return Err(format!("no {c} key"));
        }
    }
    if new.append_keys.is_empty() {
        return Err("no append key".into());
    }
    for d in &new.devices {
        if !members.iter().any(|m| m.device_key == d.device_key && !m.revoked) {
            return Err("a wrap for a device that is not an unrevoked device of this account".into());
        }
    }
    let Some(old) = old else { return Ok(false) };
    for k in &old.deks {
        if !new.deks.iter().any(|n| n.class == k.class && n.version == k.version) {
            return Err(format!("the write drops {} key version {}", k.class, k.version));
        }
    }
    for a in &old.append_keys {
        if !new.append_keys.iter().any(|n| n.version == a.version) {
            return Err(format!("the write drops append key version {}", a.version));
        }
    }
    if new.uk_version < old.uk_version {
        return Err("the write goes back to an older user key".into());
    }
    if !pending {
        return Ok(false);
    }
    let rotated = new.uk_version > old.uk_version
        && CLASSES.iter().all(|c| {
            latest(new.deks.iter().map(|k| (k.class.as_str(), k.version)), c) > latest(old.deks.iter().map(|k| (k.class.as_str(), k.version)), c)
        })
        && new.append_keys.iter().map(|a| a.version).max() > old.append_keys.iter().map(|a| a.version).max();
    if rotated { Ok(false) } else { Err("a device of this account was revoked: rotate the keys first".into()) }
}

#[tonic::async_trait]
impl Keys for Service {
    async fn get(&self, req: Request<GetKeysRequest>) -> Result<Response<GetKeysResponse>, Status> {
        let (node, _) = caller(&req).await?;
        let body = req.get_ref().encode_to_vec();
        let c = self.ca.authorize(req.metadata(), "nitya.v1.Keys/Get", &body, &node, dev::now())?;
        let row = self.ca.store().key_set(&c.account).map_err(internal)?;
        let (keys, revision, rotation_pending) = match row {
            Some(r) => (Some(KeySet::decode(r.keys.as_slice()).map_err(|e| Status::internal(e.to_string()))?), r.revision, r.rotation_pending),
            None => (None, 0, false),
        };
        Ok(Response::new(GetKeysResponse { keys, revision, rotation_pending, delegate_key: self.delegate_key.clone(), members: self.members(&c.account)? }))
    }

    async fn put(&self, req: Request<PutKeysRequest>) -> Result<Response<PutKeysResponse>, Status> {
        let (node, _) = caller(&req).await?;
        let body = req.get_ref().encode_to_vec();
        let c = self.ca.authorize(req.metadata(), "nitya.v1.Keys/Put", &body, &node, dev::now())?;
        let r = req.into_inner();
        let new = r.keys.ok_or_else(|| Status::invalid_argument("no keys"))?;
        let members = self.members(&c.account)?;
        let row = self.ca.store().key_set(&c.account).map_err(internal)?;
        let (old, revision, pending) = match row {
            Some(r) => (Some(KeySet::decode(r.keys.as_slice()).map_err(|e| Status::internal(e.to_string()))?), r.revision, r.rotation_pending),
            None => (None, 0, false),
        };
        let stale = || Status::aborted("the keys changed since they were read; read them again");
        if r.base_revision != revision {
            return Err(stale());
        }
        let pending = check_put(old.as_ref(), pending, &new, &members).map_err(Status::failed_precondition)?;
        let revision = self.ca.store().put_key_set(&c.account, revision, &new.encode_to_vec(), pending, dev::now()).map_err(internal)?.ok_or_else(stale)?;
        eprintln!("nitya-server: keys of {} now at revision {revision}, user key version {}", c.account, new.uk_version);
        Ok(Response::new(PutKeysResponse { revision }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn member(key: u8, revoked: bool) -> Member {
        Member { serial: key as u64, kind: "node".into(), hostname: "h".into(), device_key: vec![key; 32], wrap_key: crate::ca::tests::wrap_key(), revoked }
    }

    fn keys_of(a: &Member, b: &Member) -> (KeySet, Vec<u8>) {
        let mut u = nitya_keys::create("ank", &a.device_key, &a.wrap_key, "pass").unwrap();
        u.approve(&b.device_key, &b.wrap_key).unwrap();
        (u.keys.clone(), a.device_key.clone())
    }

    #[test]
    fn writes_that_lose_keys_or_wrap_to_strangers_are_refused() {
        let (a, b) = (member(1, false), member(2, false));
        let (keys, _) = keys_of(&a, &b);
        let members = vec![a.clone(), b.clone()];
        assert_eq!(check_put(None, false, &keys, &members), Ok(false));
        assert_eq!(check_put(Some(&keys), false, &keys, &members), Ok(false));
        // A wrap to a device of another account, or a revoked one.
        assert!(check_put(None, false, &keys, std::slice::from_ref(&a)).is_err());
        assert!(check_put(None, false, &keys, &[a.clone(), member(2, true)]).is_err());
        // Dropping a key version, or the recovery key.
        let mut lost = keys.clone();
        lost.deks.retain(|k| k.class != "mail");
        assert!(check_put(Some(&keys), false, &lost, &members).is_err());
        let mut lost = keys.clone();
        lost.recovery_wrap.clear();
        assert!(check_put(Some(&keys), false, &lost, &members).is_err());
    }

    #[test]
    fn after_a_revocation_only_a_rotation_is_accepted() {
        let (a, b) = (member(1, false), member(2, false));
        let (keys, _) = keys_of(&a, &b);
        let members = vec![a.clone(), member(2, true)];
        let mut without_b = keys.clone();
        without_b.devices.retain(|d| d.device_key != b.device_key);
        // What the server stored at the revocation: b's wrap gone, the account flagged.
        assert!(check_put(Some(&without_b), true, &without_b, &members).is_err());
        let mut u = nitya_keys::recover("ank", without_b.clone(), "pass").unwrap();
        u.rotate(|k| k == b.device_key.as_slice(), &crate::ca::tests::wrap_key()).unwrap();
        assert_eq!(check_put(Some(&without_b), true, &u.keys, &members), Ok(false));
    }
}
