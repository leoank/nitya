//! Device keys, certificates and signed requests (ADR-0005, ADR-0011), shared by the
//! server, which issues and checks them, and the node, which holds and uses them.

use std::path::Path;

use anyhow::{Context, Result, bail};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use nitya_proto::v1::{Cert, CertBody};
use prost::Message;
use rand::RngCore;
use sha2::{Digest, Sha256};
use tonic::metadata::{AsciiMetadataValue, BinaryMetadataValue, MetadataMap};

const CERT_TAG: &[u8] = b"nitya-cert-v1\0";
const ENROLL_TAG: &[u8] = b"nitya-enroll-v1\0";
const REQUEST_TAG: &[u8] = b"nitya-request-v1\0";

/// How long a certificate lives (ADR-0011).
pub const CERT_LIFETIME_SECS: i64 = 24 * 3600;
/// How long after its end a certificate may still be renewed.
pub const RENEW_GRACE_SECS: i64 = 30 * 24 * 3600;
/// How far a request's timestamp may be from the server's clock.
pub const CLOCK_SKEW_SECS: i64 = 60;

pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// A fresh Ed25519 key.
pub fn generate() -> SigningKey {
    let mut seed = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut seed);
    SigningKey::from_bytes(&seed)
}

/// An Ed25519 key kept as its 32-byte seed in `path`, readable only by this user; made
/// when the file does not exist.
pub fn load_or_generate(path: &Path) -> Result<SigningKey> {
    match std::fs::read(path) {
        Ok(b) => {
            let seed: [u8; 32] = b.try_into().map_err(|_| anyhow::anyhow!("{} is not a 32-byte key", path.display()))?;
            Ok(SigningKey::from_bytes(&seed))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let key = generate();
            write_private(path, key.as_bytes())?;
            Ok(key)
        }
        Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
    }
}

/// Writes `bytes` to `path` readable only by this user, replacing it atomically.
pub fn write_private(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let tmp = path.with_extension("tmp");
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&tmp)
        .with_context(|| format!("writing {}", tmp.display()))?;
    f.write_all(bytes)?;
    f.sync_all()?;
    std::fs::rename(&tmp, path).with_context(|| format!("writing {}", path.display()))
}

pub fn verifying_key(bytes: &[u8]) -> Result<VerifyingKey> {
    let b: [u8; 32] = bytes.try_into().map_err(|_| anyhow::anyhow!("an Ed25519 key is 32 bytes"))?;
    VerifyingKey::from_bytes(&b).context("not an Ed25519 key")
}

fn signature(bytes: &[u8]) -> Result<Signature> {
    Signature::from_slice(bytes).context("not an Ed25519 signature")
}

fn tagged(tag: &[u8], parts: &[&[u8]]) -> Vec<u8> {
    let mut m = tag.to_vec();
    for p in parts {
        m.extend_from_slice(&(p.len() as u64).to_be_bytes());
        m.extend_from_slice(p);
    }
    m
}

/// The CA signs a certificate body.
pub fn issue(ca: &SigningKey, body: &CertBody) -> Cert {
    let body = body.encode_to_vec();
    let signature = ca.sign(&tagged(CERT_TAG, &[&body])).to_bytes().to_vec();
    Cert { body, signature }
}

/// The body of a certificate the CA signed. Its dates are the caller's to check.
pub fn verify_cert(ca: &VerifyingKey, cert: &Cert) -> Result<CertBody> {
    ca.verify(&tagged(CERT_TAG, &[&cert.body]), &signature(&cert.signature)?)
        .context("the certificate is not signed by this server")?;
    CertBody::decode(cert.body.as_slice()).context("the certificate does not decode")
}

/// The device's proof, at enrollment, that it holds the key it enrolls.
pub fn enroll_proof(device: &SigningKey, code: &str) -> Vec<u8> {
    device
        .sign(&tagged(ENROLL_TAG, &[code.as_bytes(), device.verifying_key().as_bytes()]))
        .to_bytes()
        .to_vec()
}

pub fn verify_enroll_proof(device_key: &[u8], code: &str, proof: &[u8]) -> Result<()> {
    verifying_key(device_key)?
        .verify(&tagged(ENROLL_TAG, &[code.as_bytes(), device_key]), &signature(proof)?)
        .context("the enrollment proof does not verify")
}

const H_CERT: &str = "nitya-cert-bin";
const H_SIG: &str = "nitya-signature-bin";
const H_NONCE: &str = "nitya-nonce-bin";
const H_TS: &str = "nitya-timestamp";

fn request_message(method: &str, ts: i64, nonce: &[u8], body: &[u8]) -> Vec<u8> {
    let digest = Sha256::digest(body);
    tagged(REQUEST_TAG, &[method.as_bytes(), ts.to_string().as_bytes(), nonce, &digest])
}

