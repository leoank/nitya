//! Spike for EXP-20261123-bhfzeh: does ADR-0005's key hierarchy work as designed?
//!
//! What the server stores for an account ([`Account`]) and what each party can do with it:
//!
//! - a user key UK per account version, wrapped (age X25519) to each device and, with
//!   Argon2id, to a recovery passphrase;
//! - a data key DEK per class and version, wrapped by UK (XChaCha20-Poly1305);
//! - for classes the owner consented to, the DEK also wrapped to the server's delegate key
//!   (age), so the server can work while every device is offline;
//! - an append key pair: its public half is on the server, its private half wrapped by UK,
//!   so the server can seal new items of a class it has no delegate for and not read them;
//! - items under XChaCha20-Poly1305 with associated data binding account, class, item id
//!   and key version.
//!
//! Every delegate unwrap and every decryption the server makes goes into an [`AuditLog`]:
//! hash-chained, each entry signed by the server (Ed25519), checked by devices against
//! checkpoints they keep (ADR-0007).
//!
//! Revocation has two strategies, to test what ADR-0005 says ("rotate UK; only DEKs are
//! re-wrapped") against rotating the DEKs as well ([`Revocation`]).

use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

use age::x25519::{Identity, Recipient};
use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use rand::RngCore;
use rand::rngs::OsRng;
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

pub type Key = Zeroizing<[u8; 32]>;

pub fn random_key() -> Key {
    let mut k = Zeroizing::new([0u8; 32]);
    OsRng.fill_bytes(k.as_mut());
    k
}

#[derive(Debug, PartialEq)]
pub enum Error {
    /// AEAD or age refused: wrong key, tampered data or associated data
    Crypto,
    /// the party holds no wrap or key for this
    NoAccess(String),
}

// ------------------------------------------------------------------ primitives

/// XChaCha20-Poly1305 with a random nonce; output is nonce || ciphertext.
pub fn seal(key: &[u8; 32], plain: &[u8], aad: &[u8]) -> Vec<u8> {
    let mut nonce = [0u8; 24];
    OsRng.fill_bytes(&mut nonce);
    let ct = XChaCha20Poly1305::new(key.into())
        .encrypt(XNonce::from_slice(&nonce), Payload { msg: plain, aad })
        .expect("encrypt");
    [nonce.as_slice(), &ct].concat()
}

pub fn open(key: &[u8; 32], blob: &[u8], aad: &[u8]) -> Result<Zeroizing<Vec<u8>>, Error> {
    if blob.len() < 24 {
        return Err(Error::Crypto);
    }
    XChaCha20Poly1305::new(key.into())
        .decrypt(XNonce::from_slice(&blob[..24]), Payload { msg: &blob[24..], aad })
        .map(Zeroizing::new)
        .map_err(|_| Error::Crypto)
}

fn key_from(v: &[u8]) -> Result<Key, Error> {
    let a: [u8; 32] = v.try_into().map_err(|_| Error::Crypto)?;
    Ok(Zeroizing::new(a))
}

/// age to one X25519 recipient (ADR-0005: "age or HPKE").
pub fn wrap_to(r: &Recipient, secret: &[u8]) -> Vec<u8> {
    age::encrypt(r, secret).expect("age encrypt")
}

pub fn unwrap_with(id: &Identity, blob: &[u8]) -> Result<Zeroizing<Vec<u8>>, Error> {
    age::decrypt(id, blob).map(Zeroizing::new).map_err(|_| Error::Crypto)
}

/// Argon2id parameters of the recovery wrap: 64 MiB, 3 passes, one lane.
pub fn recovery_kek(pass: &str, salt: &[u8]) -> Key {
    let mut out = Zeroizing::new([0u8; 32]);
    Argon2::new(Algorithm::Argon2id, Version::V0x13, Params::new(64 * 1024, 3, 1, Some(32)).unwrap())
        .hash_password_into(pass.as_bytes(), salt, out.as_mut())
        .expect("argon2");
    out
}

// ------------------------------------------------------------------ the account

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Class {
    Calendar,
    Mail,
    Tasks,
}

