//! The nitya server (ADR-0001): its own tsnet identity, serving nitya's gRPC API on the
//! tailnet (ADR-0011), and the device CA's admin commands.
//!
//!   nitya-server --state-dir DIR [--hostname nitya] [--authkey-file FILE] [--port 7300]
//!                [--ephemeral]                                   serve
//!   nitya-server --state-dir DIR enroll-code --account A [--kind node|app|agent] [--minutes 15]
//!   nitya-server --state-dir DIR devices
//!   nitya-server --state-dir DIR revoke SERIAL
//!
//! The tailnet identity lives in DIR/tailnet; the store is DIR/nitya.db (ADR-0012), the
//! CA's key DIR/ca.key, the audit log's key DIR/audit.key (ADR-0015) and the sync-delegate
//! key DIR/delegate.key (ADR-0013).

mod audit;
mod ca;
mod channels;
mod devices;
mod keys;
mod policy;
mod sync;

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use nitya_device as dev;
use nitya_proto::v1::audit_server::AuditServer;
use nitya_proto::v1::channels_server::ChannelsServer;
use nitya_proto::v1::devices_server::DevicesServer;
use nitya_proto::v1::keys_server::KeysServer;
use nitya_tailnet::{AuthKey, Options, Tailnet};

/// The port nitya's server listens on, on its tailnet identity.
pub const PORT: u16 = 7300;

const USAGE: &str = "usage: nitya-server --state-dir DIR [--hostname H] [--authkey-file F] [--port P] [--ephemeral] [--delegate-key F] [--poll-secs N]\n\
       nitya-server --state-dir DIR enroll-code --account A [--kind node|app|agent] [--minutes M]\n\
       nitya-server --state-dir DIR devices\n\
       nitya-server --state-dir DIR revoke SERIAL";

const HELP: &str = "\
nitya-server: nitya's server on its own tailnet identity, and the admin commands for its
state directory. The admin commands work while the server runs.

Usage:
  nitya-server --state-dir DIR [--hostname H] [--authkey-file F] [--port P] [--ephemeral]
               [--delegate-key F] [--poll-secs N]
  nitya-server --state-dir DIR enroll-code --account A [--kind node|app|agent] [--minutes M]
  nitya-server --state-dir DIR devices
  nitya-server --state-dir DIR revoke SERIAL

Commands:
  (none)        serve the gRPC API on the tailnet: Devices, Keys, Audit, Channels; and
                sync each channel the owner consented to, every --poll-secs
  enroll-code   print a one-time code with which one device enrolls into account A as a
                node, app or agent (default node), valid M minutes (default 15)
  devices       list each device's latest certificate: account, kind, hostname, tailnet
                node id, serial, expiry, and whether it is revoked
  revoke        revoke the device whose certificate has SERIAL, for good: it can no longer
                call, renew or enroll; its wrap of the account's keys is deleted and the
                next device to unlock rotates them

Options:
  --state-dir DIR     tailnet identity (tailnet/), store (nitya.db), CA key (ca.key), audit
                      key (audit.key), sync-delegate key (delegate.key); made on first use
  --hostname H        the server's tailnet hostname (default nitya)
  --authkey-file F    a tailnet auth key, for the first join
  --port P            the port on the tailnet (default 7300)
  --ephemeral         join as an ephemeral node
  --delegate-key F    the sync-delegate key, an age identity file (as age-keygen writes
                      it); without it the server makes delegate.key in DIR
  --poll-secs N       how often each channel is synced (default 60); `nitya-node sync now`
                      syncs at once
  --help, --version
";

enum Command {
    Serve { hostname: String, authkey: Option<PathBuf>, port: u16, ephemeral: bool, delegate: Option<PathBuf>, poll: i64 },
    EnrollCode { account: String, kind: String, minutes: i64 },
    Devices,
    Revoke(u64),
}

