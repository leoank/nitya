//! The device CA (ADR-0005, ADR-0011): one-time enrollment codes, certificates bound to a
//! tailnet node id, signed requests, renewal and revocation by device key.
//!
//! Its records are in the store (ADR-0012), which the admin commands open too; its
//! signing key is the file `ca.key` beside the database, never in it.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;

use anyhow::{Context, Result, bail};
use ed25519_dalek::SigningKey;
use nitya_device as dev;
use nitya_proto::v1::{Cert, CertBody, KeySet};
use nitya_store::Store;
use prost::Message;
use rand::RngCore;
use sha2::{Digest, Sha256};
use tonic::metadata::MetadataMap;

pub const KINDS: [&str; 3] = ["node", "app", "agent"];

/// Why a call was refused: the caller did not prove who it is, or it did and may not.
#[derive(Debug, PartialEq, Eq)]
pub enum Refused {
    Unauthenticated(String),
    Denied(String),
}

impl From<Refused> for tonic::Status {
    fn from(r: Refused) -> Self {
        match r {
            Refused::Unauthenticated(m) => tonic::Status::unauthenticated(m),
            Refused::Denied(m) => tonic::Status::permission_denied(m),
        }
    }
}

fn unauth(e: impl std::fmt::Display) -> Refused {
    Refused::Unauthenticated(format!("{e:#}"))
}

pub struct Ca {
    key: SigningKey,
    store: Mutex<Store>,
    /// Nonces seen within the clock window, with their timestamps.
    nonces: Mutex<HashMap<Vec<u8>, i64>>,
}

impl Ca {
    /// The CA of the server's state directory `dir`, made on first use.
    pub fn open(dir: &Path) -> Result<Ca> {
        use std::os::unix::fs::PermissionsExt;
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
        let key = dev::load_or_generate(&dir.join("ca.key"))?;
        let store = Store::open(&dir.join("nitya.db"))?;
        Ok(Ca { key, store: Mutex::new(store), nonces: Mutex::default() })
    }

