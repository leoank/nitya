//! The nitya node (ADR-0001): its own tsnet identity on each machine, whatever tailnet the
//! machine itself is on, a device key with a certificate from the server (ADR-0011), and
//! the account's keys while it works (ADR-0013).
//!
//!   nitya-node --state-dir DIR --hostname NAME [--authkey-file FILE] [--server nitya:7300]
//!              [--ephemeral] enroll CODE | whoami | audit verify | keys …
//!
//!   keys fingerprint              this device's wrap key, to compare when approving it
//!   keys init                     the account's first keys; the recovery passphrase on stdin
//!   keys status                   the account's devices and delegated classes
//!   keys approve SERIAL FPR       give device SERIAL the keys, if its fingerprint is FPR
//!   keys recover                  unlock with the recovery passphrase (stdin), approve this device
//!   keys consent CLASS | keys withdraw CLASS   the server's sync delegate for CLASS
//!
//!   audit verify [--all]          check the account's audit log from this device's
//!                                 checkpoint on (--all: from the start, which also
//!                                 catches edits to older entries' details), show what
//!                                 is new, hand back the new head
//!
//!   channels                      the account's provider channels and where each stands
//!   channels add google [--no-browser]
//!                                 sign a Google account in here, in the browser; choose
//!                                 which of its calendars become channels
//!   sync now [ID]                 sync channel ID (or every channel) at once
//!   events CH                     channel CH's events, opened with the account's keys
//!   events show CH ID | add CH FILE | edit CH ID [FILE] | delete CH ID
//!                                 one event as iCalendar; a new one from FILE (- for
//!                                 stdin); a changed one from FILE or $EDITOR; written to
//!                                 the provider at once, refused if it cannot hold it
//!
//! DIR holds the tailnet identity (tailnet/), the device key (device.key), its wrap key
//! (wrap.key), its certificate (device.cert), the server's CA key (ca.pub), the server's
//! audit key as first seen (audit.pub) and the last checkpoint (audit.checkpoint). Calls
//! renew the certificate first once half its life has passed; every keys command checks
//! the audit log first and stops if its history was rewritten, and one that unlocks
//! rotates the keys first when a device was revoked.

use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use ed25519_dalek::SigningKey;
use nitya_device as dev;
use nitya_keys::{CLASSES, Identity, Unlocked};
use nitya_proto::v1::audit_client::AuditClient;
use nitya_proto::v1::channels_client::ChannelsClient;
use nitya_proto::v1::devices_client::DevicesClient;
use nitya_proto::v1::keys_client::KeysClient;
use nitya_proto::v1::{
    AddRequest, Cert, CertBody, Checkpoint, EnrollRequest, ExchangeRequest, GetKeysRequest, GetKeysResponse, ItemsRequest, ListChannelsRequest, PutKeysRequest, RenewRequest,
    SignInRequest, SinceRequest, SyncNowRequest, WhoAmIRequest, WriteRequest,
};
use nitya_tailnet::{AuthKey, Options, Tailnet};
use prost::Message;
use tonic::transport::Channel;

const USAGE: &str = "usage: nitya-node --state-dir DIR --hostname NAME [--authkey-file F] [--server HOST:PORT] [--ephemeral] enroll CODE | whoami | audit verify [--all] | channels [add google [--no-browser]] | sync now [ID] | events CH [show CH ID | add CH FILE | edit CH ID [FILE] | delete CH ID] | keys (fingerprint | init | status | approve SERIAL FPR | recover | consent CLASS | withdraw CLASS)";

const HELP: &str = "\
nitya-node: nitya's node on this machine, on its own tailnet identity, with a device key
and a certificate from the server.

Usage:
  nitya-node --state-dir DIR --hostname NAME [--authkey-file F] [--server HOST:PORT]
             [--ephemeral] COMMAND

Commands:
  enroll CODE                  enroll this device with a code from `nitya-server enroll-code`
  whoami                       who the server takes this device to be (the default)
  audit verify [--all]         check the account's audit log from this device's checkpoint
                               (--all: from the first entry), print the new entries, and
                               hand the server the head this device verified
  channels                     the account's provider channels: stage, last sync, last error
  channels add google [--no-browser]
                               sign a Google account in, in this machine's browser (or
                               print the URL), and choose which of its calendars to sync,
                               each a channel; needs `keys consent tokens`
  sync now [ID]                sync channel ID, or every channel of the account, now rather
                               than at the next poll; prints what each run did
  events CH                    channel CH's events: id, start, summary; opened here with
                               the account's keys
  events show CH ID            one event as iCalendar (ID: its id or a unique prefix)
  events add CH FILE           a new event from an iCalendar FILE (- for stdin)
  events edit CH ID [FILE]     change an event: from FILE, or in $EDITOR
  events delete CH ID          delete an event
                               Changes are written to the provider at once; one it cannot
                               hold (Google: floating time, RANGE=THISANDFUTURE) is refused
                               before it is sent, and one changed at the provider since
                               this device read it is refused, not overwritten
  keys fingerprint             this device's wrap key fingerprint, to compare when another
                               device approves it; needs no server
  keys init                    make the account's keys on its first device; reads the
                               recovery passphrase from the terminal (twice) or stdin
  keys status                  the account's devices, which hold the keys, the classes
                               delegated to the server
  keys approve SERIAL FPR      give device SERIAL the account's keys, if its fingerprint
                               is FPR (from `keys fingerprint` on that device)
  keys recover                 unlock with the recovery passphrase and approve this device
  keys consent CLASS           let the server read and write CLASS while no device is online
  keys withdraw CLASS          take that back; CLASS is mail, calendar, tasks, tokens, files