pub struct Device {
    pub id: String,
    pub identity: Identity,
}

impl Device {
    pub fn new(id: &str) -> Self {
        Device { id: id.into(), identity: Identity::generate() }
    }
}

#[derive(Clone)]
pub enum Sealing {
    /// under the class's DEK of this version
    Dek(u32),
    /// a random item key sealed to the account's append key of this version
    Append(u32, Vec<u8>),
}

#[derive(Clone)]
pub struct Item {
    pub id: u64,
    pub class: Class,
    pub sealing: Sealing,
    pub blob: Vec<u8>,
}

/// What the server stores for one account. Nothing in it is readable without a key that
/// the server does not hold, except through the delegate wraps.
#[derive(Clone, Default)]
pub struct Account {
    pub name: String,
    pub uk_version: u32,
    pub uk_wraps: BTreeMap<String, Vec<u8>>,
    pub recovery: Option<(Vec<u8>, Vec<u8>)>, // (salt, UK sealed under Argon2id(passphrase))
    pub dek_wraps: BTreeMap<(Class, u32), Vec<u8>>, // DEK under the current UK
    pub current: BTreeMap<Class, u32>,
    pub delegate_wraps: BTreeMap<(Class, u32), Vec<u8>>, // DEK sealed to the server's delegate key
    pub append_version: u32,
    pub append_public: BTreeMap<u32, String>, // the append key's public half (age recipient)
    pub append_private: BTreeMap<u32, Vec<u8>>, // its private half under the current UK
    pub items: Vec<Item>,
    next_item: u64,
}

fn aad(account: &str, what: &str) -> Vec<u8> {
    format!("nitya/v1|{account}|{what}").into_bytes()
}

impl Account {
    fn item_aad(&self, class: Class, id: u64, s: &Sealing) -> Vec<u8> {
        let v = match s {
            Sealing::Dek(v) => format!("dek{v}"),
            Sealing::Append(v, _) => format!("append{v}"),
        };
        aad(&self.name, &format!("item|{class:?}|{id}|{v}"))
    }
    fn dek_aad(&self, class: Class, v: u32) -> Vec<u8> {
        aad(&self.name, &format!("dek|{class:?}|{v}|uk{}", self.uk_version))
    }
    fn append_aad(&self, v: u32) -> Vec<u8> {
        aad(&self.name, &format!("append|{v}|uk{}", self.uk_version))
    }
}

/// An unlocked session of a device: it holds UK in memory.
pub struct Unlocked<'a> {
    pub account: &'a mut Account,
    uk: Key,
}

/// Creates an account from its first device, with a DEK per class and a recovery wrap.
pub fn create_account(name: &str, first: &Device, passphrase: &str) -> Account {
    let mut a = Account { name: name.into(), uk_version: 1, append_version: 1, ..Default::default() };
    let uk = random_key();
    a.uk_wraps.insert(first.id.clone(), wrap_to(&first.identity.to_public(), uk.as_ref()));
    let mut salt = vec![0u8; 16];
    OsRng.fill_bytes(&mut salt);
    let kek = recovery_kek(passphrase, &salt);
    let rec = seal(&kek, uk.as_ref(), &aad(name, "recovery|uk1"));
    a.recovery = Some((salt, rec));
    for c in [Class::Calendar, Class::Mail, Class::Tasks] {
        let dek = random_key();
        let w = seal(&uk, dek.as_ref(), &a.dek_aad(c, 1));
        a.dek_wraps.insert((c, 1), w);
        a.current.insert(c, 1);
    }
    let append = Identity::generate();
    a.append_public.insert(1, append.to_public().to_string());
    let private = seal(&uk, append.to_string().expose_secret_bytes(), &a.append_aad(1));
    a.append_private.insert(1, private);
    a
}

trait ExposeBytes {
    fn expose_secret_bytes(&self) -> &[u8];
}
impl ExposeBytes for age::secrecy::SecretString {
    fn expose_secret_bytes(&self) -> &[u8] {
        use age::secrecy::ExposeSecret;
        self.expose_secret().as_bytes()
    }
}