    pub(crate) fn store(&self) -> std::sync::MutexGuard<'_, Store> {
        self.store.lock().expect("store lock")
    }

    pub fn public_key(&self) -> [u8; 32] {
        self.key.verifying_key().to_bytes()
    }

    fn code_hash(code: &str) -> Vec<u8> {
        Sha256::digest(code.trim().as_bytes()).to_vec()
    }

    /// A one-time code for one device of `account` to enroll as `kind`, valid `secs`.
    /// Only its hash is kept.
    pub fn new_code(&self, account: &str, kind: &str, secs: i64, now: i64) -> Result<String> {
        if !KINDS.contains(&kind) {
            bail!("the kind is one of {}", KINDS.join(", "));
        }
        if account.is_empty() || account.contains('\n') {
            bail!("an account name is one non-empty line");
        }
        let mut b = [0u8; 16];
        rand::rngs::OsRng.fill_bytes(&mut b);
        let code = dev::hex(&b);
        self.store().put_code(&Self::code_hash(&code), account, kind, now + secs, now)?;
        Ok(code)
    }

    /// Enrolls the device holding `device_key`, with `wrap_key` for the account's keys,
    /// calling from tailnet node `node_id`, with a code from `new_code`. The code is used
    /// up whatever the outcome after the proof.
    #[allow(clippy::too_many_arguments)]
    pub fn enroll(&self, code: &str, device_key: &[u8], wrap_key: &str, proof: &[u8], node_id: &str, hostname: &str, now: i64) -> Result<Cert, Refused> {
        dev::verify_enroll_proof(device_key, code.trim(), wrap_key, proof).map_err(unauth)?;
        nitya_keys::recipient(wrap_key).map_err(unauth)?;
        if node_id.is_empty() {
            return Err(Refused::Unauthenticated("the tailnet does not name the caller's node".into()));
        }
        // Of two enrollments with one code, one claims it.
        let code = self
            .store()
            .claim_code(&Self::code_hash(code))
            .map_err(|e| Refused::Denied(format!("the store: {e:#}")))?
            .ok_or_else(|| Refused::Denied("unknown or used enrollment code".into()))?;
        if now > code.expires {
            return Err(Refused::Denied("the enrollment code has expired".into()));
        }
        if self.is_revoked(device_key) {
            return Err(Refused::Denied("this device key is revoked".into()));
        }
        self.issue(CertBody {
            serial: 0,
            account: code.account,
            kind: code.kind,
            device_key: device_key.to_vec(),
            node_id: node_id.into(),
            hostname: hostname.into(),
            not_before: 0,
            not_after: 0,
            wrap_key: wrap_key.into(),
        }, now)
        .map_err(|e| Refused::Denied(format!("issuing: {e:#}")))
    }

    fn issue(&self, mut body: CertBody, now: i64) -> Result<Cert> {
        body.serial = rand::rngs::OsRng.next_u64() >> 1;
        body.not_before = now;
        body.not_after = now + dev::CERT_LIFETIME_SECS;
        let cert = dev::issue(&self.key, &body);
        self.store().put_cert(body.serial, &body.account, &body.device_key, body.not_after, &cert.encode_to_vec(), now)?;
        Ok(cert)
    }

    /// The certificate of a signed request for `method` whose message encodes to `body`,
    /// from tailnet node `node_id` at `now`, if every check of ADR-0011 passes.
    pub fn authorize(&self, md: &MetadataMap, method: &str, body: &[u8], node_id: &str, now: i64) -> Result<CertBody, Refused> {
        self.check(md, method, body, node_id, now, 0)
    }

    /// A new certificate for the caller of a signed `Renew`, which may come up to 30 days
    /// after its certificate's end.
    pub fn renew(&self, md: &MetadataMap, method: &str, body: &[u8], node_id: &str, now: i64) -> Result<Cert, Refused> {
        let old = self.check(md, method, body, node_id, now, dev::RENEW_GRACE_SECS)?;
        self.issue(old, now).map_err(|e| Refused::Denied(format!("issuing: {e:#}")))
    }

    fn check(&self, md: &MetadataMap, method: &str, body: &[u8], node_id: &str, now: i64, grace: i64) -> Result<CertBody, Refused> {
        let proof = dev::read_proof(md).map_err(unauth)?;
        let cert = dev::verify_cert(&self.key.verifying_key(), &proof.cert).map_err(unauth)?;
        if now + dev::CLOCK_SKEW_SECS < cert.not_before {
            return Err(unauth("the certificate is not valid yet"));
        }
        if now > cert.not_after + grace {
            return Err(unauth(if grace == 0 { "the certificate has expired; renew it" } else { "the certificate is too old to renew; enroll again" }));
        }
        if self.is_revoked(&cert.device_key) {
            return Err(Refused::Denied("this device is revoked".into()));
        }
        if node_id.is_empty() || node_id != cert.node_id {
            return Err(Refused::Denied("the certificate belongs to another tailnet node".into()));
        }
        if (proof.ts - now).abs() > dev::CLOCK_SKEW_SECS {
            return Err(unauth("the request's timestamp is too far from the server's clock"));
        }
        dev::verify_request(&cert.device_key, &proof, method, body).map_err(unauth)?;
        let mut seen = self.nonces.lock().expect("nonce lock");
        seen.retain(|_, ts| (now - *ts).abs() <= 2 * dev::CLOCK_SKEW_SECS);
        if seen.insert(proof.nonce.clone(), proof.ts).is_some() {
            return Err(unauth("the request was replayed"));
        }
        Ok(cert)
    }

    /// Revoked, or unknown because the store failed: either way, refused.
    fn is_revoked(&self, device_key: &[u8]) -> bool {
        self.store().is_revoked(device_key).unwrap_or(true)
    }

    /// Revokes the device that certificate `serial` was issued to, for good; returns it,
    /// and whether it held a wrap of its account's keys, which then need rotating.
    pub fn revoke(&self, serial: u64, now: i64) -> Result<(CertBody, bool)> {
        let bytes = self.store().cert(serial)?.with_context(|| format!("no certificate with serial {serial}"))?;
        let cert = self.read(&bytes)?;
        self.store().revoke(&cert.device_key, now)?;
        let had_wrap = self.drop_wrap(&cert.account, &cert.device_key, now)?;
        Ok((cert, had_wrap))
    }

    /// Deletes a revoked device's wrap of the account's keys and flags the account: the
    /// next device to unlock rotates them (ADR-0010, ADR-0013).
    fn drop_wrap(&self, account: &str, device_key: &[u8], now: i64) -> Result<bool> {
        let store = self.store();
        loop {
            let Some(row) = store.key_set(account)? else { return Ok(false) };
            let mut keys = KeySet::decode(row.keys.as_slice())?;
            let before = keys.devices.len();
            keys.devices.retain(|d| d.device_key != device_key);
            if keys.devices.len() == before {
                return Ok(false);
            }
            // Another process (the server, or another admin command) may write in between.
            if store.put_key_set(account, row.revision, &keys.encode_to_vec(), true, now)?.is_some() {
                return Ok(true);
            }
        }
    }

    fn read(&self, bytes: &[u8]) -> Result<CertBody> {
        dev::verify_cert(&self.key.verifying_key(), &Cert::decode(bytes)?)
    }

    /// Each enrolled device's latest certificate, and whether the device is revoked.
    pub fn devices(&self) -> Result<Vec<(CertBody, bool)>> {
        let certs = self.store().latest_certs()?;
        let mut v = Vec::new();
        for b in certs {
            let c = self.read(&b)?;
            let r = self.is_revoked(&c.device_key);
            v.push((c, r));
        }
        v.sort_by(|a, b| (&a.0.account, &a.0.hostname).cmp(&(&b.0.account, &b.0.hostname)));
        Ok(v)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use nitya_proto::v1::WhoAmIRequest;

    const WHO: &str = "nitya.v1.Devices/WhoAmI";
    const T: i64 = 1_800_000_000;
    /// A wrap key (an age recipient) for the test devices.
    pub(crate) static W: std::sync::LazyLock<String> = std::sync::LazyLock::new(wrap_key);

    pub(crate) fn wrap_key() -> String {
        age::x25519::Identity::generate().to_public().to_string()
    }

    struct Fixture {
        _dir: tempfile::TempDir,
        ca: Ca,
        dev: SigningKey,
        cert: Cert,
    }

    fn enrolled() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let ca = Ca::open(dir.path()).unwrap();
        let dev = dev::generate();
        let code = ca.new_code("ank", "node", 900, T).unwrap();
        let key = dev.verifying_key().to_bytes();
        let cert = ca.enroll(&code, &key, &W, &dev::enroll_proof(&dev, &code, &W), "n1", "rogue", T).unwrap();
        Fixture { _dir: dir, ca, dev, cert }
    }

    fn signed(f: &Fixture, method: &str, ts: i64) -> MetadataMap {
        let mut req = tonic::Request::new(WhoAmIRequest {});
        dev::sign_request(&mut req, &f.dev, &f.cert, method, ts);
        req.metadata().clone()
    }

    #[test]
    fn enrollment_issues_a_certificate_bound_to_the_node() {
        let f = enrolled();
        let c = dev::verify_cert(&f.ca.key.verifying_key(), &f.cert).unwrap();
        assert_eq!((c.account.as_str(), c.kind.as_str(), c.node_id.as_str(), c.hostname.as_str()), ("ank", "node", "n1", "rogue"));
        assert_eq!(c.device_key, f.dev.verifying_key().to_bytes());
        assert_eq!(c.not_after - c.not_before, dev::CERT_LIFETIME_SECS);
        assert_eq!(c.wrap_key, *W);
    }

    #[test]
    fn a_code_works_once_and_not_after_it_expires() {
        let f = enrolled();
        let d = dev::generate();
        let key = d.verifying_key().to_bytes();
        let code = f.ca.new_code("ank", "app", 900, T).unwrap();
        let proof = dev::enroll_proof(&d, &code, &W);
        assert!(f.ca.enroll(&code, &key, &W, &proof, "n2", "phone", T).is_ok());
        assert!(matches!(f.ca.enroll(&code, &key, &W, &proof, "n2", "phone", T), Err(Refused::Denied(_))));
        let late = f.ca.new_code("ank", "app", 900, T).unwrap();
        let proof = dev::enroll_proof(&d, &late, &W);
        assert_eq!(f.ca.enroll(&late, &key, &W, &proof, "n2", "phone", T + 901), Err(Refused::Denied("the enrollment code has expired".into())));
        assert!(f.ca.new_code("ank", "admin", 900, T).is_err());
    }

    #[test]
    fn a_proof_for_another_key_does_not_use_up_the_code() {
        let f = enrolled();
        let (d, thief) = (dev::generate(), dev::generate());
        let code = f.ca.new_code("ank", "node", 900, T).unwrap();
        let bad = f.ca.enroll(&code, &thief.verifying_key().to_bytes(), &W, &dev::enroll_proof(&d, &code, &W), "n3", "x", T);
        assert!(matches!(bad, Err(Refused::Unauthenticated(_))));
        // A wrap key that is not an age recipient, or not the one the proof signed.
        let other = &wrap_key();
        assert!(f.ca.enroll(&code, &d.verifying_key().to_bytes(), "x", &dev::enroll_proof(&d, &code, "x"), "n3", "x", T).is_err());
        assert!(f.ca.enroll(&code, &d.verifying_key().to_bytes(), other, &dev::enroll_proof(&d, &code, &W), "n3", "x", T).is_err());
        assert!(f.ca.enroll(&code, &d.verifying_key().to_bytes(), &W, &dev::enroll_proof(&d, &code, &W), "n3", "x", T).is_ok());
    }

    #[test]
    fn a_signed_request_from_the_node_passes() {
        let f = enrolled();
        let c = f.ca.authorize(&signed(&f, WHO, T + 10), WHO, &[], "n1", T + 10).unwrap();
        assert_eq!(c.account, "ank");
    }

    #[test]
    fn every_failed_check_refuses() {
        let f = enrolled();
        let deny = |r: Result<CertBody, Refused>| matches!(r, Err(Refused::Denied(_)));
        let unauth = |r: Result<CertBody, Refused>| matches!(r, Err(Refused::Unauthenticated(_)));
        // Another tailnet node with a copy of the certificate and the key's signature.
        assert!(deny(f.ca.authorize(&signed(&f, WHO, T), WHO, &[], "n2", T)));
        // Signed for another method, or another body.
        assert!(unauth(f.ca.authorize(&signed(&f, "nitya.v1.Devices/Renew", T), WHO, &[], "n1", T)));
        assert!(unauth(f.ca.authorize(&signed(&f, WHO, T), WHO, b"x", "n1", T)));
        // The clock: a stale request, and an expired certificate.
        assert!(unauth(f.ca.authorize(&signed(&f, WHO, T), WHO, &[], "n1", T + 61)));
        let after = T + dev::CERT_LIFETIME_SECS + 1;
        assert!(unauth(f.ca.authorize(&signed(&f, WHO, after), WHO, &[], "n1", after)));
        // A replay of the same request.
        let md = signed(&f, WHO, T);
        assert!(f.ca.authorize(&md, WHO, &[], "n1", T).is_ok());
        assert_eq!(f.ca.authorize(&md, WHO, &[], "n1", T), Err(Refused::Unauthenticated("the request was replayed".into())));
        // A certificate from another CA, and no proof at all.
        let other = enrolled();
        assert!(unauth(f.ca.authorize(&signed(&other, WHO, T), WHO, &[], "n1", T)));
        assert!(unauth(f.ca.authorize(&MetadataMap::new(), WHO, &[], "n1", T)));
    }

    #[test]
    fn renewal_works_within_the_grace_and_keeps_the_binding() {
        let f = enrolled();
        const RENEW: &str = "nitya.v1.Devices/Renew";
        let later = T + dev::CERT_LIFETIME_SECS + 10 * 24 * 3600;
        let new = f.ca.renew(&signed(&f, RENEW, later), RENEW, &[], "n1", later).unwrap();
        let (old, new) = (dev::verify_cert(&f.ca.key.verifying_key(), &f.cert).unwrap(), dev::verify_cert(&f.ca.key.verifying_key(), &new).unwrap());
        assert_ne!(old.serial, new.serial);
        assert_eq!((&old.account, &old.device_key, &old.node_id, &old.wrap_key), (&new.account, &new.device_key, &new.node_id, &new.wrap_key));
        assert_eq!(new.not_before, later);
        let too_late = T + dev::CERT_LIFETIME_SECS + dev::RENEW_GRACE_SECS + 1;
        assert!(matches!(f.ca.renew(&signed(&f, RENEW, too_late), RENEW, &[], "n1", too_late), Err(Refused::Unauthenticated(_))));
    }

    #[test]
    fn a_revoked_device_is_refused_everything() {
        let f = enrolled();
        const RENEW: &str = "nitya.v1.Devices/Renew";
        let serial = dev::verify_cert(&f.ca.key.verifying_key(), &f.cert).unwrap().serial;
        assert!(!f.ca.revoke(serial, T).unwrap().1);
        assert_eq!(f.ca.authorize(&signed(&f, WHO, T), WHO, &[], "n1", T), Err(Refused::Denied("this device is revoked".into())));
        assert!(matches!(f.ca.renew(&signed(&f, RENEW, T), RENEW, &[], "n1", T), Err(Refused::Denied(_))));
        let code = f.ca.new_code("ank", "node", 900, T).unwrap();
        let again = f.ca.enroll(&code, &f.dev.verifying_key().to_bytes(), &W, &dev::enroll_proof(&f.dev, &code, &W), "n1", "rogue", T);
        assert_eq!(again, Err(Refused::Denied("this device key is revoked".into())));
        let list = f.ca.devices().unwrap();
        assert_eq!(list.len(), 1);
        assert!(list[0].1);
    }

    #[test]
    fn revoking_a_device_with_a_wrap_deletes_it_and_flags_the_account() {
        let f = enrolled();
        let c = dev::verify_cert(&f.ca.key.verifying_key(), &f.cert).unwrap();
        let u = nitya_keys::create("ank", &c.device_key, &c.wrap_key, "pass").unwrap();
        f.ca.store().put_key_set("ank", 0, &u.keys.encode_to_vec(), false, T).unwrap();
        assert!(f.ca.revoke(c.serial, T).unwrap().1);
        let row = f.ca.store().key_set("ank").unwrap().unwrap();
        assert!(row.rotation_pending);
        assert_eq!(row.revision, 2);
        assert!(KeySet::decode(row.keys.as_slice()).unwrap().devices.is_empty());
        // Again: nothing left to delete.
        assert!(!f.ca.revoke(c.serial, T).unwrap().1);
    }

    #[test]
    fn the_state_is_private_and_survives_a_restart() {
        use std::os::unix::fs::PermissionsExt;
        let f = enrolled();
        let dir = f._dir.path();
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(dir), 0o700);
        assert_eq!(mode(&dir.join("ca.key")), 0o600);
        assert_eq!(mode(&dir.join("nitya.db")), 0o600);
        let again = Ca::open(dir).unwrap();
        assert_eq!(again.public_key(), f.ca.public_key());
        assert!(again.authorize(&signed(&f, WHO, T), WHO, &[], "n1", T).is_ok());
    }
}
