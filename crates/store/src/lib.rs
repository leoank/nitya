//! nitya's store (ADR-0003, ADR-0012): one SQLite database in the server's state
//! directory. Accounts, the device CA's records, and channels and items. Items arrive
//! sealed; the store never sees a key.

use std::path::Path;

use anyhow::{Context, Result, bail};
use rusqlite::{Connection, OptionalExtension, params};

/// The schema, one entry per version; never edit one that has shipped, add the next.
const MIGRATIONS: &[&str] = &[
    // 1: accounts, the CA (ADR-0011), channels and items (ADR-0003).
    "
    CREATE TABLE accounts (
        id INTEGER PRIMARY KEY,
        name TEXT NOT NULL UNIQUE,
        created INTEGER NOT NULL
    );
    CREATE TABLE enroll_codes (
        hash BLOB PRIMARY KEY,
        account_id INTEGER NOT NULL REFERENCES accounts(id),
        kind TEXT NOT NULL,
        expires INTEGER NOT NULL
    );
    CREATE TABLE certs (
        serial INTEGER PRIMARY KEY,
        account_id INTEGER NOT NULL REFERENCES accounts(id),
        device_key BLOB NOT NULL,
        not_after INTEGER NOT NULL,
        cert BLOB NOT NULL
    );
    CREATE INDEX certs_device ON certs(device_key);
    CREATE TABLE revoked (
        device_key BLOB PRIMARY KEY,
        at INTEGER NOT NULL
    );
    CREATE TABLE channels (
        id INTEGER PRIMARY KEY,
        account_id INTEGER NOT NULL REFERENCES accounts(id),
        provider TEXT NOT NULL,
        class TEXT NOT NULL,
        name TEXT NOT NULL,
        cursor BLOB,
        stage TEXT NOT NULL DEFAULT 'new',
        updated INTEGER NOT NULL,
        UNIQUE (account_id, provider, class, name)
    );
    CREATE TABLE items (
        id INTEGER PRIMARY KEY,
        channel_id INTEGER NOT NULL REFERENCES channels(id),
        provider_id TEXT NOT NULL,
        etag TEXT,
        key_version INTEGER NOT NULL,
        sealed BLOB NOT NULL,
        updated INTEGER NOT NULL,
        UNIQUE (channel_id, provider_id)
    );
    ",
];

pub struct Store {
    db: Connection,
}

/// An enrollment code as the store keeps it: for which account and kind, until when.
#[derive(Debug, PartialEq, Eq)]
pub struct Code {
    pub account: String,
    pub kind: String,
    pub expires: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Channel {
    pub id: i64,
    pub account: String,
    pub provider: String,
    pub class: String,
    pub name: String,
    pub cursor: Option<Vec<u8>>,
    pub stage: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub id: i64,
    pub channel_id: i64,
    pub provider_id: String,
    pub etag: Option<String>,
    pub key_version: u32,
    pub sealed: Vec<u8>,
    pub updated: i64,
}

impl Store {
    /// Opens (or creates) the database at `path`, readable only by this user, and brings
    /// its schema up to date.
    pub fn open(path: &Path) -> Result<Store> {
        let db = Connection::open(path).with_context(|| format!("opening {}", path.display()))?;
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
        }
        db.busy_timeout(std::time::Duration::from_secs(5))?;
        db.pragma_update(None, "journal_mode", "WAL")?;
        db.pragma_update(None, "foreign_keys", true)?;
        let mut s = Store { db };
        s.migrate().with_context(|| format!("migrating {}", path.display()))?;
        Ok(s)
    }