pub fn unlock<'a>(account: &'a mut Account, device: &Device) -> Result<Unlocked<'a>, Error> {
    let w = account.uk_wraps.get(&device.id).ok_or_else(|| Error::NoAccess(format!("no UK wrap for {}", device.id)))?;
    let uk = key_from(&unwrap_with(&device.identity, w)?)?;
    Ok(Unlocked { account, uk })
}

pub fn recover<'a>(account: &'a mut Account, passphrase: &str) -> Result<Unlocked<'a>, Error> {
    let (salt, rec) = account.recovery.clone().ok_or_else(|| Error::NoAccess("no recovery wrap".into()))?;
    let kek = recovery_kek(passphrase, &salt);
    let uk = key_from(&open(&kek, &rec, &aad(&account.name, &format!("recovery|uk{}", account.uk_version)))?)?;
    Ok(Unlocked { account, uk })
}

/// Revocation strategies: what ADR-0005 says, and rotating the data keys as well.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Revocation {
    /// "deletes its wrap and rotates UK; only DEKs are re-wrapped" (ADR-0005 as written)
    RewrapOnly,
    /// also a new DEK version per class (and a new append key) for everything written
    /// from now on; old items stay under their old version, still readable by the owner
    RotateDeks,
}

impl<'a> Unlocked<'a> {
    pub fn uk(&self) -> &Key {
        &self.uk
    }

    pub fn dek(&self, class: Class, v: u32) -> Result<Key, Error> {
        let w = self.account.dek_wraps.get(&(class, v)).ok_or_else(|| Error::NoAccess(format!("no DEK {class:?} v{v}")))?;
        key_from(&open(&self.uk, w, &self.account.dek_aad(class, v))?)
    }

    pub fn write(&mut self, class: Class, plain: &[u8]) -> u64 {
        let v = self.account.current[&class];
        let dek = self.dek(class, v).expect("current DEK");
        let id = self.account.next_item;
        self.account.next_item += 1;
        let s = Sealing::Dek(v);
        let blob = seal(&dek, plain, &self.account.item_aad(class, id, &s));
        self.account.items.push(Item { id, class, sealing: s, blob });
        id
    }

    pub fn read(&self, id: u64) -> Result<Zeroizing<Vec<u8>>, Error> {
        let it = self.account.items.iter().find(|i| i.id == id).ok_or_else(|| Error::NoAccess("no item".into()))?;
        let a = self.account.item_aad(it.class, it.id, &it.sealing);
        match &it.sealing {
            Sealing::Dek(v) => open(&*self.dek(it.class, *v)?, &it.blob, &a),
            Sealing::Append(v, sealed_key) => {
                let id = self.append_identity(*v)?;
                let k = key_from(&unwrap_with(&id, sealed_key)?)?;
                open(&k, &it.blob, &a)
            }
        }
    }

    fn append_identity(&self, v: u32) -> Result<Identity, Error> {
        let w = self.account.append_private.get(&v).ok_or_else(|| Error::NoAccess("no append key".into()))?;
        let s = open(&self.uk, w, &self.account.append_aad(v))?;
        std::str::from_utf8(&s).map_err(|_| Error::Crypto)?.parse::<Identity>().map_err(|_| Error::Crypto)
    }

    /// Re-encrypts appended items under the class's current DEK (a device does this when
    /// it next comes online).
    pub fn ingest_appended(&mut self) -> usize {
        let ids: Vec<u64> = self.account.items.iter().filter(|i| matches!(i.sealing, Sealing::Append(..))).map(|i| i.id).collect();
        for id in &ids {
            let plain = self.read(*id).expect("appended item");
            let pos = self.account.items.iter().position(|i| i.id == *id).unwrap();
            let class = self.account.items[pos].class;
            let v = self.account.current[&class];
            let s = Sealing::Dek(v);
            let blob = seal(&self.dek(class, v).unwrap(), &plain, &self.account.item_aad(class, *id, &s));
            self.account.items[pos] = Item { id: *id, class, sealing: s, blob };
        }
        ids.len()
    }