Every keys command checks the audit log first and stops if its history was rewritten;
one that unlocks rotates the keys first when a device was revoked. Calls renew the
certificate once half its life has passed.

Options:
  --state-dir DIR     tailnet identity (tailnet/), device key, wrap key, certificate, the
                      CA's and the audit log's keys as first seen, the audit checkpoint
  --hostname NAME     this node's tailnet hostname, one per machine (nitya-node-rogue)
  --authkey-file F    a tailnet auth key, for the first join
  --server HOST:PORT  the server on the tailnet (default nitya:7300)
  --ephemeral         join as an ephemeral node
  --help, --version
";

enum Command {
    Enroll(String),
    WhoAmI,
    AuditVerify { all: bool },
    Channels(Vec<String>),
    SyncNow(i64),
    Events(Vec<String>),
    Keys(Vec<String>),
}

struct Args {
    dir: PathBuf,
    hostname: String,
    authkey: Option<PathBuf>,
    server: String,
    ephemeral: bool,
    cmd: Command,
}

fn args() -> Result<Option<Args>> {
    let mut a = std::env::args().skip(1);
    let (mut dir, mut hostname, mut authkey, mut server, mut ephemeral) = (None, None, None, "nitya:7300".to_string(), false);
    let mut cmd = None;
    while let Some(x) = a.next() {
        let mut val = || a.next().with_context(|| format!("{x} needs a value"));
        match x.as_str() {
            "--state-dir" => dir = Some(PathBuf::from(val()?)),
            "--hostname" => hostname = Some(val()?),
            "--authkey-file" => authkey = Some(PathBuf::from(val()?)),
            "--server" => server = val()?,
            "--ephemeral" => ephemeral = true,
            "--all" if matches!(cmd, Some(Command::AuditVerify { .. })) => cmd = Some(Command::AuditVerify { all: true }),
            "--version" => return Ok(None),
            "--help" => {
                print!("{HELP}");
                std::process::exit(0)
            }
            "enroll" if cmd.is_none() => cmd = Some(Command::Enroll(val()?)),
            "whoami" if cmd.is_none() => cmd = Some(Command::WhoAmI),
            "keys" if cmd.is_none() => cmd = Some(Command::Keys(a.by_ref().collect())),
            "channels" if cmd.is_none() => cmd = Some(Command::Channels(a.by_ref().collect())),
            "events" if cmd.is_none() => cmd = Some(Command::Events(a.by_ref().collect())),
            "sync" if cmd.is_none() => {
                if val()? != "now" {
                    bail!("{USAGE}");
                }
                cmd = Some(Command::SyncNow(0))
            }
            id if matches!(cmd, Some(Command::SyncNow(0))) => cmd = Some(Command::SyncNow(id.parse().context("a channel id is a number (see `channels`)")?)),
            "audit" if cmd.is_none() => {
                if val()? != "verify" {
                    bail!("{USAGE}");
                }
                cmd = Some(Command::AuditVerify { all: false })
            }
            _ => bail!("unknown argument {x}\n{USAGE}"),
        }
    }
    Ok(Some(Args {
        dir: dir.with_context(|| format!("--state-dir is required\n{USAGE}"))?,
        hostname: hostname.context("--hostname is required (one per machine, e.g. nitya-node-rogue)")?,
        authkey,
        server,
        ephemeral,
        cmd: cmd.unwrap_or(Command::WhoAmI),
    }))
}

/// The certificate in `dir`, checked against the CA key kept at enrollment.
fn load_cert(dir: &Path, device: &SigningKey) -> Result<(Cert, CertBody)> {
    let bytes = std::fs::read(dir.join("device.cert")).context("this node is not enrolled: run `nitya-node … enroll CODE` with a code from `nitya-server enroll-code`")?;
    let cert = Cert::decode(bytes.as_slice())?;
    let ca = dev::verifying_key(&std::fs::read(dir.join("ca.pub"))?)?;
    let body = dev::verify_cert(&ca, &cert)?;
    if body.device_key != device.verifying_key().to_bytes() {
        bail!("device.cert is not for device.key");
    }
    Ok((cert, body))
}

fn save_cert(dir: &Path, cert: &Cert, device: &SigningKey) -> Result<CertBody> {
    let ca = dev::verifying_key(&std::fs::read(dir.join("ca.pub"))?)?;
    let body = dev::verify_cert(&ca, cert).context("the server's certificate does not verify")?;
    if body.device_key != device.verifying_key().to_bytes() {
        bail!("the server certified another key");
    }
    dev::write_private(&dir.join("device.cert"), &cert.encode_to_vec())?;
    Ok(body)
}

