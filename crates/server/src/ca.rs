//! The device CA (ADR-0005, ADR-0011): one-time enrollment codes, certificates bound to a
//! tailnet node id, signed requests, renewal and revocation by device key.
//!
//! Until the store (phase 1 item 4) its state is files under one directory, which the
//! admin commands write too: `ca.key` (the CA's Ed25519 seed), `codes/<sha256 of code>`,
//! `issued/<serial>.cert` and `revoked/<device key in hex>`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::{Context, Result, bail};
use ed25519_dalek::SigningKey;
use nitya_device as dev;
use nitya_proto::v1::{Cert, CertBody};
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
    dir: PathBuf,
    key: SigningKey,
    /// Nonces seen within the clock window, with their timestamps.
    nonces: Mutex<HashMap<Vec<u8>, i64>>,
}

impl Ca {
    /// The CA in `dir`, made on first use.
    pub fn open(dir: &Path) -> Result<Ca> {
        use std::os::unix::fs::PermissionsExt;
        for d in ["", "codes", "issued", "revoked"] {
            std::fs::create_dir_all(dir.join(d)).with_context(|| format!("creating {}", dir.join(d).display()))?;
        }
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
        let key = dev::load_or_generate(&dir.join("ca.key"))?;
        Ok(Ca { dir: dir.to_path_buf(), key, nonces: Mutex::default() })
    }

    pub fn public_key(&self) -> [u8; 32] {
        self.key.verifying_key().to_bytes()
    }

    fn code_path(&self, code: &str) -> PathBuf {
        self.dir.join("codes").join(dev::hex(&Sha256::digest(code.trim().as_bytes())))
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
        dev::write_private(&self.code_path(&code), format!("{account}\n{kind}\n{}\n", now + secs).as_bytes())?;
        Ok(code)
    }

    /// Enrolls the device holding `device_key`, calling from tailnet node `node_id`, with
    /// a code from `new_code`. The code is used up whatever the outcome after the proof.
    pub fn enroll(&self, code: &str, device_key: &[u8], proof: &[u8], node_id: &str, hostname: &str, now: i64) -> Result<Cert, Refused> {
        dev::verify_enroll_proof(device_key, code.trim(), proof).map_err(unauth)?;
        if node_id.is_empty() {
            return Err(Refused::Unauthenticated("the tailnet does not name the caller's node".into()));
        }
        // Claiming the code is a rename: of two enrollments with one code, one wins.
        let path = self.code_path(code);
        let mut tag = [0u8; 8];
        rand::rngs::OsRng.fill_bytes(&mut tag);
        let claimed = path.with_extension(format!("claimed-{}", dev::hex(&tag)));
        std::fs::rename(&path, &claimed).map_err(|_| Refused::Denied("unknown or used enrollment code".into()))?;
        let text = std::fs::read_to_string(&claimed).unwrap_or_default();
        let _ = std::fs::remove_file(&claimed);
        let [account, kind, expires] = text.lines().collect::<Vec<_>>()[..] else {
            return Err(Refused::Denied("unreadable enrollment code".into()));
        };
        if expires.parse::<i64>().map_or(true, |e| now > e) {
            return Err(Refused::Denied("the enrollment code has expired".into()));
        }
        if self.is_revoked(device_key) {
            return Err(Refused::Denied("this device key is revoked".into()));
        }
        self.issue(CertBody {
            serial: 0,
            account: account.into(),
            kind: kind.into(),
            device_key: device_key.to_vec(),
            node_id: node_id.into(),
            hostname: hostname.into(),
            not_before: 0,
            not_after: 0,
        }, now)
        .map_err(|e| Refused::Denied(format!("issuing: {e:#}")))
    }