    pub fn enroll(&mut self, device: &Device) {
        let w = wrap_to(&device.identity.to_public(), self.uk.as_ref());
        self.account.uk_wraps.insert(device.id.clone(), w);
    }

    /// The owner's consent: the class's current DEK sealed to the server's delegate key.
    pub fn consent_delegate(&mut self, class: Class, server: &Recipient) {
        let v = self.account.current[&class];
        let dek = self.dek(class, v).unwrap();
        self.account.delegate_wraps.insert((class, v), wrap_to(server, dek.as_ref()));
    }

    pub fn withdraw_delegate(&mut self, class: Class) {
        self.account.delegate_wraps.retain(|(c, _), _| *c != class);
    }

    /// Revokes a device from a remaining one: its wrap is deleted and UK rotated; every
    /// DEK and append key is re-wrapped under the new UK; with `RotateDeks` each class also
    /// gets a new DEK (and the delegate wraps of consented classes are renewed) and a new
    /// append key.
    pub fn revoke(&mut self, device_id: &str, how: Revocation, passphrase: &str, server: &Recipient) {
        let old: Vec<((Class, u32), Key)> = self.account.dek_wraps.keys().cloned().collect::<Vec<_>>()
            .into_iter().map(|(c, v)| ((c, v), self.dek(c, v).unwrap())).collect();
        let old_append: Vec<(u32, Identity)> = self.account.append_private.keys().cloned().collect::<Vec<_>>()
            .into_iter().map(|v| (v, self.append_identity(v).unwrap())).collect();
        let devices: Vec<String> = self.account.uk_wraps.keys().filter(|d| *d != device_id).cloned().collect();
        let remaining: Vec<(String, Recipient)> = devices.iter().map(|d| (d.clone(), self.recipient_of(d))).collect();
        // new UK, wrapped to the remaining devices and the recovery passphrase
        let uk = random_key();
        self.account.uk_version += 1;
        self.account.uk_wraps.clear();
        for (d, r) in remaining {
            self.account.uk_wraps.insert(d, wrap_to(&r, uk.as_ref()));
        }
        let mut salt = vec![0u8; 16];
        OsRng.fill_bytes(&mut salt);
        let kek = recovery_kek(passphrase, &salt);
        let rec = seal(&kek, uk.as_ref(), &aad(&self.account.name, &format!("recovery|uk{}", self.account.uk_version)));
        self.account.recovery = Some((salt, rec));
        self.uk = uk;
        self.account.dek_wraps.clear();
        for ((c, v), k) in &old {
            let w = seal(&self.uk, k.as_ref(), &self.account.dek_aad(*c, *v));
            self.account.dek_wraps.insert((*c, *v), w);
        }
        self.account.append_private.clear();
        for (v, id) in &old_append {
            let w = seal(&self.uk, id.to_string().expose_secret_bytes(), &self.account.append_aad(*v));
            self.account.append_private.insert(*v, w);
        }
        if how == Revocation::RotateDeks {
            for c in [Class::Calendar, Class::Mail, Class::Tasks] {
                let v = self.account.current[&c] + 1;
                let dek = random_key();
                let w = seal(&self.uk, dek.as_ref(), &self.account.dek_aad(c, v));
                self.account.dek_wraps.insert((c, v), w);
                self.account.current.insert(c, v);
                if self.account.delegate_wraps.keys().any(|(dc, _)| *dc == c) {
                    self.account.delegate_wraps.insert((c, v), wrap_to(server, dek.as_ref()));
                }
            }
            let v = self.account.append_version + 1;
            let append = Identity::generate();
            self.account.append_public.insert(v, append.to_public().to_string());
            let w = seal(&self.uk, append.to_string().expose_secret_bytes(), &self.account.append_aad(v));
            self.account.append_private.insert(v, w);
            self.account.append_version = v;
        }
    }

    /// The spike keeps device public keys in a registry beside the account (nitya's
    /// enrollment records); here they are recovered from the test's devices.
    fn recipient_of(&self, d: &str) -> Recipient {
        REGISTRY.with(|r| r.borrow().get(d).cloned().expect("device registered"))
    }
}

