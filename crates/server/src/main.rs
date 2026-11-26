//! The nitya server (ADR-0001): its own tsnet identity, serving nitya's gRPC API on the
//! tailnet (ADR-0011), and the device CA's admin commands.
//!
//!   nitya-server --state-dir DIR [--hostname nitya] [--authkey-file FILE] [--port 7300]
//!                [--ephemeral]                                   serve
//!   nitya-server --state-dir DIR enroll-code --account A [--kind node|app|agent] [--minutes 15]
//!   nitya-server --state-dir DIR devices
//!   nitya-server --state-dir DIR revoke SERIAL
//!
//! The tailnet identity lives in DIR/tailnet, the CA in DIR/ca.

mod ca;
mod devices;

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use nitya_device as dev;
use nitya_proto::v1::devices_server::DevicesServer;
use nitya_tailnet::{AuthKey, Options, Tailnet};

/// The port nitya's server listens on, on its tailnet identity.
pub const PORT: u16 = 7300;

const USAGE: &str = "usage: nitya-server --state-dir DIR [--hostname H] [--authkey-file F] [--port P] [--ephemeral]\n\
       nitya-server --state-dir DIR enroll-code --account A [--kind node|app|agent] [--minutes M]\n\
       nitya-server --state-dir DIR devices\n\
       nitya-server --state-dir DIR revoke SERIAL";

enum Command {
    Serve { hostname: String, authkey: Option<PathBuf>, port: u16, ephemeral: bool },
    EnrollCode { account: String, kind: String, minutes: i64 },
    Devices,
    Revoke(u64),
}

fn args() -> Result<Option<(PathBuf, Command)>> {
    let mut a = std::env::args().skip(1);
    let (mut dir, mut hostname, mut authkey, mut port, mut ephemeral) = (None, "nitya".to_string(), None, PORT, false);
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
            "--account" => account = Some(val()?),
            "--kind" => kind = val()?,
            "--minutes" => minutes = val()?.parse().context("--minutes")?,
            "--version" => return Ok(None),
            "enroll-code" | "devices" | "revoke" if sub.is_none() => sub = Some(x),
            s if sub.as_deref() == Some("revoke") && serial.is_none() => serial = Some(s.parse::<u64>().context("a serial is a number")?),
            _ => bail!("unknown argument {x}\n{USAGE}"),
        }
    }
    let dir = dir.with_context(|| format!("--state-dir is required\n{USAGE}"))?;
    let cmd = match sub.as_deref() {
        None => Command::Serve { hostname, authkey, port, ephemeral },
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
    let ca = ca::Ca::open(&dir.join("ca"))?;
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
            let c = ca.revoke(serial)?;
            println!("revoked {} of {} ({}): it can no longer call, renew or enroll", c.hostname, c.account, c.kind);
            Ok(())
        }
        Command::Serve { hostname, authkey, port, ephemeral } => {
            let tailnet = Tailnet::start(Options {
                name: "nitya".into(),
                dir: dir.join("tailnet"),
                hostname: hostname.clone(),
                authkey: authkey.map(AuthKey::File),
                ephemeral,
                give_up_after: None,
            })
            .await?;
            eprintln!("nitya-server: on tailnet as {hostname} ({}), port {port}", tailnet.ips());
            tonic::transport::Server::builder()
                .add_service(DevicesServer::new(devices::Service { ca: Arc::new(ca) }))
                .serve_with_incoming(tailnet.incoming(port)?)
                .await
                .context("serving on the tailnet")
        }
    }
}