fn args() -> Result<Option<(PathBuf, Command)>> {
    let mut a = std::env::args().skip(1);
    let (mut dir, mut hostname, mut authkey, mut port, mut ephemeral) = (None, "nitya".to_string(), None, PORT, false);
    let mut delegate = None;
    let mut poll = 60;
    let (mut account, mut kind, mut minutes) = (None, "node".to_string(), 15);
    let mut sub: Option<String> = None;
    let mut serial = None;
    while let Some(x) = a.next() {
        let mut val = || a.next().with_context(|| format!("{x} needs a value"));
        match x.as_str() {
            "--state-dir" => dir = Some(PathBuf::from(val()?)),
            "--hostname" => hostname = val()?,
            "--authkey-file" => authkey = Some(PathBuf::from(val()?)),
            "--port" => port = val()?.parse().context("--port")?,
            "--ephemeral" => ephemeral = true,
            "--delegate-key" => delegate = Some(PathBuf::from(val()?)),
            "--poll-secs" => poll = val()?.parse().context("--poll-secs")?,
            "--account" => account = Some(val()?),
            "--kind" => kind = val()?,
            "--minutes" => minutes = val()?.parse().context("--minutes")?,
            "--version" => return Ok(None),
            "--help" => {
                print!("{HELP}");
                std::process::exit(0)
            }
            "enroll-code" | "devices" | "revoke" if sub.is_none() => sub = Some(x),
            s if sub.as_deref() == Some("revoke") && serial.is_none() => serial = Some(s.parse::<u64>().context("a serial is a number")?),
            _ => bail!("unknown argument {x}\n{USAGE}"),
        }
    }
    let dir = dir.with_context(|| format!("--state-dir is required\n{USAGE}"))?;
    let cmd = match sub.as_deref() {
        None => Command::Serve { hostname, authkey, port, ephemeral, delegate, poll },
        Some("enroll-code") => Command::EnrollCode { account: account.context("enroll-code needs --account")?, kind, minutes },
        Some("devices") => Command::Devices,
        _ => Command::Revoke(serial.context("revoke needs a certificate's serial (see `devices`)")?),
    };
    Ok(Some((dir, cmd)))
}

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<()> {
    let Some((dir, cmd)) = args()? else {
        println!("nitya-server {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    };
    let ca = ca::Ca::open(&dir)?;
    match cmd {
        Command::EnrollCode { account, kind, minutes } => {
            let code = ca.new_code(&account, &kind, minutes * 60, dev::now())?;
            eprintln!("a one-time code for one {kind} of {account}, valid {minutes} minutes:");
            println!("{code}");
            Ok(())
        }
        Command::Devices => {
            for (c, revoked) in ca.devices()? {
                println!(
                    "{}\t{}\t{}\t{}\tserial {}\tuntil {}{}",
                    c.account,
                    c.kind,
                    c.hostname,
                    c.node_id,
                    c.serial,
                    c.not_after,
                    if revoked { "\trevoked" } else { "" }
                );
            }
            Ok(())
        }
        Command::Revoke(serial) => {
            let (c, had_wrap) = ca.revoke(serial, dev::now())?;
            println!("revoked {} of {} ({}): it can no longer call, renew or enroll", c.hostname, c.account, c.kind);
            if had_wrap {
                println!("its wrap of the account's keys is deleted; the next device of {} to unlock rotates them", c.account);
            }
            Ok(())
        }
        Command::Serve { hostname, authkey, port, ephemeral, delegate, poll } => {
            let tailnet = Tailnet::start(Options {
                name: "nitya".into(),
                dir: dir.join("tailnet"),
                hostname: hostname.clone(),
                authkey: authkey.map(AuthKey::File),
                ephemeral,
                give_up_after: None,
            })
            .await?;
            // Sync uses it to seal and open consented classes. Provided (the NixOS module
            // passes an agenix secret as a credential, ADR-0016), or made beside the store.
            let delegate = match delegate {
                Some(p) => nitya_keys::Delegate::load(&p)?,
                None => nitya_keys::Delegate::load_or_generate(&dir.join("delegate.key"))?,
            };
            // Agent grants come with phase 5; until then agents are refused everything.
            let policy = Arc::new(policy::Policy::new(&[])?);
            eprintln!("nitya-server: on tailnet as {hostname} ({}), port {port}", tailnet.ips());
            let ca = Arc::new(ca);
            let delegate_key = delegate.public();
            // No provider drivers yet: they come with phase 2's items 2 to 4.
            let engine = Arc::new(sync::Engine::new(ca.clone(), Arc::new(delegate), Default::default(), poll));
            engine.clone().spawn();
            tonic::transport::Server::builder()
                .add_service(DevicesServer::new(devices::Service { ca: ca.clone() }))
                .add_service(KeysServer::new(keys::Service { ca: ca.clone(), delegate_key, policy: policy.clone() }))
                .add_service(AuditServer::new(audit::Service { ca: ca.clone(), policy: policy.clone() }))
                .add_service(ChannelsServer::new(channels::Service { ca, policy, engine }))
                .serve_with_incoming(tailnet.incoming(port)?)
                .await
                .context("serving on the tailnet")
        }
    }
}