thread_local! {
    /// Device id to its public key: nitya's enrollment records (ADR-0005's certificates).
    pub static REGISTRY: std::cell::RefCell<BTreeMap<String, Recipient>> = Default::default();
}

pub fn register(d: &Device) {
    REGISTRY.with(|r| r.borrow_mut().insert(d.id.clone(), d.identity.to_public()));
}

// ------------------------------------------------------------------ what a revoked device kept

/// Everything a device could have copied while it was enrolled: its own identity and the
/// account's wraps as they were (the server's data is not secret from an enrolled device).
pub struct Kept {
    pub device: Device,
    pub snapshot: Account,
}

impl Kept {
    /// Tries to read an item of the account *as it is now* with what was kept: the old UK
    /// from the old wrap, then the old DEK wraps and append keys.
    pub fn try_read(&self, now: &Account, id: u64) -> Result<Zeroizing<Vec<u8>>, Error> {
        let mut snap = self.snapshot.clone();
        let old = unlock(&mut snap, &self.device)?;
        let it = now.items.iter().find(|i| i.id == id).ok_or_else(|| Error::NoAccess("no item".into()))?;
        let a = now.item_aad(it.class, it.id, &it.sealing);
        match &it.sealing {
            Sealing::Dek(v) => open(&*old.dek(it.class, *v)?, &it.blob, &a),
            Sealing::Append(v, sk) => {
                let id = old.append_identity(*v)?;
                open(&*key_from(&unwrap_with(&id, sk)?)?, &it.blob, &a)
            }
        }
    }
}

// ------------------------------------------------------------------ the server

/// The server: its delegate key (unlocked at service start), its log signing key, and the
/// audit log. It acts on an account's data only through delegate wraps, and logs each use.
pub struct Server {
    pub delegate: Identity,
    pub signer: SigningKey,
    pub log: AuditLog,
}

impl Server {
    pub fn new() -> Self {
        Server { delegate: Identity::generate(), signer: SigningKey::generate(&mut OsRng), log: AuditLog::default() }
    }

    pub fn recipient(&self) -> Recipient {
        self.delegate.to_public()
    }

    fn delegate_dek(&mut self, a: &Account, class: Class, v: u32) -> Result<Key, Error> {
        let res = a.delegate_wraps.get(&(class, v)).ok_or_else(|| Error::NoAccess(format!("no delegate for {class:?}")));
        let out = res.and_then(|w| key_from(&unwrap_with(&self.delegate, w)?));
        let decision = if out.is_ok() { "allow" } else { "deny" };
        self.log.append(&self.signer, "system:sync-delegate", "delegate.unwrap", &format!("{}/{class:?}/dek{v}", a.name), decision);
        out
    }

    /// Background sync writing a calendar change while no device is online.
    pub fn sync_write(&mut self, a: &mut Account, class: Class, plain: &[u8]) -> Result<u64, Error> {
        let v = a.current[&class];
        let id = a.next_item;
        match self.delegate_dek(a, class, v) {
            Ok(dek) => {
                let s = Sealing::Dek(v);
                let blob = seal(&dek, plain, &a.item_aad(class, id, &s));
                self.log.append(&self.signer, "system:sync-delegate", "item.encrypt", &format!("{}/{class:?}/{id}", a.name), "allow");
                a.items.push(Item { id, class, sealing: s, blob });
            }
            Err(_) => {
                // no delegate: append-only, sealed to the account's append key
                let av = a.append_version;
                let r: Recipient = a.append_public[&av].parse().map_err(|_| Error::Crypto)?;
                let k = random_key();
                let s = Sealing::Append(av, wrap_to(&r, k.as_ref()));
                let blob = seal(&k, plain, &a.item_aad(class, id, &s));
                self.log.append(&self.signer, "system:sync", "item.append-sealed", &format!("{}/{class:?}/{id}", a.name), "allow");
                a.items.push(Item { id, class, sealing: s, blob });
            }
        }
        a.next_item += 1;
        Ok(id)
    }

