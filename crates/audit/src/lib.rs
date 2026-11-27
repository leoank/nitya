//! The audit log (ADR-0007, ADR-0015): how the server seals an entry onto an account's
//! chain, and how a device checks the chain from its checkpoint and signs a new one.
//!
//! An entry's hash covers its encoded bytes, which hold the previous entry's hash and the
//! hash of its detail; the detail itself can be redacted without breaking the chain. The
//! server signs each hash. Only a device's checkpoint catches a history the server
//! rewrote and signed again.

use anyhow::{Context, Result, bail};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
pub use nitya_proto::v1::{AuditEntry, Checkpoint, SignedEntry};
use prost::Message;
use sha2::{Digest, Sha256};

const ENTRY_TAG: &[u8] = b"nitya-audit-v1\0";
const CHECKPOINT_TAG: &[u8] = b"nitya-checkpoint-v1\0";

/// The `prev` of a chain's first entry.
pub const GENESIS: [u8; 32] = [0; 32];

fn entry_hash(entry: &[u8]) -> Vec<u8> {
    Sha256::digest(nitya_device::tagged(ENTRY_TAG, &[entry])).to_vec()
}

/// Seals `entry` (its seq and prev already set) with its detail, signed by the server's
/// audit key.
pub fn seal(key: &SigningKey, mut entry: AuditEntry, detail: &[u8]) -> SignedEntry {
    entry.detail_hash = Sha256::digest(detail).to_vec();
    let bytes = entry.encode_to_vec();
    let hash = entry_hash(&bytes);
    let signature = key.sign(&hash).to_bytes().to_vec();
    SignedEntry { entry: bytes, detail: detail.to_vec(), hash, signature }
}

/// The head of a chain: its last entry's seq and hash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Head {
    pub seq: u64,
    pub hash: Vec<u8>,
}

/// Checks `entries` of `chain`, as the server gave them from the device's checkpoint
/// `from` on (or from the start), and returns the new head with the entries decoded.
pub fn verify(audit_key: &VerifyingKey, chain: &str, from: Option<&Head>, entries: &[SignedEntry]) -> Result<(Head, Vec<AuditEntry>)> {
    let mut prev: Option<Head> = None;
    let mut out = Vec::new();
    for (i, s) in entries.iter().enumerate() {
        let hash = entry_hash(&s.entry);
        if hash != s.hash {
            bail!("entry {i}: its contents do not match its hash");
        }
        let sig = Signature::from_slice(&s.signature).context("not a signature")?;
        audit_key.verify(&hash, &sig).with_context(|| format!("entry {i}: not signed by the server's audit key"))?;
        let e = AuditEntry::decode(s.entry.as_slice()).with_context(|| format!("entry {i} does not decode"))?;
        if e.chain != chain {
            bail!("entry {} belongs to another chain", e.seq);
        }
        if !s.detail.is_empty() && Sha256::digest(&s.detail).as_slice() != e.detail_hash.as_slice() {
            bail!("entry {}: its detail was changed", e.seq);
        }
        match &prev {
            Some(p) if e.seq != p.seq + 1 || e.prev != p.hash => bail!("entry {} does not follow entry {}", e.seq, p.seq),
            None => match from {
                Some(c) if e.seq != c.seq || hash != c.hash => {
                    bail!("the history was rewritten: entry {} is not the one this device checkpointed", c.seq)
                }
                None if e.seq != 0 || e.prev != GENESIS => bail!("the chain does not start at its first entry"),
                _ => {}
            },
            _ => {}
        }
        prev = Some(Head { seq: e.seq, hash });
        out.push(e);
    }
    match (prev, from) {
        (Some(h), _) => Ok((h, out)),
        (None, Some(c)) => bail!("the log is shorter than this device's checkpoint at entry {}", c.seq),
        (None, None) => Ok((Head { seq: 0, hash: vec![] }, out)),
    }
}

fn checkpoint_message(chain: &str, seq: u64, hash: &[u8]) -> Vec<u8> {
    nitya_device::tagged(CHECKPOINT_TAG, &[chain.as_bytes(), &seq.to_be_bytes(), hash])
}

pub fn sign_checkpoint(device: &SigningKey, chain: &str, head: &Head, time: i64) -> Checkpoint {
    Checkpoint {
        chain: chain.into(),
        seq: head.seq,
        hash: head.hash.clone(),
        device_key: device.verifying_key().to_bytes().to_vec(),
        time,
        signature: device.sign(&checkpoint_message(chain, head.seq, &head.hash)).to_bytes().to_vec(),
    }
}