/// The device's certificate, renewed first once half its life has passed.
async fn current_cert(client: &mut DevicesClient<Channel>, dir: &Path, device: &SigningKey) -> Result<(Cert, CertBody)> {
    let (cert, body) = load_cert(dir, device)?;
    if dev::now() <= body.not_after - dev::CERT_LIFETIME_SECS / 2 {
        return Ok((cert, body));
    }
    let mut req = tonic::Request::new(RenewRequest {});
    dev::sign_request(&mut req, device, &cert, "nitya.v1.Devices/Renew", dev::now());
    let cert = client.renew(req).await.context("renewing the certificate")?.into_inner();
    let body = save_cert(dir, &cert, device)?;
    eprintln!("nitya-node: renewed, serial {} until {}", body.serial, body.not_after);
    Ok((cert, body))
}

/// The recovery passphrase from stdin: asked twice without echo on a terminal when it is
/// being set, read as one line otherwise.
fn passphrase(new: bool) -> Result<String> {
    if std::io::stdin().is_terminal() {
        let p = rpassword::prompt_password("recovery passphrase: ")?;
        if new && rpassword::prompt_password("again: ")? != p {
            bail!("the passphrases differ");
        }
        return Ok(p);
    }
    let mut line = String::new();
    std::io::stdin().read_line(&mut line)?;
    Ok(line.trim_end_matches(['\r', '\n']).to_string())
}

/// Checks the account's audit log from this device's checkpoint on (ADR-0015): the
/// server's audit key as first seen, an unbroken chain, other devices' checkpoints; then
/// keeps and hands back the new head. Returns the entries that are new to this device.
async fn verify_audit(channel: Channel, dir: &Path, device: &SigningKey, cert: &Cert, account: &str, all: bool) -> Result<Vec<nitya_audit::AuditEntry>> {
    let mut client = AuditClient::new(channel);
    let saved = match std::fs::read(dir.join("audit.checkpoint")) {
        Ok(b) => Some(Checkpoint::decode(b.as_slice())?),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e).context("reading audit.checkpoint"),
    };
    let saved = saved.map(|c| nitya_audit::Head { seq: c.seq, hash: c.hash.clone() });
    let from = if all { None } else { saved.clone() };
    let mut req = tonic::Request::new(SinceRequest { from: from.as_ref().map_or(0, |h| h.seq) });
    dev::sign_request(&mut req, device, cert, "nitya.v1.Audit/Since", dev::now());
    let r = client.since(req).await.context("reading the audit log")?.into_inner();
    let pinned = dir.join("audit.pub");
    match std::fs::read(&pinned) {
        Ok(k) if k != r.audit_key => bail!("the server's audit key changed since this device first saw it: the log cannot be trusted"),
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => dev::write_private(&pinned, &r.audit_key)?,
        Err(e) => return Err(e).context("reading audit.pub"),
    }
    let key = dev::verifying_key(&r.audit_key)?;
    let (head, entries) = nitya_audit::verify(&key, account, from.as_ref(), &r.entries).context("the audit log does not verify")?;
    if let Some(c) = saved.as_ref().filter(|_| all) {
        match r.entries.get(c.seq as usize) {
            Some(e) if e.hash == c.hash => {}
            Some(_) => bail!("the history was rewritten: entry {} is not the one this device checkpointed", c.seq),
            None => bail!("the log is shorter than this device's checkpoint at entry {}", c.seq),
        }
    }
    let first = entries.first().map_or(0, |e| e.seq);
    let mut agreed = 0;
    for c in r.checkpoints.iter().filter(|c| c.device_key != device.verifying_key().to_bytes()) {
        if nitya_audit::verify_checkpoint(c).is_err() || c.seq < first {
            continue;
        }
        match r.entries.get((c.seq - first) as usize) {
            Some(e) if e.hash == c.hash => agreed += 1,
            Some(_) => bail!("the history was rewritten: another device checkpointed a different entry {}", c.seq),
            None => bail!("the log is shorter than another device's checkpoint at entry {}", c.seq),
        }
    }
    let cp = nitya_audit::sign_checkpoint(device, account, &head, dev::now());
    dev::write_private(&dir.join("audit.checkpoint"), &cp.encode_to_vec())?;
    let mut req = tonic::Request::new(cp);
    dev::sign_request(&mut req, device, cert, "nitya.v1.Audit/Submit", dev::now());
    client.submit(req).await.context("handing back the checkpoint")?;
    eprintln!("nitya-node: audit log of {account} verified to entry {}; {agreed} other device checkpoint(s) agree", head.seq);
    Ok(entries.into_iter().skip(usize::from(from.is_some())).collect())
}

struct KeysCall {
    client: KeysClient<Channel>,
    device: SigningKey,
    cert: Cert,
}

