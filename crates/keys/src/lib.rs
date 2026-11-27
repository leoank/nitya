//! An account's key hierarchy (ADR-0005, ADR-0010, ADR-0013), as EXP-20261123-bhfzeh
//! built it, shared by the node, which holds the user key while it works, and the
//! server, which stores the [`KeySet`] and holds only its sync-delegate key.
//!
//! - The user key UK is wrapped (age X25519) to each approved device's wrap key and to
//!   the recovery key, whose own secret is sealed under Argon2id of the passphrase.
//! - Each data class has DEK versions under UK; an append key pair lets the server seal
//!   items of classes it has no delegate for; consented classes have their DEKs wrapped
//!   to the server's delegate key.
//! - Everything under UK binds the account and the UK version in its associated data;
//!   items bind the account, class, item and key version. Tags under UK over each
//!   device's wrap key and the recovery key let a device see that the server swapped in
//!   a key of its own.

use std::path::Path;

use age::secrecy::ExposeSecret;
pub use age::x25519::Identity;
use age::x25519::Recipient;
use anyhow::{Context, Result, anyhow, bail};
use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
pub use nitya_proto::v1::KeySet;
use nitya_proto::v1::{AppendKey, ClassKey, DeviceWrap, SealedItem};
use rand::RngCore;
use rand::rngs::OsRng;
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

/// The data classes of ADR-0005, each with its own DEK.
pub const CLASSES: [&str; 5] = ["mail", "calendar", "tasks", "tokens", "files"];

const AAD_TAG: &[u8] = b"nitya-keys-v1\0";

pub type Key = Zeroizing<[u8; 32]>;

fn random_key() -> Key {
    let mut k = Zeroizing::new([0u8; 32]);
    OsRng.fill_bytes(k.as_mut());
    k
}

fn key_from(v: &[u8]) -> Result<Key> {
    let a: [u8; 32] = v.try_into().map_err(|_| anyhow!("a key is 32 bytes"))?;
    Ok(Zeroizing::new(a))
}

/// Associated data: length-prefixed parts after a tag and the account.
fn aad(account: &str, parts: &[&[u8]]) -> Vec<u8> {
    let mut m = AAD_TAG.to_vec();
    for p in [account.as_bytes()].iter().chain(parts) {
        m.extend_from_slice(&(p.len() as u64).to_be_bytes());
        m.extend_from_slice(p);
    }
    m
}

/// XChaCha20-Poly1305 with a random nonce, which goes first.
fn seal(key: &[u8; 32], plain: &[u8], aad: &[u8]) -> Vec<u8> {
    let mut nonce = [0u8; 24];
    OsRng.fill_bytes(&mut nonce);
    let ct = XChaCha20Poly1305::new(key.into())
        .encrypt(XNonce::from_slice(&nonce), Payload { msg: plain, aad })
        .expect("XChaCha20-Poly1305 encrypts any length we use");
    [nonce.as_slice(), &ct].concat()
}

fn open(key: &[u8; 32], blob: &[u8], aad: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
    if blob.len() < 24 {
        bail!("sealed data too short");
    }
    XChaCha20Poly1305::new(key.into())
        .decrypt(XNonce::from_slice(&blob[..24]), Payload { msg: &blob[24..], aad })
        .map(Zeroizing::new)
        .map_err(|_| anyhow!("does not open: wrong key, or changed"))
}

pub fn recipient(s: &str) -> Result<Recipient> {
    s.parse().map_err(|e| anyhow!("not an age X25519 recipient: {e}"))
}

fn wrap_to(r: &Recipient, secret: &[u8]) -> Result<Vec<u8>> {
    age::encrypt(r, secret).context("wrapping with age")
}

fn unwrap_with(id: &Identity, blob: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
    age::decrypt(id, blob).map(Zeroizing::new).map_err(|_| anyhow!("does not unwrap with this key"))
}

fn identity_from(secret: &[u8]) -> Result<Identity> {
    std::str::from_utf8(secret).ok().and_then(|s| s.parse().ok()).context("not an age identity")
}