    /// An agent job reading an item while no device is online.
    pub fn read(&mut self, a: &Account, id: u64) -> Result<Zeroizing<Vec<u8>>, Error> {
        let it = a.items.iter().find(|i| i.id == id).cloned().ok_or_else(|| Error::NoAccess("no item".into()))?;
        let out = match &it.sealing {
            Sealing::Dek(v) => self.delegate_dek(a, it.class, *v).and_then(|k| open(&k, &it.blob, &a.item_aad(it.class, it.id, &it.sealing))),
            Sealing::Append(..) => Err(Error::NoAccess("sealed to the append key".into())),
        };
        let decision = if out.is_ok() { "allow" } else { "deny" };
        self.log.append(&self.signer, "system:agent-job", "item.decrypt", &format!("{}/{:?}/{}", a.name, it.class, it.id), decision);
        out
    }
}

impl Default for Server {
    fn default() -> Self {
        Self::new()
    }
}

// ------------------------------------------------------------------ the audit log (ADR-0007)

#[derive(Clone, Debug)]
pub struct Entry {
    pub seq: u64,
    pub time: u64,
    pub actor: String,
    pub action: String,
    pub resource: String,
    pub decision: String,
    pub prev: [u8; 32],
    pub hash: [u8; 32],
    pub sig: Signature,
}

impl Entry {
    fn body(seq: u64, time: u64, actor: &str, action: &str, resource: &str, decision: &str, prev: &[u8; 32]) -> Vec<u8> {
        // length-prefixed fields, so no two entries share an encoding
        let mut b = Vec::new();
        for f in [seq.to_string().as_str(), &time.to_string(), actor, action, resource, decision] {
            b.extend((f.len() as u32).to_be_bytes());
            b.extend(f.as_bytes());
        }
        b.extend(prev);
        b
    }
}

#[derive(Clone, Default)]
pub struct AuditLog {
    pub entries: Vec<Entry>,
}

/// A device's checkpoint of the chain: the head it last saw.
#[derive(Clone, Copy, Debug)]
pub struct Checkpoint {
    pub seq: u64,
    pub hash: [u8; 32],
}

impl AuditLog {
    pub fn append(&mut self, key: &SigningKey, actor: &str, action: &str, resource: &str, decision: &str) {
        let seq = self.entries.len() as u64;
        let prev = self.entries.last().map(|e| e.hash).unwrap_or([0; 32]);
        let time = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
        let hash: [u8; 32] = Sha256::digest(Entry::body(seq, time, actor, action, resource, decision, &prev)).into();
        let sig = key.sign(&hash);
        self.entries.push(Entry { seq, time, actor: actor.into(), action: action.into(), resource: resource.into(),
                                  decision: decision.into(), prev, hash, sig });
    }

    pub fn checkpoint(&self) -> Checkpoint {
        let e = self.entries.last().expect("an entry");
        Checkpoint { seq: e.seq, hash: e.hash }
    }

    /// Verifies the whole chain and that it still contains every checkpoint a device kept.
    pub fn verify(&self, server: &VerifyingKey, checkpoints: &[Checkpoint]) -> Result<(), String> {
        let mut prev = [0u8; 32];
        for (i, e) in self.entries.iter().enumerate() {
            if e.seq != i as u64 {
                return Err(format!("entry {i}: sequence {} out of order", e.seq));
            }
            if e.prev != prev {
                return Err(format!("entry {i}: does not follow the previous entry"));
            }
            let h: [u8; 32] = Sha256::digest(Entry::body(e.seq, e.time, &e.actor, &e.action, &e.resource, &e.decision, &e.prev)).into();
            if h != e.hash {
                return Err(format!("entry {i}: contents do not match its hash"));
            }
            server.verify(&e.hash, &e.sig).map_err(|_| format!("entry {i}: bad signature"))?;
            prev = e.hash;
        }
        for c in checkpoints {
            match self.entries.get(c.seq as usize) {
                None => return Err(format!("checkpoint {}: the log is shorter", c.seq)),
                Some(e) if e.hash != c.hash => return Err(format!("checkpoint {}: history was rewritten", c.seq)),
                _ => {}
            }
        }
        Ok(())
    }
}