impl KeysCall {
    async fn get(&mut self) -> Result<GetKeysResponse> {
        let mut req = tonic::Request::new(GetKeysRequest {});
        dev::sign_request(&mut req, &self.device, &self.cert, "nitya.v1.Keys/Get", dev::now());
        Ok(self.client.get(req).await.context("reading the account's keys")?.into_inner())
    }

    async fn put(&mut self, base_revision: u64, u: &Unlocked) -> Result<u64> {
        let mut req = tonic::Request::new(PutKeysRequest { base_revision, keys: Some(u.keys.clone()) });
        dev::sign_request(&mut req, &self.device, &self.cert, "nitya.v1.Keys/Put", dev::now());
        Ok(self.client.put(req).await.context("writing the account's keys")?.into_inner().revision)
    }
}

/// Rotates the keys when the server says a device holding a wrap was revoked (ADR-0010).
fn rotate_if_pending(u: &mut Unlocked, r: &GetKeysResponse) -> Result<bool> {
    if !r.rotation_pending {
        return Ok(false);
    }
    let revoked = |k: &[u8]| r.members.iter().any(|m| m.device_key == k && m.revoked);
    u.rotate(revoked, &r.delegate_key)?;
    eprintln!("nitya-node: a device was revoked; rotated the keys to user key version {}", u.keys.uk_version);
    Ok(true)
}

async fn keys(args: &[String], call: &mut KeysCall, body: &CertBody, wrap: &Identity) -> Result<()> {
    let me = body.device_key.as_slice();
    let my_wrap = wrap.to_public().to_string();
    let r = call.get().await?;
    let unlock = |r: &GetKeysResponse| -> Result<Unlocked> {
        let keys = r.keys.clone().context("the account has no keys yet: `keys init` on its first device")?;
        nitya_keys::unlock(&body.account, keys, me, wrap)
    };
    let class = |c: &str| -> Result<String> {
        if !CLASSES.contains(&c) {
            bail!("a class is one of {}", CLASSES.join(", "));
        }
        Ok(c.to_string())
    };
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    match args.as_slice() {
        ["init"] => {
            if r.keys.is_some() {
                bail!("the account has keys: approve this device from one that has them (`keys fingerprint` here, `keys approve` there), or `keys recover`");
            }
            let u = nitya_keys::create(&body.account, me, &my_wrap, &passphrase(true)?)?;
            call.put(0, &u).await?;
            println!("made the keys of {}; this device holds them", body.account);
        }
        ["status"] => {
            let unlocked = match unlock(&r) {
                Ok(mut u) => {
                    if rotate_if_pending(&mut u, &r)? {
                        call.put(r.revision, &u).await?;
                    }
                    Some(u)
                }
                Err(e) => {
                    eprintln!("nitya-node: not unlocked here: {e:#}");
                    None
                }
            };
            let keys = unlocked.as_ref().map(|u| u.keys.clone()).or(r.keys.clone()).unwrap_or_default();
            for m in &r.members {
                let state = if m.revoked {
                    "revoked"
                } else if keys.devices.iter().any(|d| d.device_key == m.device_key) {
                    "holds the keys"
                } else {
                    "not approved"
                };
                let this = if m.device_key == me { "  (this device)" } else { "" };
                println!("{}\t{}\tserial {}\t{}\t{state}{this}", m.kind, m.hostname, m.serial, nitya_keys::fingerprint(&m.wrap_key));
            }
            let mut delegated: Vec<&str> = keys.delegated.iter().map(|k| k.class.as_str()).collect();
            delegated.sort();
            delegated.dedup();
            println!("user key version {}; delegated to the server: {}", keys.uk_version, if delegated.is_empty() { "none".into() } else { delegated.join(", ") });
            if r.rotation_pending && unlocked.is_none() {
                println!("a revoked device held the keys: the next device that unlocks rotates them");
            }
        }
        ["approve", serial, fpr] => {
            let serial: u64 = serial.parse().context("a serial is a number")?;
            let m = r.members.iter().find(|m| m.serial == serial && !m.revoked).context("no unrevoked device of this account with that serial (see `keys status`)")?;
            let want = nitya_keys::fingerprint(&m.wrap_key);
            if want != fpr.trim().to_lowercase() {
                bail!("device {serial}'s wrap key has fingerprint {want}, not {fpr}: do not approve it");
            }
            let mut u = unlock(&r)?;
            rotate_if_pending(&mut u, &r)?;
            u.approve(&m.device_key, &m.wrap_key)?;
            call.put(r.revision, &u).await?;
            println!("approved {} ({}): it holds the keys of {}", m.hostname, m.kind, body.account);
        }
        ["recover"] => {
            let keys = r.keys.clone().context("the account has no keys to recover")?;
            let mut u = nitya_keys::recover(&body.account, keys, &passphrase(false)?)?;
            rotate_if_pending(&mut u, &r)?;
            u.approve(me, &my_wrap)?;
            call.put(r.revision, &u).await?;
            println!("recovered the keys of {}; this device holds them", body.account);
        }
        ["consent", c] => {
            let c = class(c)?;
            let mut u = unlock(&r)?;
            rotate_if_pending(&mut u, &r)?;
            u.consent(&c, &r.delegate_key)?;
            call.put(r.revision, &u).await?;
            println!("the server may now read and write {c} while no device is online; withdraw with `keys withdraw {c}`");
        }
        ["withdraw", c] => {
            let c = class(c)?;
            let mut u = unlock(&r)?;
            rotate_if_pending(&mut u, &r)?;
            u.withdraw(&c);
            call.put(r.revision, &u).await?;
            println!("the server no longer holds any {c} key; what it receives is sealed to the account until a device opens it");
        }
        _ => bail!("{USAGE}"),
    }
    Ok(())
}