fn identity_bytes(id: &Identity) -> Zeroizing<Vec<u8>> {
    Zeroizing::new(id.to_string().expose_secret().as_bytes().to_vec())
}

/// An age X25519 identity kept in `path`, readable only by this user; made when the file
/// does not exist. A device's wrap key, or the server's delegate key.
pub fn load_or_generate(path: &Path) -> Result<Identity> {
    match std::fs::read(path) {
        Ok(b) => identity_from(&Zeroizing::new(b)).with_context(|| format!("{}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let id = Identity::generate();
            nitya_device::write_private(path, &identity_bytes(&id))?;
            Ok(id)
        }
        Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
    }
}

/// What a person compares between two devices before approving one: 80 bits of the
/// wrap key's hash, in groups of four.
pub fn fingerprint(wrap_key: &str) -> String {
    let h = nitya_device::hex(&Sha256::digest(wrap_key.as_bytes())[..10]);
    h.as_bytes().chunks(4).map(|c| std::str::from_utf8(c).unwrap()).collect::<Vec<_>>().join("-")
}

/// Argon2id of the recovery passphrase: 64 MiB, 3 passes, one lane.
fn recovery_kek(passphrase: &str, salt: &[u8]) -> Key {
    let mut out = Zeroizing::new([0u8; 32]);
    Argon2::new(Algorithm::Argon2id, Version::V0x13, Params::new(64 * 1024, 3, 1, Some(32)).expect("valid parameters"))
        .hash_password_into(passphrase.as_bytes(), salt, out.as_mut())
        .expect("a 16-byte salt and a 32-byte output are valid");
    out
}

fn latest(list: &[ClassKey], class: &str) -> Option<u32> {
    list.iter().filter(|k| k.class == class).map(|k| k.version).max()
}

fn item_aad(account: &str, class: &str, item: &str, appended: bool, version: u32) -> Vec<u8> {
    let how: &[u8] = if appended { b"append" } else { b"dek" };
    aad(account, &[b"item", class.as_bytes(), item.as_bytes(), how, &version.to_be_bytes()])
}

fn seal_under(account: &str, class: &str, item: &str, version: u32, dek: &[u8; 32], plain: &[u8]) -> SealedItem {
    SealedItem { version, item_key: vec![], ciphertext: seal(dek, plain, &item_aad(account, class, item, false, version)) }
}

/// Seals an item to the account's latest append key: the server can, the owner's devices
/// can open it, the server cannot.
fn seal_appended(account: &str, keys: &KeySet, class: &str, item: &str, plain: &[u8]) -> Result<SealedItem> {
    let a = keys.append_keys.iter().max_by_key(|a| a.version).context("the account has no append key")?;
    let k = random_key();
    Ok(SealedItem {
        version: a.version,
        item_key: wrap_to(&recipient(&a.public)?, k.as_ref())?,
        ciphertext: seal(&k, plain, &item_aad(account, class, item, true, a.version)),
    })
}

/// A device's session: it holds UK in memory, and changes the key set it was unlocked
/// from, which it then writes back whole.
pub struct Unlocked {
    account: String,
    uk: Key,
    pub keys: KeySet,
}

/// The first device makes the account's keys, with the recovery passphrase the owner
/// chose; the device is approved by making them.
pub fn create(account: &str, device_key: &[u8], wrap_key: &str, passphrase: &str) -> Result<Unlocked> {
    if passphrase.is_empty() {
        bail!("a recovery passphrase is required");
    }
    let mut u = Unlocked { account: account.into(), uk: random_key(), keys: KeySet { uk_version: 1, ..Default::default() } };
    let recovery = Identity::generate();
    let mut salt = [0u8; 16];
    OsRng.fill_bytes(&mut salt);
    u.keys.recovery_salt = salt.to_vec();
    u.keys.recovery_secret = seal(&recovery_kek(passphrase, &salt), &identity_bytes(&recovery), &aad(account, &[b"recovery"]));
    u.keys.recovery_public = recovery.to_public().to_string();
    u.wrap_recovery()?;
    u.approve(device_key, wrap_key)?;
    for c in CLASSES {
        u.new_dek(c, 1);
    }
    u.new_append_key(1);
    Ok(u)
}

/// Unlocks the account's keys with this device's wrap key.
pub fn unlock(account: &str, keys: KeySet, device_key: &[u8], wrap: &Identity) -> Result<Unlocked> {
    let w = keys
        .devices
        .iter()
        .find(|d| d.device_key == device_key)
        .context("this device holds no wrap of the account's keys: approve it from an unlocked device, or recover")?;
    let uk = key_from(&unwrap_with(wrap, &w.wrap)?)?;
    let u = Unlocked { account: account.into(), uk, keys };
    u.check()?;
    Ok(u)
}

/// Unlocks the account's keys with the recovery passphrase, as when every device is lost.
pub fn recover(account: &str, keys: KeySet, passphrase: &str) -> Result<Unlocked> {
    let secret = open(&recovery_kek(passphrase, &keys.recovery_salt), &keys.recovery_secret, &aad(account, &[b"recovery"]))
        .context("wrong recovery passphrase")?;
    let recovery = identity_from(&secret)?;
    if recovery.to_public().to_string() != keys.recovery_public {
        bail!("the stored recovery key is not the passphrase's: the key set was changed");
    }
    let uk = key_from(&unwrap_with(&recovery, &keys.recovery_wrap)?)?;
    let u = Unlocked { account: account.into(), uk, keys };
    u.check()?;
    Ok(u)
}

impl Unlocked {
    pub fn account(&self) -> &str {
        &self.account
    }

    /// Associated data of something under the current UK.
    fn uk_aad(&self, parts: &[&[u8]]) -> Vec<u8> {
        let v = self.keys.uk_version.to_be_bytes();
        let mut all: Vec<&[u8]> = vec![b"uk", &v];
        all.extend_from_slice(parts);
        aad(&self.account, &all)
    }

    fn tag(&self, parts: &[&[u8]]) -> Vec<u8> {
        seal(&self.uk, &[], &self.uk_aad(parts))
    }

    /// Every tag in the key set opens under UK: no device or recovery key was swapped in.
    fn check(&self) -> Result<()> {
        let k = &self.keys;
        open(&self.uk, &k.recovery_tag, &self.uk_aad(&[b"recovery", k.recovery_public.as_bytes()]))
            .context("the recovery key is not one a device approved: the key set was changed")?;
        for d in &k.devices {
            open(&self.uk, &d.tag, &self.uk_aad(&[b"device", &d.device_key, d.wrap_key.as_bytes()]))
                .with_context(|| format!("the wrap key {} was not approved by a device: the key set was changed", fingerprint(&d.wrap_key)))?;
        }
        Ok(())
    }

    fn wrap_recovery(&mut self) -> Result<()> {
        self.keys.recovery_wrap = wrap_to(&recipient(&self.keys.recovery_public)?, self.uk.as_ref())?;
        self.keys.recovery_tag = self.tag(&[b"recovery", self.keys.recovery_public.as_bytes()]);
        Ok(())
    }

    /// Wraps UK to a device: the owner's approval, after comparing its fingerprint.
    pub fn approve(&mut self, device_key: &[u8], wrap_key: &str) -> Result<()> {
        let wrap = wrap_to(&recipient(wrap_key)?, self.uk.as_ref())?;
        let tag = self.tag(&[b"device", device_key, wrap_key.as_bytes()]);
        self.keys.devices.retain(|d| d.device_key != device_key);
        self.keys.devices.push(DeviceWrap { device_key: device_key.to_vec(), wrap_key: wrap_key.into(), wrap, tag });
        Ok(())
    }

    fn dek_aad(&self, class: &str, version: u32) -> Vec<u8> {
        self.uk_aad(&[b"dek", class.as_bytes(), &version.to_be_bytes()])
    }

    fn new_dek(&mut self, class: &str, version: u32) {
        let dek = random_key();
        let wrap = seal(&self.uk, dek.as_ref(), &self.dek_aad(class, version));
        self.keys.deks.push(ClassKey { class: class.into(), version, wrap });
    }

    pub fn dek(&self, class: &str, version: u32) -> Result<Key> {
        let k = self
            .keys
            .deks
            .iter()
            .find(|k| k.class == class && k.version == version)
            .with_context(|| format!("no {class} key version {version}"))?;
        key_from(&open(&self.uk, &k.wrap, &self.dek_aad(class, version))?)
    }

    /// The DEK version that writes of `class` use.
    pub fn current(&self, class: &str) -> Result<u32> {
        latest(&self.keys.deks, class).with_context(|| format!("no {class} key"))
    }

    fn append_aad(&self, version: u32) -> Vec<u8> {
        self.uk_aad(&[b"append", &version.to_be_bytes()])
    }

    fn new_append_key(&mut self, version: u32) {
        let id = Identity::generate();
        let private = seal(&self.uk, &identity_bytes(&id), &self.append_aad(version));
        self.keys.append_keys.push(AppendKey { version, public: id.to_public().to_string(), private });
    }

    fn append_identity(&self, version: u32) -> Result<Identity> {
        let a = self.keys.append_keys.iter().find(|a| a.version == version).with_context(|| format!("no append key version {version}"))?;
        identity_from(&open(&self.uk, &a.private, &self.append_aad(version))?)
    }

    /// The owner's consent: the server may use `class`'s current DEK (to the delegate key
    /// the server gives).
    pub fn consent(&mut self, class: &str, delegate: &str) -> Result<()> {
        let v = self.current(class)?;
        let wrap = wrap_to(&recipient(delegate)?, self.dek(class, v)?.as_ref())?;
        self.keys.delegated.retain(|k| !(k.class == class && k.version == v));
        self.keys.delegated.push(ClassKey { class: class.into(), version: v, wrap });
        Ok(())
    }

    /// Withdraws consent for `class`: the server keeps no wrap of any of its versions.
    pub fn withdraw(&mut self, class: &str) {
        self.keys.delegated.retain(|k| k.class != class);
    }

    pub fn delegated_classes(&self) -> Vec<String> {
        let mut v: Vec<String> = self.keys.delegated.iter().map(|k| k.class.clone()).collect();
        v.sort();
        v.dedup();
        v
    }

    /// Rotation after a revocation (ADR-0010): a new UK, wrapped to the devices still
    /// approved and not `revoked` and to the recovery key; every DEK and append key
    /// re-wrapped under it; a new DEK version per class and a new append key for what is
    /// written from now on; delegated classes get their new version wrapped to `delegate`.
    pub fn rotate(&mut self, revoked: impl Fn(&[u8]) -> bool, delegate: &str) -> Result<()> {
        self.check()?;
        let deks = self.keys.deks.iter().map(|k| Ok((k.class.clone(), k.version, self.dek(&k.class, k.version)?))).collect::<Result<Vec<_>>>()?;
        let appends = self.keys.append_keys.iter().map(|a| Ok((a.version, a.public.clone(), self.append_identity(a.version)?))).collect::<Result<Vec<_>>>()?;
        let keep: Vec<(Vec<u8>, String)> =
            self.keys.devices.iter().filter(|d| !revoked(&d.device_key)).map(|d| (d.device_key.clone(), d.wrap_key.clone())).collect();
        let delegated = self.delegated_classes();

        self.uk = random_key();
        self.keys.uk_version += 1;
        self.wrap_recovery()?;
        self.keys.devices.clear();
        for (dk, wk) in &keep {
            self.approve(dk, wk)?;
        }
        self.keys.deks.clear();
        for (c, v, dek) in &deks {
            let wrap = seal(&self.uk, dek.as_ref(), &self.dek_aad(c, *v));
            self.keys.deks.push(ClassKey { class: c.clone(), version: *v, wrap });
        }
        self.keys.append_keys.clear();
        for (v, public, id) in &appends {
            let private = seal(&self.uk, &identity_bytes(id), &self.append_aad(*v));
            self.keys.append_keys.push(AppendKey { version: *v, public: public.clone(), private });
        }
        for c in CLASSES {
            let next = latest(&self.keys.deks, c).unwrap_or(0) + 1;
            self.new_dek(c, next);
        }
        let next = self.keys.append_keys.iter().map(|a| a.version).max().unwrap_or(0) + 1;
        self.new_append_key(next);
        for c in &delegated {
            self.consent(c, delegate)?;
        }
        Ok(())
    }

    /// Seals an item of `class` under its current DEK.
    pub fn seal_item(&self, class: &str, item: &str, plain: &[u8]) -> Result<SealedItem> {
        let v = self.current(class)?;
        Ok(seal_under(&self.account, class, item, v, &*self.dek(class, v)?, plain))
    }

    /// Opens an item, whether sealed under a DEK or to the append key.
    pub fn open_item(&self, class: &str, item: &str, s: &SealedItem) -> Result<Zeroizing<Vec<u8>>> {
        let appended = !s.item_key.is_empty();
        let key = if appended {
            key_from(&unwrap_with(&self.append_identity(s.version)?, &s.item_key)?)?
        } else {
            self.dek(class, s.version)?
        };
        open(&key, &s.ciphertext, &item_aad(&self.account, class, item, appended, s.version))
    }
}

/// The server's sync-delegate key (ADR-0005): it opens only the DEKs the owner consented
/// to, and seals the other classes to the append key.
pub struct Delegate {
    id: Identity,
}

impl Delegate {
    pub fn load_or_generate(path: &Path) -> Result<Delegate> {
        Ok(Delegate { id: load_or_generate(path)? })
    }

    /// A delegate key provided from outside, as `age-keygen` writes it (comment lines and
    /// one `AGE-SECRET-KEY-` line; an agenix secret passed as a systemd credential); never
    /// made here.
    pub fn load(path: &Path) -> Result<Delegate> {
        let b = Zeroizing::new(std::fs::read(path).with_context(|| format!("reading the delegate key {}", path.display()))?);
        let line = b
            .split(|c| *c == b'\n')
            .map(|l| l.trim_ascii())
            .find(|l| l.starts_with(b"AGE-SECRET-KEY-"))
            .with_context(|| format!("{} holds no age identity", path.display()))?;
        Ok(Delegate { id: identity_from(line)? })
    }

    /// The recipient devices wrap consented DEKs to.
    pub fn public(&self) -> String {
        self.id.to_public().to_string()
    }

    fn dek(&self, keys: &KeySet, class: &str, version: u32) -> Result<Key> {
        let k = keys
            .delegated
            .iter()
            .find(|k| k.class == class && k.version == version)
            .with_context(|| format!("no consent for {class} version {version}"))?;
        key_from(&unwrap_with(&self.id, &k.wrap)?)
    }

    /// Seals an item of `class` that arrived while no device is online: under the current
    /// DEK if the owner consented to it, else to the append key.
    pub fn seal_item(&self, account: &str, keys: &KeySet, class: &str, item: &str, plain: &[u8]) -> Result<SealedItem> {
        let v = latest(&keys.deks, class).with_context(|| format!("the account has no {class} key"))?;
        match self.dek(keys, class, v) {
            Ok(dek) => Ok(seal_under(account, class, item, v, &dek, plain)),
            Err(_) => seal_appended(account, keys, class, item, plain),
        }
    }

    /// Opens an item of a consented class; items sealed to the append key only a device
    /// opens.
    pub fn open_item(&self, account: &str, keys: &KeySet, class: &str, item: &str, s: &SealedItem) -> Result<Zeroizing<Vec<u8>>> {
        if !s.item_key.is_empty() {
            bail!("sealed to the account's append key: only a device opens it");
        }
        let dek = self.dek(keys, class, s.version)?;
        open(&dek, &s.ciphertext, &item_aad(account, class, item, false, s.version))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PASS: &str = "correct horse battery staple";

    struct Dev {
        key: Vec<u8>,
        wrap: Identity,
    }

    impl Dev {
        fn new(n: u8) -> Dev {
            Dev { key: vec![n; 32], wrap: Identity::generate() }
        }
        fn wrap_key(&self) -> String {
            self.wrap.to_public().to_string()
        }
    }

    fn account() -> (Dev, KeySet) {
        let a = Dev::new(1);
        let u = create("ank", &a.key, &a.wrap_key(), PASS).unwrap();
        (a, u.keys)
    }

    #[test]
    fn a_device_reads_what_it_wrote_and_a_new_device_only_once_approved() {
        let (a, keys) = account();
        let ua = unlock("ank", keys.clone(), &a.key, &a.wrap).unwrap();
        let s = ua.seal_item("mail", "m1", b"hello").unwrap();
        assert_eq!(&ua.open_item("mail", "m1", &s).unwrap()[..], b"hello");
        // Bound to its account, class and item.
        assert!(ua.open_item("mail", "m2", &s).is_err());
        assert!(ua.open_item("calendar", "m1", &s).is_err());
        let b = Dev::new(2);
        assert!(unlock("ank", keys.clone(), &b.key, &b.wrap).is_err());
        let mut ua = ua;
        ua.approve(&b.key, &b.wrap_key()).unwrap();
        let ub = unlock("ank", ua.keys.clone(), &b.key, &b.wrap).unwrap();
        assert_eq!(&ub.open_item("mail", "m1", &s).unwrap()[..], b"hello");
        // Another device's wrap key does not open this device's wrap.
        assert!(unlock("ank", ua.keys.clone(), &a.key, &b.wrap).is_err());
    }

    #[test]
    fn the_recovery_passphrase_alone_unlocks_and_a_wrong_one_does_not() {
        let (a, keys) = account();
        let s = unlock("ank", keys.clone(), &a.key, &a.wrap).unwrap().seal_item("tasks", "t1", b"x").unwrap();
        assert!(recover("ank", keys.clone(), "wrong").is_err());
        let r = recover("ank", keys.clone(), PASS).unwrap();
        assert_eq!(&r.open_item("tasks", "t1", &s).unwrap()[..], b"x");
        assert!(create("ank", &a.key, &a.wrap_key(), "").is_err());
    }

    #[test]
    fn a_key_the_server_swaps_in_is_caught_at_unlock() {
        let (a, keys) = account();
        let ua = unlock("ank", keys.clone(), &a.key, &a.wrap).unwrap();
        // The server adds its own device with a wrap of a key it made up: no tag opens.
        let admin = Dev::new(9);
        let mut forged = keys.clone();
        forged.devices.push(DeviceWrap { device_key: admin.key.clone(), wrap_key: admin.wrap_key(), wrap: vec![], tag: ua.tag(&[b"x"]) });
        assert!(unlock("ank", forged, &a.key, &a.wrap).is_err());
        // Or swaps the recovery key for its own, so the next rotation wraps UK to it.
        let mut forged = keys.clone();
        forged.recovery_public = Identity::generate().to_public().to_string();
        assert!(unlock("ank", forged, &a.key, &a.wrap).is_err());
    }

    #[test]
    fn the_delegate_works_offline_on_consented_classes_only() {
        let (a, keys) = account();
        let d = Delegate { id: Identity::generate() };
        let mut ua = unlock("ank", keys, &a.key, &a.wrap).unwrap();
        ua.consent("calendar", &d.public()).unwrap();
        let keys = ua.keys.clone();
        // While every device is offline: calendar under its DEK, which the delegate reads.
        let cal = d.seal_item("ank", &keys, "calendar", "e1", b"standup").unwrap();
        assert!(cal.item_key.is_empty());
        assert_eq!(&d.open_item("ank", &keys, "calendar", "e1", &cal).unwrap()[..], b"standup");
        // Mail is sealed to the append key: stored, unreadable to the server.
        let mail = d.seal_item("ank", &keys, "mail", "m1", b"secret").unwrap();
        assert!(!mail.item_key.is_empty());
        assert!(d.open_item("ank", &keys, "mail", "m1", &mail).is_err());
        let own = ua.seal_item("mail", "m2", b"mine").unwrap();
        assert!(d.open_item("ank", &keys, "mail", "m2", &own).is_err());
        // A device opens both.
        assert_eq!(&ua.open_item("mail", "m1", &mail).unwrap()[..], b"secret");
        assert_eq!(&ua.open_item("calendar", "e1", &cal).unwrap()[..], b"standup");
        // Withdrawn consent: the delegate seals to the append key and reads nothing.
        ua.withdraw("calendar");
        let after = d.seal_item("ank", &ua.keys, "calendar", "e2", b"x").unwrap();
        assert!(!after.item_key.is_empty());
        assert!(d.open_item("ank", &ua.keys, "calendar", "e1", &cal).is_err());
    }

    #[test]
    fn after_rotation_a_revoked_device_that_kept_everything_reads_nothing_new() {
        let (a, keys) = account();
        let b = Dev::new(2);
        let d = Delegate { id: Identity::generate() };
        let mut ua = unlock("ank", keys, &a.key, &a.wrap).unwrap();
        ua.approve(&b.key, &b.wrap_key()).unwrap();
        ua.consent("calendar", &d.public()).unwrap();
        let old = ua.seal_item("mail", "m0", b"before").unwrap();
        // What b keeps when it is revoked: its wrap key and the whole key set.
        let kept = unlock("ank", ua.keys.clone(), &b.key, &b.wrap).unwrap();

        ua.rotate(|k| k == b.key.as_slice(), &d.public()).unwrap();
        let keys = ua.keys.clone();
        assert_eq!(keys.uk_version, 2);
        assert!(unlock("ank", keys.clone(), &b.key, &b.wrap).is_err());
        let new = ua.seal_item("mail", "m1", b"after").unwrap();
        let delegated = d.seal_item("ank", &keys, "calendar", "e1", b"after").unwrap();
        let appended = d.seal_item("ank", &keys, "mail", "m2", b"after").unwrap();
        assert_eq!((new.version, delegated.version), (2, 2));
        for (class, item, s) in [("mail", "m1", &new), ("calendar", "e1", &delegated), ("mail", "m2", &appended)] {
            assert!(kept.open_item(class, item, s).is_err(), "{item}");
        }
        // Nothing is lost: the remaining device, the delegate and recovery read old and new.
        let ua = unlock("ank", keys.clone(), &a.key, &a.wrap).unwrap();
        assert_eq!(&ua.open_item("mail", "m0", &old).unwrap()[..], b"before");
        assert_eq!(&ua.open_item("mail", "m2", &appended).unwrap()[..], b"after");
        assert_eq!(&d.open_item("ank", &keys, "calendar", "e1", &delegated).unwrap()[..], b"after");
        let r = recover("ank", keys, PASS).unwrap();
        assert_eq!(&r.open_item("mail", "m1", &new).unwrap()[..], b"after");
    }

    #[test]
    fn a_provided_delegate_key_is_read_and_never_made() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("delegate.key");
        assert!(Delegate::load(&p).is_err());
        let id = Identity::generate();
        std::fs::write(&p, format!("# created: 2026-11-27\n# public key: {}\n{}\n", id.to_public(), id.to_string().expose_secret())).unwrap();
        assert_eq!(Delegate::load(&p).unwrap().public(), id.to_public().to_string());
    }

    #[test]
    fn a_wrap_key_file_is_private_and_stable() {
        use std::os::unix::fs::PermissionsExt;
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("wrap.key");
        let a = load_or_generate(&p).unwrap();
        assert_eq!(std::fs::metadata(&p).unwrap().permissions().mode() & 0o777, 0o600);
        assert_eq!(load_or_generate(&p).unwrap().to_public().to_string(), a.to_public().to_string());
        assert_eq!(fingerprint("age1x").len(), 24);
    }
}