    pub fn version(&self) -> Result<usize> {
        Ok(self.db.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))? as usize)
    }

    fn migrate(&mut self) -> Result<()> {
        let have = self.version()?;
        if have > MIGRATIONS.len() {
            bail!("the database is at schema {have}, newer than this binary's {}; not touching it", MIGRATIONS.len());
        }
        for (i, sql) in MIGRATIONS.iter().enumerate().skip(have) {
            let tx = self.db.transaction()?;
            tx.execute_batch(sql).with_context(|| format!("schema {}", i + 1))?;
            tx.pragma_update(None, "user_version", (i + 1) as i64)?;
            tx.commit()?;
        }
        Ok(())
    }

    /// The account's id, made if it does not exist.
    pub fn account(&self, name: &str, now: i64) -> Result<i64> {
        self.db.execute("INSERT OR IGNORE INTO accounts (name, created) VALUES (?1, ?2)", params![name, now])?;
        Ok(self.db.query_row("SELECT id FROM accounts WHERE name = ?1", [name], |r| r.get(0))?)
    }

    pub fn put_code(&self, hash: &[u8], account: &str, kind: &str, expires: i64, now: i64) -> Result<()> {
        let id = self.account(account, now)?;
        self.db.execute("INSERT INTO enroll_codes (hash, account_id, kind, expires) VALUES (?1, ?2, ?3, ?4)", params![hash, id, kind, expires])?;
        Ok(())
    }

    /// Takes the code with this hash out of the store: of two claims, one gets it.
    pub fn claim_code(&self, hash: &[u8]) -> Result<Option<Code>> {
        Ok(self
            .db
            .query_row(
                "DELETE FROM enroll_codes WHERE hash = ?1
                 RETURNING (SELECT name FROM accounts WHERE id = account_id), kind, expires",
                [hash],
                |r| Ok(Code { account: r.get(0)?, kind: r.get(1)?, expires: r.get(2)? }),
            )
            .optional()?)
    }

    pub fn put_cert(&self, serial: u64, account: &str, device_key: &[u8], not_after: i64, cert: &[u8], now: i64) -> Result<()> {
        let id = self.account(account, now)?;
        self.db.execute(
            "INSERT INTO certs (serial, account_id, device_key, not_after, cert) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![serial as i64, id, device_key, not_after, cert],
        )?;
        Ok(())
    }

    pub fn cert(&self, serial: u64) -> Result<Option<Vec<u8>>> {
        Ok(self.db.query_row("SELECT cert FROM certs WHERE serial = ?1", [serial as i64], |r| r.get(0)).optional()?)
    }

    /// The latest certificate of each device.
    pub fn latest_certs(&self) -> Result<Vec<Vec<u8>>> {
        let mut q = self.db.prepare(
            "SELECT cert FROM certs c WHERE not_after = (SELECT max(not_after) FROM certs WHERE device_key = c.device_key)
             GROUP BY device_key",
        )?;
        let rows = q.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?;
        Ok(rows)
    }

    pub fn revoke(&self, device_key: &[u8], now: i64) -> Result<()> {
        self.db.execute("INSERT OR IGNORE INTO revoked (device_key, at) VALUES (?1, ?2)", params![device_key, now])?;
        Ok(())
    }

    pub fn is_revoked(&self, device_key: &[u8]) -> Result<bool> {
        Ok(self.db.query_row("SELECT 1 FROM revoked WHERE device_key = ?1", [device_key], |_| Ok(())).optional()?.is_some())
    }

    /// The channel's id, made if it does not exist.
    pub fn channel(&self, account: &str, provider: &str, class: &str, name: &str, now: i64) -> Result<i64> {
        let id = self.account(account, now)?;
        self.db.execute(
            "INSERT OR IGNORE INTO channels (account_id, provider, class, name, updated) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![id, provider, class, name, now],
        )?;
        Ok(self.db.query_row(
            "SELECT id FROM channels WHERE account_id = ?1 AND provider = ?2 AND class = ?3 AND name = ?4",
            params![id, provider, class, name],
            |r| r.get(0),
        )?)
    }

    pub fn channels(&self, account: &str) -> Result<Vec<Channel>> {
        let mut q = self.db.prepare(
            "SELECT c.id, a.name, provider, class, c.name, cursor, stage FROM channels c JOIN accounts a ON a.id = account_id
             WHERE a.name = ?1 ORDER BY c.id",
        )?;
        let rows = q
            .query_map([account], |r| {
                Ok(Channel {
                    id: r.get(0)?,
                    account: r.get(1)?,
                    provider: r.get(2)?,
                    class: r.get(3)?,
                    name: r.get(4)?,
                    cursor: r.get(5)?,
                    stage: r.get(6)?,
                })
            })?
            .collect::<Result<_, _>>()?;
        Ok(rows)
    }

    /// Writes a sync's items and the channel's new cursor and stage together: either all of
    /// it is stored or none (ADR-0003's crash-safe multi-step writes).
    pub fn sync_batch(&mut self, channel: i64, items: &[Item], cursor: Option<&[u8]>, stage: &str, now: i64) -> Result<()> {
        let tx = self.db.transaction()?;
        for it in items {
            tx.execute(
                "INSERT INTO items (channel_id, provider_id, etag, key_version, sealed, updated) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT (channel_id, provider_id) DO UPDATE SET
                   etag = excluded.etag, key_version = excluded.key_version, sealed = excluded.sealed, updated = excluded.updated",
                params![channel, it.provider_id, it.etag, it.key_version, it.sealed, now],
            )?;
        }
        let n = tx.execute("UPDATE channels SET cursor = ?2, stage = ?3, updated = ?4 WHERE id = ?1", params![channel, cursor, stage, now])?;
        if n != 1 {
            bail!("no channel {channel}");
        }
        tx.commit()?;
        Ok(())
    }

    pub fn items(&self, channel: i64) -> Result<Vec<Item>> {
        let mut q = self.db.prepare(
            "SELECT id, channel_id, provider_id, etag, key_version, sealed, updated FROM items WHERE channel_id = ?1 ORDER BY id",
        )?;
        let rows = q
            .query_map([channel], |r| {
                Ok(Item {
                    id: r.get(0)?,
                    channel_id: r.get(1)?,
                    provider_id: r.get(2)?,
                    etag: r.get(3)?,
                    key_version: r.get(4)?,
                    sealed: r.get(5)?,
                    updated: r.get(6)?,
                })
            })?
            .collect::<Result<_, _>>()?;
        Ok(rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (tempfile::TempDir, Store) {
        let d = tempfile::tempdir().unwrap();
        let s = Store::open(&d.path().join("nitya.db")).unwrap();
        (d, s)
    }

    fn item(pid: &str, sealed: &[u8]) -> Item {
        Item { id: 0, channel_id: 0, provider_id: pid.into(), etag: Some("e1".into()), key_version: 1, sealed: sealed.to_vec(), updated: 0 }
    }

    #[test]
    fn opening_migrates_once_and_is_private() {
        use std::os::unix::fs::PermissionsExt;
        let (d, s) = store();
        assert_eq!(s.version().unwrap(), MIGRATIONS.len());
        s.account("ank", 1).unwrap();
        drop(s);
        let p = d.path().join("nitya.db");
        assert_eq!(std::fs::metadata(&p).unwrap().permissions().mode() & 0o777, 0o600);
        let s = Store::open(&p).unwrap();
        assert_eq!(s.account("ank", 2).unwrap(), 1);
    }

    #[test]
    fn a_newer_database_is_refused() {
        let (d, s) = store();
        s.db.pragma_update(None, "user_version", 99).unwrap();
        drop(s);
        let e = Store::open(&d.path().join("nitya.db")).err().unwrap();
        assert!(format!("{e:#}").contains("newer than this binary"), "{e:#}");
    }

    #[test]
    fn a_code_is_claimed_once() {
        let (_d, s) = store();
        s.put_code(b"h", "ank", "node", 100, 1).unwrap();
        assert_eq!(s.claim_code(b"h").unwrap(), Some(Code { account: "ank".into(), kind: "node".into(), expires: 100 }));
        assert_eq!(s.claim_code(b"h").unwrap(), None);
    }

    #[test]
    fn certificates_and_revocation() {
        let (_d, s) = store();
        s.put_cert(1, "ank", b"k1", 10, b"c1", 0).unwrap();
        s.put_cert(2, "ank", b"k1", 20, b"c2", 0).unwrap();
        s.put_cert(3, "ank", b"k2", 15, b"c3", 0).unwrap();
        let mut latest = s.latest_certs().unwrap();
        latest.sort();
        assert_eq!(latest, vec![b"c2".to_vec(), b"c3".to_vec()]);
        assert_eq!(s.cert(1).unwrap(), Some(b"c1".to_vec()));
        assert!(!s.is_revoked(b"k1").unwrap());
        s.revoke(b"k1", 5).unwrap();
        s.revoke(b"k1", 6).unwrap();
        assert!(s.is_revoked(b"k1").unwrap() && !s.is_revoked(b"k2").unwrap());
    }

    #[test]
    fn a_sync_batch_stores_items_and_cursor_together() {
        let (_d, mut s) = store();
        let ch = s.channel("ank", "google", "calendar", "primary", 0).unwrap();
        assert_eq!(s.channel("ank", "google", "calendar", "primary", 1).unwrap(), ch);
        s.sync_batch(ch, &[item("a", b"x"), item("b", b"y")], Some(b"t1"), "synced", 1).unwrap();
        s.sync_batch(ch, &[item("a", b"x2")], Some(b"t2"), "synced", 2).unwrap();
        let items = s.items(ch).unwrap();
        assert_eq!(items.iter().map(|i| (i.provider_id.as_str(), i.sealed.as_slice())).collect::<Vec<_>>(), vec![("a", &b"x2"[..]), ("b", &b"y"[..])]);
        let c = &s.channels("ank").unwrap()[0];
        assert_eq!((c.cursor.as_deref(), c.stage.as_str()), (Some(&b"t2"[..]), "synced"));
        // A batch for a channel that does not exist stores nothing.
        assert!(s.sync_batch(999, &[item("z", b"z")], None, "synced", 3).is_err());
        assert!(s.db.query_row("SELECT count(*) FROM items WHERE provider_id = 'z'", [], |r| r.get::<_, i64>(0)).unwrap() == 0);
    }
}
