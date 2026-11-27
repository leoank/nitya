//! The nitya node (ADR-0001): its own tsnet identity on each machine, whatever tailnet the
//! machine itself is on, a device key with a certificate from the server (ADR-0011), and
//! the account's keys while it works (ADR-0013).
//!
//!   nitya-node --state-dir DIR --hostname NAME [--authkey-file FILE] [--server nitya:7300]
//!              [--ephemeral] enroll CODE | whoami | keys …
//!
//!   keys fingerprint              this device's wrap key, to compare when approving it
//!   keys init                     the account's first keys; the recovery passphrase on stdin
//!   keys status                   the account's devices and delegated classes
//!   keys approve SERIAL FPR       give device SERIAL the keys, if its fingerprint is FPR
//!   keys recover                  unlock with the recovery passphrase (stdin), approve this device
//!   keys consent CLASS | keys withdraw CLASS   the server's sync delegate for CLASS
//!
//! DIR holds the tailnet identity (tailnet/), the device key (device.key), its wrap key
//! (wrap.key), its certificate (device.cert) and the server's CA key (ca.pub). Calls
//! renew the certificate first once half its life has passed; a keys command that
//! unlocks rotates the keys first when a device was revoked.

use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use ed25519_dalek::SigningKey;
use nitya_device as dev;
use nitya_keys::{CLASSES, Identity, Unlocked};
use nitya_proto::v1::devices_client::DevicesClient;
use nitya_proto::v1::keys_client::KeysClient;
use nitya_proto::v1::{Cert, CertBody, EnrollRequest, GetKeysRequest, GetKeysResponse, PutKeysRequest, RenewRequest, WhoAmIRequest};
use nitya_tailnet::{AuthKey, Options, Tailnet};
use prost::Message;
use tonic::transport::Channel;

const USAGE: &str = "usage: nitya-node --state-dir DIR --hostname NAME [--authkey-file F] [--server HOST:PORT] [--ephemeral] enroll CODE | whoami | keys (fingerprint | init | status | approve SERIAL FPR | recover | consent CLASS | withdraw CLASS)";

enum Command {
    Enroll(String),
    WhoAmI,
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
            "--version" => return Ok(None),
            "enroll" if cmd.is_none() => cmd = Some(Command::Enroll(val()?)),
            "whoami" if cmd.is_none() => cmd = Some(Command::WhoAmI),
            "keys" if cmd.is_none() => cmd = Some(Command::Keys(a.by_ref().collect())),
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
        Command::Keys(k) => {
            let (cert, body) = current_cert(&mut client, &a.dir, &device).await?;
            let mut call = KeysCall { client: KeysClient::new(channel), device, cert };
            keys(&k, &mut call, &body, &wrap).await?;
        }
    }
    Ok(())
}