pub fn verify_checkpoint(c: &Checkpoint) -> Result<()> {
    let sig = Signature::from_slice(&c.signature).context("not a signature")?;
    nitya_device::verifying_key(&c.device_key)?
        .verify(&checkpoint_message(&c.chain, c.seq, &c.hash), &sig)
        .context("the checkpoint is not signed by its device")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chain(key: &SigningKey, n: u64) -> Vec<SignedEntry> {
        let mut prev = GENESIS.to_vec();
        (0..n)
            .map(|seq| {
                let e = AuditEntry { seq, chain: "ank".into(), action: "keys.read".into(), decision: "allow".into(), prev: prev.clone(), ..Default::default() };
                let s = seal(key, e, format!("host-{seq}").as_bytes());
                prev = s.hash.clone();
                s
            })
            .collect()
    }

    #[test]
    fn a_chain_verifies_from_the_start_and_from_a_checkpoint() {
        let k = nitya_device::generate();
        let c = chain(&k, 5);
        let (head, entries) = verify(&k.verifying_key(), "ank", None, &c).unwrap();
        assert_eq!((head.seq, entries.len()), (4, 5));
        let at2 = Head { seq: 2, hash: c[2].hash.clone() };
        assert_eq!(verify(&k.verifying_key(), "ank", Some(&at2), &c[2..]).unwrap().0, head);
        // Another chain's entries, or another server's key.
        assert!(verify(&k.verifying_key(), "someone", None, &c).is_err());
        assert!(verify(&nitya_device::generate().verifying_key(), "ank", None, &c).is_err());
    }

    #[test]
    fn edits_deletions_and_truncation_are_caught() {
        let k = nitya_device::generate();
        let key = k.verifying_key();
        let c = chain(&k, 5);
        let mut edited = c.clone();
        let mut e = AuditEntry::decode(edited[3].entry.as_slice()).unwrap();
        e.decision = "deny".into();
        edited[3].entry = e.encode_to_vec();
        assert!(verify(&key, "ank", None, &edited).is_err());
        let mut deleted = c.clone();
        deleted.remove(2);
        assert!(verify(&key, "ank", None, &deleted).is_err());
        let at4 = Head { seq: 4, hash: c[4].hash.clone() };
        assert!(verify(&key, "ank", Some(&at4), &[]).is_err());
        let mut changed = c.clone();
        changed[1].detail = b"someone else".to_vec();
        assert!(verify(&key, "ank", None, &changed).is_err());
    }

    #[test]
    fn a_redacted_detail_keeps_the_chain() {
        let k = nitya_device::generate();
        let mut c = chain(&k, 3);
        c[1].detail.clear();
        assert!(verify(&k.verifying_key(), "ank", None, &c).is_ok());
    }

    #[test]
    fn a_history_rewritten_and_signed_again_is_caught_only_by_a_checkpoint() {
        let k = nitya_device::generate();
        let c = chain(&k, 4);
        let at3 = Head { seq: 3, hash: c[3].hash.clone() };
        // The server rewrites entry 1 and re-chains and re-signs everything after it.
        let mut prev = c[0].hash.clone();
        let mut rewritten = vec![c[0].clone()];
        for seq in 1..4 {
            let e = AuditEntry { seq, chain: "ank".into(), action: "keys.read".into(), decision: if seq == 1 { "deny" } else { "allow" }.into(), prev: prev.clone(), ..Default::default() };
            let s = seal(&k, e, format!("host-{seq}").as_bytes());
            prev = s.hash.clone();
            rewritten.push(s);
        }
        assert!(verify(&k.verifying_key(), "ank", None, &rewritten).is_ok());
        let e = verify(&k.verifying_key(), "ank", Some(&at3), &rewritten[3..]).unwrap_err();
        assert!(format!("{e:#}").contains("rewritten"), "{e:#}");
    }

    #[test]
    fn a_checkpoint_is_bound_to_its_device() {
        let d = nitya_device::generate();
        let mut c = sign_checkpoint(&d, "ank", &Head { seq: 3, hash: vec![7; 32] }, 100);
        verify_checkpoint(&c).unwrap();
        c.seq = 4;
        assert!(verify_checkpoint(&c).is_err());
    }
}