/// A device's signature on a call.
struct Call {
    device: SigningKey,
    cert: Cert,
}

impl Call {
    fn req<T>(&self, msg: T, method: &str) -> tonic::Request<T>
    where
        T: Message,
    {
        let mut r = tonic::Request::new(msg);
        dev::sign_request(&mut r, &self.device, &self.cert, method, dev::now());
        r
    }
}

fn b64(b: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(b)
}

fn random(n: usize) -> Vec<u8> {
    use rand::RngCore;
    let mut b = vec![0u8; n];
    rand::thread_rng().fill_bytes(&mut b);
    b
}

/// `%XX` and `+` decoded, as a query string's values come.
fn query_value(v: &str) -> String {
    let b = v.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < b.len() => {
                match std::str::from_utf8(&b[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok()).ok_or(()) {
                    Ok(x) => {
                        out.push(x);
                        i += 2;
                    }
                    Err(()) => out.push(b'%'),
                }
            }
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Waits on `listener` for the sign-in's redirect and answers the browser; the query's
/// pairs. Requests without a code or an error (a favicon) are answered and skipped.
fn redirect(listener: std::net::TcpListener) -> Result<std::collections::HashMap<String, String>> {
    use std::io::{BufRead, BufReader, Write as _};
    loop {
        let (mut stream, _) = listener.accept()?;
        let mut line = String::new();
        BufReader::new(&stream).read_line(&mut line)?;
        let target = line.split_whitespace().nth(1).unwrap_or("/");
        let q: std::collections::HashMap<String, String> = target
            .split_once('?')
            .map(|(_, q)| q.split('&').filter_map(|kv| kv.split_once('=')).map(|(k, v)| (k.to_string(), query_value(v))).collect())
            .unwrap_or_default();
        let done = q.contains_key("code") || q.contains_key("error");
        let page = if done { "nitya has the answer; you can close this tab." } else { "" };
        let _ = write!(stream, "HTTP/1.1 200 OK\r\ncontent-type: text/plain; charset=utf-8\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{page}", page.len());
        if done {
            return Ok(q);
        }
    }
}

/// Signs a Google account in through this machine's browser (RFC 8252: a loopback
/// redirect and PKCE): the server holds the OAuth client, trades the code for the tokens
/// and keeps them; nothing secret passes through here but the one-time code.
async fn add_google(mut client: ChannelsClient<Channel>, device: &SigningKey, cert: &Cert, no_browser: bool) -> Result<()> {
    use sha2::Digest;
    let call = Call { device: device.clone(), cert: cert.clone() };
    let listener = std::net::TcpListener::bind("127.0.0.1:0").context("listening on loopback for the sign-in's redirect")?;
    let redirect_uri = format!("http://127.0.0.1:{}/", listener.local_addr()?.port());
    let verifier = b64(&random(32));
    let challenge = b64(&sha2::Sha256::digest(verifier.as_bytes()));
    let state = b64(&random(16));
    let req = SignInRequest { provider: "google".into(), redirect_uri: redirect_uri.clone(), code_challenge: challenge, state: state.clone() };
    let url = client.sign_in(call.req(req, "nitya.v1.Channels/SignIn")).await.context("starting the sign-in")?.into_inner().url;
    eprintln!("nitya-node: sign in to Google in the browser:\n\n  {url}\n");
    if !no_browser {
        let opener = if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
        if std::process::Command::new(opener).arg(&url).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn().is_err() {
            eprintln!("nitya-node: could not start {opener}; open the URL yourself");
        }
    }
    let q = tokio::time::timeout(std::time::Duration::from_secs(600), tokio::task::spawn_blocking(move || redirect(listener)))
        .await
        .context("no sign-in within 10 minutes")???;
    if let Some(e) = q.get("error") {
        bail!("Google did not sign the account in: {e}");
    }
    if q.get("state") != Some(&state) {
        bail!("the redirect's state is not this sign-in's: not using it");
    }
    let code = q.get("code").cloned().unwrap_or_default();
    let req = ExchangeRequest { provider: "google".into(), code, code_verifier: verifier, redirect_uri };
    let x = client.exchange(call.req(req, "nitya.v1.Channels/Exchange")).await.context("handing the server the sign-in")?.into_inner();
    if x.calendars.is_empty() {
        bail!("the account has no calendars");
    }
    for (i, c) in x.calendars.iter().enumerate() {
        let had = if c.channel_id == 0 { String::new() } else { format!("  (channel {})", c.channel_id) };
        println!("{:>3}  {}{}\t{}\t{}{had}", i + 1, c.summary, if c.primary { " (primary)" } else { "" }, c.role, c.id);
    }
    eprint!("calendars to sync, by number (1,3) or all: ");
    let mut line = String::new();
    std::io::stdin().read_line(&mut line)?;
    let line = line.trim();
    let chosen: Vec<String> = if line == "all" {
        x.calendars.iter().map(|c| c.id.clone()).collect()
    } else {
        line.split([',', ' ']).filter(|s| !s.is_empty()).map(|n| {
            let i: usize = n.parse().with_context(|| format!("{n} is not a number from the list"))?;
            x.calendars.get(i.wrapping_sub(1)).map(|c| c.id.clone()).with_context(|| format!("no calendar {i}"))
        }).collect::<Result<_>>()?
    };
    if chosen.is_empty() {
        bail!("no calendar chosen; nothing added");
    }
    let req = AddRequest { handle: x.handle, calendar_ids: chosen };
    let r = client.add(call.req(req, "nitya.v1.Channels/Add")).await.context("adding the calendars")?.into_inner();
    for c in r.channels {
        println!("channel {}: {} {} {} ({})", c.id, c.provider, c.class, c.name, c.stage);
    }
    println!("they sync within a minute while the server holds consent to tokens and calendar (`keys status`)");
    Ok(())
}

/// What a device seals a change under; the server opens it under the same name.
fn change_ref(channel: i64) -> String {
    format!("channel:{channel}:change")
}

/// An event's start as a person reads it, and a key that sorts by it.
fn start_of(ev: &nitya_ical::Comp) -> (String, String) {
    let Some(p) = ev.get("DTSTART") else { return (String::new(), "-".into()) };
    let v = p.value.trim();
    let shown = if v.len() >= 15 {
        let zone = if v.ends_with('Z') { "Z".to_string() } else { p.param("TZID").map(|z| format!(" {z}")).unwrap_or_default() };
        format!("{}-{}-{} {}:{}{zone}", &v[0..4], &v[4..6], &v[6..8], &v[9..11], &v[11..13])
    } else if v.len() == 8 {
        format!("{}-{}-{}", &v[0..4], &v[4..6], &v[6..8])
    } else {
        v.to_string()
    };
    (v.to_string(), shown)
}

/// The text of `path`, or stdin for `-`.
fn read_input(path: &str) -> Result<Vec<u8>> {
    use std::io::Read;
    if path == "-" {
        let mut b = Vec::new();
        std::io::stdin().read_to_end(&mut b)?;
        return Ok(b);
    }
    std::fs::read(path).with_context(|| format!("reading {path}"))
}

/// `text` changed in $EDITOR (vi without one), in a private temporary directory that is
/// removed after.
fn edit_text(text: &[u8]) -> Result<Vec<u8>> {
    let dir = std::env::temp_dir().join(format!("nitya-edit-{}", b64(&random(9))));
    std::fs::create_dir(&dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
    }
    let f = dir.join("event.ics");
    let out = (|| {
        std::fs::write(&f, text)?;
        let editor = std::env::var("VISUAL").or_else(|_| std::env::var("EDITOR")).unwrap_or_else(|_| "vi".into());
        let status = std::process::Command::new("sh").arg("-c").arg(format!("{editor} \"$1\"")).arg("sh").arg(&f).status().context("starting the editor")?;
        if !status.success() {
            bail!("the editor failed; nothing changed");
        }
        Ok(std::fs::read(&f)?)
    })();
    let _ = std::fs::remove_dir_all(&dir);
    out
}

async fn events(args: &[String], call: &Call, channel: Channel, body: &CertBody, wrap: &Identity) -> Result<()> {
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let (op, ch, rest) = match args.as_slice() {
        [ch] => ("list", *ch, &args[1..]),
        [op, ch, rest @ ..] => (*op, *ch, rest),
        _ => bail!("{USAGE}"),
    };
    let ch: i64 = ch.parse().context("a channel is a number (see `channels`)")?;
    let mut client = ChannelsClient::new(channel.clone()).max_decoding_message_size(256 << 20).max_encoding_message_size(64 << 20);
    let r = client.items(call.req(ItemsRequest { channel_id: ch }, "nitya.v1.Channels/Items")).await.context("reading the channel's items")?.into_inner();
    let keys = KeysClient::new(channel).get(call.req(GetKeysRequest {}, "nitya.v1.Keys/Get")).await.context("reading the account's keys")?.into_inner();
    let u = nitya_keys::unlock(&body.account, keys.keys.context("the account has no keys yet")?, &body.device_key, wrap)?;
    let open = |pid: &str| -> Result<(Vec<u8>, String)> {
        let it = r.items.iter().find(|i| i.provider_id == pid).with_context(|| format!("no item {pid}"))?;
        let s = it.sealed.as_ref().context("an item without its sealed text")?;
        Ok((u.open_item(&r.class, &format!("channel:{ch}:{pid}"), s)?.to_vec(), it.etag.clone()))
    };
    let resolve = |prefix: &str| -> Result<String> {
        let m: Vec<&str> = r.items.iter().map(|i| i.provider_id.as_str()).filter(|p| p.starts_with(prefix)).collect();
        match m.as_slice() {
            [one] => Ok(one.to_string()),
            [] => bail!("no event {prefix} in channel {ch}"),
            _ => bail!("{prefix} names {} events; give more of the id", m.len()),
        }
    };
    let write = |provider_id: String, base_etag: String, data: Option<Vec<u8>>| async {
        let sealed = data.map(|d| u.seal_item(&r.class, &change_ref(ch), &d)).transpose()?;
        let req = WriteRequest { channel_id: ch, provider_id, base_etag, sealed };
        let w = client.clone().write(call.req(req, "nitya.v1.Channels/Write")).await.context("writing")?.into_inner();
        match w.state.as_str() {
            "written" => println!("written: {}", w.provider_id),
            "queued" => println!("queued; the next sync writes it{}", if w.error.is_empty() { String::new() } else { format!(" ({})", w.error) }),
            _ => bail!("refused: {}", w.error),
        }
        anyhow::Ok(())
    };
    match (op, rest) {
        ("list", []) => {
            let mut rows = Vec::new();
            for it in &r.items {
                let (text, _) = open(&it.provider_id)?;
                let root = nitya_ical::parse(&String::from_utf8_lossy(&text));
                for ev in root.find("VEVENT") {
                    let (key, shown) = start_of(ev);
                    let mut what = ev.text("SUMMARY").unwrap_or_default().replace('\n', " ");
                    if ev.text("STATUS").as_deref() == Some("CANCELLED") {
                        what = format!("(cancelled) {what}");
                    }
                    if ev.get("RRULE").is_some() {
                        what += "  (series)";
                    } else if ev.get("RECURRENCE-ID").is_some() {
                        what += "  (one occurrence)";
                    }
                    rows.push((key, it.provider_id.clone(), shown, what));
                }
            }
            rows.sort();
            for (_, id, start, what) in rows {
                println!("{id}\t{start}\t{what}");
            }
            for p in &r.pending {
                let what = if p.delete { "delete" } else if p.provider_id.is_empty() { "new" } else { "edit" };
                let state = if p.error.is_empty() { "waiting".to_string() } else { format!("refused: {}", p.error) };
                println!("change {}\t{what} {}\t{state}", p.id, p.provider_id);
            }
        }
        ("show", [id]) => {
            let (text, _) = open(&resolve(id)?)?;
            print!("{}", String::from_utf8_lossy(&text));
        }
        ("add", [file]) => write(String::new(), String::new(), Some(read_input(file)?)).await?,
        ("edit", [id, file @ ..]) if file.len() <= 1 => {
            let id = resolve(id)?;
            let (text, etag) = open(&id)?;
            let new = match file {
                [f] => read_input(f)?,
                _ => edit_text(&text)?,
            };
            if new == text {
                println!("no change");
                return Ok(());
            }
            write(id, etag, Some(new)).await?;
        }
        ("delete", [id]) => {
            let id = resolve(id)?;
            let (_, etag) = open(&id)?;
            write(id, etag, None).await?;
        }
        _ => bail!("{USAGE}"),
    }
    Ok(())
}

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<()> {
    let Some(a) = args()? else {
        println!("nitya-node {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    };
    std::fs::create_dir_all(&a.dir)?;
    let device = dev::load_or_generate(&a.dir.join("device.key"))?;
    let wrap = nitya_keys::load_or_generate(&a.dir.join("wrap.key"))?;
    if let Command::Keys(k) = &a.cmd
        && k.first().map(String::as_str) == Some("fingerprint")
    {
        println!("{}", nitya_keys::fingerprint(&wrap.to_public().to_string()));
        return Ok(());
    }
    let tailnet: Arc<Tailnet> = Tailnet::start(Options {
        name: "nitya".into(),
        dir: a.dir.join("tailnet"),
        hostname: a.hostname,
        authkey: a.authkey.map(AuthKey::File),
        ephemeral: a.ephemeral,
        give_up_after: Some(std::time::Duration::from_secs(60)),
    })
    .await?;
    let channel: Channel = tonic::transport::Endpoint::try_from(format!("http://{}", a.server))?
        .connect_with_connector(tailnet.connector())
        .await
        .with_context(|| format!("connecting to {} on the tailnet", a.server))?;
    let mut client = DevicesClient::new(channel.clone());

    match a.cmd {
        Command::Enroll(code) => {
            let wrap_key = wrap.to_public().to_string();
            let proof = dev::enroll_proof(&device, code.trim(), &wrap_key);
            let r = client
                .enroll(EnrollRequest { code: code.trim().into(), device_key: device.verifying_key().to_bytes().to_vec(), proof, wrap_key })
                .await
                .context("enrolling")?
                .into_inner();
            dev::write_private(&a.dir.join("ca.pub"), &r.ca_key)?;
            let body = save_cert(&a.dir, &r.cert.context("no certificate in the answer")?, &device)?;
            println!("enrolled as a {} of {}, serial {}, until {}", body.kind, body.account, body.serial, body.not_after);
        }
        Command::WhoAmI => {
            let (cert, _) = current_cert(&mut client, &a.dir, &device).await?;
            let mut req = tonic::Request::new(WhoAmIRequest {});
            dev::sign_request(&mut req, &device, &cert, "nitya.v1.Devices/WhoAmI", dev::now());
            let w = client.who_am_i(req).await.context("asking the server who this node is")?.into_inner();
            println!("{} of {}: {} on node {}, serial {}, until {}", w.kind, w.account, w.hostname, w.node_id, w.serial, w.not_after);
        }
        Command::Channels(args) if !args.is_empty() => {
            let (cert, _) = current_cert(&mut client, &a.dir, &device).await?;
            let args: Vec<&str> = args.iter().map(String::as_str).collect();
            let no_browser = match args.as_slice() {
                ["add", "google"] => false,
                ["add", "google", "--no-browser"] => true,
                _ => bail!("{USAGE}"),
            };
            add_google(ChannelsClient::new(channel), &device, &cert, no_browser).await?;
        }
        Command::Events(args) => {
            let (cert, body) = current_cert(&mut client, &a.dir, &device).await?;
            let call = Call { device, cert };
            events(&args, &call, channel, &body, &wrap).await?;
        }
        Command::Channels(_) => {
            let (cert, _) = current_cert(&mut client, &a.dir, &device).await?;
            let mut req = tonic::Request::new(ListChannelsRequest {});
            dev::sign_request(&mut req, &device, &cert, "nitya.v1.Channels/List", dev::now());
            let r = ChannelsClient::new(channel).list(req).await.context("listing the channels")?.into_inner();
            if r.channels.is_empty() {
                println!("no channels yet");
            }
            for c in r.channels {
                let last = if c.last_sync == 0 { "never synced".to_string() } else { format!("synced {}s ago", dev::now() - c.last_sync) };
                let err = if c.last_error.is_empty() { String::new() } else { format!("\t{}", c.last_error) };
                println!("{}\t{}\t{}\t{}\t{}\t{last}{err}", c.id, c.provider, c.class, c.name, c.stage);
            }
        }
        Command::SyncNow(id) => {
            let (cert, _) = current_cert(&mut client, &a.dir, &device).await?;
            let mut req = tonic::Request::new(SyncNowRequest { channel_id: id });
            dev::sign_request(&mut req, &device, &cert, "nitya.v1.Channels/SyncNow", dev::now());
            let r = ChannelsClient::new(channel).sync_now(req).await.context("syncing")?.into_inner();
            if r.reports.is_empty() {
                println!("no channels to sync");
            }
            for s in r.reports {
                let what = if s.error.is_empty() {
                    let mut w = format!("{} sync: {} upserted, {} deleted", if s.full { "full" } else { "incremental" }, s.upserts, s.deletes);
                    if s.written + s.refused > 0 {
                        w += &format!("; {} change(s) written, {} refused", s.written, s.refused);
                    }
                    w
                } else {
                    s.error
                };
                println!("{}\t{}\t{what}", s.channel_id, s.stage);
            }
        }
        Command::AuditVerify { all } => {
            let (cert, body) = current_cert(&mut client, &a.dir, &device).await?;
            for e in verify_audit(channel, &a.dir, &device, &cert, &body.account, all).await? {
                let who = match e.actor.split_once(':') {
                    Some(("system", _)) | None => e.actor.clone(),
                    Some((kind, key)) => format!("{kind}:{}", &key[..key.len().min(8)]),
                };
                println!("{}\t{who}\t{}\t{}\t{}\t{}", e.seq, e.action, e.resource, e.decision, e.policies.join(","));
            }
        }
        Command::Keys(k) => {
            let (cert, body) = current_cert(&mut client, &a.dir, &device).await?;
            verify_audit(channel.clone(), &a.dir, &device, &cert, &body.account, false).await?;
            let mut call = KeysCall { client: KeysClient::new(channel), device, cert };
            keys(&k, &mut call, &body, &wrap).await?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_redirects_query_values_are_decoded() {
        assert_eq!(query_value("4%2F0Ab-x_y"), "4/0Ab-x_y");
        assert_eq!(query_value("a+b%"), "a b%");
        assert_eq!(query_value("%zz"), "%zz");
    }

    #[test]
    fn starts_read_as_a_person_reads_them() {
        let ev = |p: &str| nitya_ical::parse(&format!("BEGIN:VEVENT\r\n{p}\r\nEND:VEVENT\r\n")).find("VEVENT")[0].clone();
        assert_eq!(start_of(&ev("DTSTART;TZID=Europe/London:20270310T090000")).1, "2027-03-10 09:00 Europe/London");
        assert_eq!(start_of(&ev("DTSTART:20270310T090000Z")).1, "2027-03-10 09:00Z");
        assert_eq!(start_of(&ev("DTSTART;VALUE=DATE:20270310")).1, "2027-03-10");
    }
}