    fn issue(&self, mut body: CertBody, now: i64) -> Result<Cert> {
        body.serial = rand::rngs::OsRng.next_u64() >> 1;
        body.not_before = now;
        body.not_after = now + dev::CERT_LIFETIME_SECS;
        let cert = dev::issue(&self.key, &body);
        dev::write_private(&self.dir.join("issued").join(format!("{}.cert", body.serial)), &cert.encode_to_vec())?;
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

    fn revoked_path(&self, device_key: &[u8]) -> PathBuf {
        self.dir.join("revoked").join(dev::hex(device_key))
    }

    fn is_revoked(&self, device_key: &[u8]) -> bool {
        self.revoked_path(device_key).exists()
    }

    /// Revokes the device that certificate `serial` was issued to, for good.
    pub fn revoke(&self, serial: u64) -> Result<CertBody> {
        let cert = self.read_issued(&self.dir.join("issued").join(format!("{serial}.cert")))?;
        dev::write_private(&self.revoked_path(&cert.device_key), b"")?;
        Ok(cert)
    }

    fn read_issued(&self, path: &Path) -> Result<CertBody> {
        let bytes = std::fs::read(path).with_context(|| format!("no certificate {}", path.display()))?;
        dev::verify_cert(&self.key.verifying_key(), &Cert::decode(bytes.as_slice())?)
    }

    /// Each enrolled device's latest certificate, and whether the device is revoked.
    pub fn devices(&self) -> Result<Vec<(CertBody, bool)>> {
        let mut latest: HashMap<Vec<u8>, CertBody> = HashMap::new();
        for e in std::fs::read_dir(self.dir.join("issued"))? {
            let p = e?.path();
            if p.extension().is_some_and(|x| x == "cert") {
                let c = self.read_issued(&p)?;
                if latest.get(&c.device_key).is_none_or(|o| o.not_after < c.not_after) {
                    latest.insert(c.device_key.clone(), c);
                }
            }
        }
        let mut v: Vec<_> = latest.into_values().map(|c| { let r = self.is_revoked(&c.device_key); (c, r) }).collect();
        v.sort_by(|a, b| (&a.0.account, &a.0.hostname).cmp(&(&b.0.account, &b.0.hostname)));
        Ok(v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nitya_proto::v1::WhoAmIRequest;

    const WHO: &str = "nitya.v1.Devices/WhoAmI";
    const T: i64 = 1_800_000_000;

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
        let cert = ca.enroll(&code, &key, &dev::enroll_proof(&dev, &code), "n1", "rogue", T).unwrap();
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
    }

    #[test]
    fn a_code_works_once_and_not_after_it_expires() {
        let f = enrolled();
        let d = dev::generate();
        let key = d.verifying_key().to_bytes();
        let code = f.ca.new_code("ank", "app", 900, T).unwrap();
        let proof = dev::enroll_proof(&d, &code);
        assert!(f.ca.enroll(&code, &key, &proof, "n2", "phone", T).is_ok());
        assert!(matches!(f.ca.enroll(&code, &key, &proof, "n2", "phone", T), Err(Refused::Denied(_))));
        let late = f.ca.new_code("ank", "app", 900, T).unwrap();
        let proof = dev::enroll_proof(&d, &late);
        assert_eq!(f.ca.enroll(&late, &key, &proof, "n2", "phone", T + 901), Err(Refused::Denied("the enrollment code has expired".into())));
        assert!(f.ca.new_code("ank", "admin", 900, T).is_err());
    }

    #[test]
    fn a_proof_for_another_key_does_not_use_up_the_code() {
        let f = enrolled();
        let (d, thief) = (dev::generate(), dev::generate());
        let code = f.ca.new_code("ank", "node", 900, T).unwrap();
        let bad = f.ca.enroll(&code, &thief.verifying_key().to_bytes(), &dev::enroll_proof(&d, &code), "n3", "x", T);
        assert!(matches!(bad, Err(Refused::Unauthenticated(_))));
        assert!(f.ca.enroll(&code, &d.verifying_key().to_bytes(), &dev::enroll_proof(&d, &code), "n3", "x", T).is_ok());
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
        assert_eq!((&old.account, &old.device_key, &old.node_id), (&new.account, &new.device_key, &new.node_id));
        assert_eq!(new.not_before, later);
        let too_late = T + dev::CERT_LIFETIME_SECS + dev::RENEW_GRACE_SECS + 1;
        assert!(matches!(f.ca.renew(&signed(&f, RENEW, too_late), RENEW, &[], "n1", too_late), Err(Refused::Unauthenticated(_))));
    }

    #[test]
    fn a_revoked_device_is_refused_everything() {
        let f = enrolled();
        const RENEW: &str = "nitya.v1.Devices/Renew";
        let serial = dev::verify_cert(&f.ca.key.verifying_key(), &f.cert).unwrap().serial;
        f.ca.revoke(serial).unwrap();
        assert_eq!(f.ca.authorize(&signed(&f, WHO, T), WHO, &[], "n1", T), Err(Refused::Denied("this device is revoked".into())));
        assert!(matches!(f.ca.renew(&signed(&f, RENEW, T), RENEW, &[], "n1", T), Err(Refused::Denied(_))));
        let code = f.ca.new_code("ank", "node", 900, T).unwrap();
        let again = f.ca.enroll(&code, &f.dev.verifying_key().to_bytes(), &dev::enroll_proof(&f.dev, &code), "n1", "rogue", T);
        assert_eq!(again, Err(Refused::Denied("this device key is revoked".into())));
        let list = f.ca.devices().unwrap();
        assert_eq!(list.len(), 1);
        assert!(list[0].1);
    }

    #[test]
    fn the_state_is_private_and_survives_a_restart() {
        use std::os::unix::fs::PermissionsExt;
        let f = enrolled();
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&f.ca.dir), 0o700);
        assert_eq!(mode(&f.ca.dir.join("ca.key")), 0o600);
        let again = Ca::open(&f.ca.dir).unwrap();
        assert_eq!(again.public_key(), f.ca.public_key());
        assert!(again.authorize(&signed(&f, WHO, T), WHO, &[], "n1", T).is_ok());
    }
}