/// Signs a request for `method` (`nitya.v1.Devices/WhoAmI`) at time `ts`: the
/// certificate, the timestamp, a fresh nonce and the signature go in its metadata.
pub fn sign_request<T: Message>(req: &mut tonic::Request<T>, device: &SigningKey, cert: &Cert, method: &str, ts: i64) {
    let mut nonce = [0u8; 16];
    rand::rngs::OsRng.fill_bytes(&mut nonce);
    let body = req.get_ref().encode_to_vec();
    let sig = device.sign(&request_message(method, ts, &nonce, &body));
    let md = req.metadata_mut();
    md.insert_bin(H_CERT, BinaryMetadataValue::from_bytes(&cert.encode_to_vec()));
    md.insert_bin(H_SIG, BinaryMetadataValue::from_bytes(&sig.to_bytes()));
    md.insert_bin(H_NONCE, BinaryMetadataValue::from_bytes(&nonce));
    md.insert(H_TS, AsciiMetadataValue::try_from(ts.to_string()).expect("digits are ASCII"));
}

/// What a signed request carries, read from its metadata.
#[derive(Debug, Clone)]
pub struct Proof {
    pub cert: Cert,
    pub signature: Vec<u8>,
    pub nonce: Vec<u8>,
    pub ts: i64,
}

pub fn read_proof(md: &MetadataMap) -> Result<Proof> {
    let bin = |k: &str| -> Result<Vec<u8>> {
        Ok(md
            .get_bin(k)
            .with_context(|| format!("the request has no {k}"))?
            .to_bytes()
            .with_context(|| format!("{k} is not base64"))?
            .to_vec())
    };
    let cert = Cert::decode(bin(H_CERT)?.as_slice()).context("the request's certificate does not decode")?;
    let ts = md
        .get(H_TS)
        .context("the request has no timestamp")?
        .to_str()
        .ok()
        .and_then(|s| s.parse().ok())
        .context("the request's timestamp is not a number")?;
    let nonce = bin(H_NONCE)?;
    if nonce.len() != 16 {
        bail!("the request's nonce is not 16 bytes");
    }
    Ok(Proof { cert, signature: bin(H_SIG)?, nonce, ts })
}

/// Checks the device's signature over a request; the certificate, clock and nonce are
/// the caller's to check.
pub fn verify_request(device_key: &[u8], proof: &Proof, method: &str, body: &[u8]) -> Result<()> {
    verifying_key(device_key)?
        .verify(&request_message(method, proof.ts, &proof.nonce, body), &signature(&proof.signature)?)
        .context("the request's signature does not verify")
}

#[cfg(test)]
mod tests {
    use super::*;
    use nitya_proto::v1::WhoAmIRequest;

    fn body(device: &SigningKey) -> CertBody {
        CertBody {
            serial: 7,
            account: "ank".into(),
            kind: "node".into(),
            device_key: device.verifying_key().to_bytes().to_vec(),
            node_id: "n1".into(),
            hostname: "rogue".into(),
            not_before: 100,
            not_after: 200,
        }
    }

    #[test]
    fn a_certificate_verifies_only_under_its_ca_and_unchanged() {
        let (ca, other, dev) = (generate(), generate(), generate());
        let cert = issue(&ca, &body(&dev));
        assert_eq!(verify_cert(&ca.verifying_key(), &cert).unwrap(), body(&dev));
        assert!(verify_cert(&other.verifying_key(), &cert).is_err());
        let mut forged = body(&dev);
        forged.account = "someone-else".into();
        let tampered = Cert { body: forged.encode_to_vec(), signature: cert.signature.clone() };
        assert!(verify_cert(&ca.verifying_key(), &tampered).is_err());
    }

    #[test]
    fn the_enrollment_proof_binds_the_code_and_the_key() {
        let (dev, other) = (generate(), generate());
        let key = dev.verifying_key().to_bytes();
        let proof = enroll_proof(&dev, "code-1");
        verify_enroll_proof(&key, "code-1", &proof).unwrap();
        assert!(verify_enroll_proof(&key, "code-2", &proof).is_err());
        assert!(verify_enroll_proof(&other.verifying_key().to_bytes(), "code-1", &proof).is_err());
    }

    #[test]
    fn a_signed_request_verifies_for_its_method_and_body_only() {
        let (ca, dev) = (generate(), generate());
        let cert = issue(&ca, &body(&dev));
        let mut req = tonic::Request::new(WhoAmIRequest {});
        sign_request(&mut req, &dev, &cert, "nitya.v1.Devices/WhoAmI", 150);
        let p = read_proof(req.metadata()).unwrap();
        assert_eq!((p.ts, p.cert.clone()), (150, cert));
        let key = dev.verifying_key().to_bytes();
        verify_request(&key, &p, "nitya.v1.Devices/WhoAmI", &[]).unwrap();
        assert!(verify_request(&key, &p, "nitya.v1.Devices/Renew", &[]).is_err());
        assert!(verify_request(&key, &p, "nitya.v1.Devices/WhoAmI", b"x").is_err());
        assert!(verify_request(&key, &Proof { ts: 151, ..p.clone() }, "nitya.v1.Devices/WhoAmI", &[]).is_err());
        assert!(verify_request(&generate().verifying_key().to_bytes(), &p, "nitya.v1.Devices/WhoAmI", &[]).is_err());
    }

    #[test]
    fn a_key_file_is_private_and_stable() {
        use std::os::unix::fs::PermissionsExt;
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("device.key");
        let a = load_or_generate(&p).unwrap();
        assert_eq!(std::fs::metadata(&p).unwrap().permissions().mode() & 0o777, 0o600);
        assert_eq!(load_or_generate(&p).unwrap().to_bytes(), a.to_bytes());
    }
}
