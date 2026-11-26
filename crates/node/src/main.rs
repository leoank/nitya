//! The nitya node (ADR-0001): its own tsnet identity on each machine, whatever tailnet the
//! machine itself is on, and a device key with a certificate from the server (ADR-0011).
//!
//!   nitya-node --state-dir DIR --hostname NAME [--authkey-file FILE] [--server nitya:7300]
//!              [--ephemeral] enroll CODE | whoami
//!
//! DIR holds the tailnet identity (tailnet/), the device key (device.key), its certificate
//! (device.cert) and the server's CA key (ca.pub). `whoami` renews the certificate first
//! once half its life has passed.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use ed25519_dalek::SigningKey;
use nitya_device as dev;
use nitya_proto::v1::devices_client::DevicesClient;
use nitya_proto::v1::{Cert, CertBody, EnrollRequest, RenewRequest, WhoAmIRequest};
use nitya_tailnet::{AuthKey, Options, Tailnet};
use prost::Message;
use tonic::transport::Channel;

const USAGE: &str = "usage: nitya-node --state-dir DIR --hostname NAME [--authkey-file F] [--server HOST:PORT] [--ephemeral] enroll CODE | whoami";

enum Command {
    Enroll(String),
    WhoAmI,
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

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<()> {
    let Some(a) = args()? else {
        println!("nitya-node {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    };
    std::fs::create_dir_all(&a.dir)?;
    let device = dev::load_or_generate(&a.dir.join("device.key"))?;
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
    let mut client = DevicesClient::new(channel);

    match a.cmd {
        Command::Enroll(code) => {
            let proof = dev::enroll_proof(&device, code.trim());
            let r = client
                .enroll(EnrollRequest { code: code.trim().into(), device_key: device.verifying_key().to_bytes().to_vec(), proof })
                .await
                .context("enrolling")?
                .into_inner();
            dev::write_private(&a.dir.join("ca.pub"), &r.ca_key)?;
            let body = save_cert(&a.dir, &r.cert.context("no certificate in the answer")?, &device)?;
            println!("enrolled as a {} of {}, serial {}, until {}", body.kind, body.account, body.serial, body.not_after);
        }
        Command::WhoAmI => {
            let (mut cert, body) = load_cert(&a.dir, &device)?;
            if dev::now() > body.not_after - dev::CERT_LIFETIME_SECS / 2 {
                let mut req = tonic::Request::new(RenewRequest {});
                dev::sign_request(&mut req, &device, &cert, "nitya.v1.Devices/Renew", dev::now());
                cert = client.renew(req).await.context("renewing the certificate")?.into_inner();
                let b = save_cert(&a.dir, &cert, &device)?;
                eprintln!("nitya-node: renewed, serial {} until {}", b.serial, b.not_after);
            }
            let mut req = tonic::Request::new(WhoAmIRequest {});
            dev::sign_request(&mut req, &device, &cert, "nitya.v1.Devices/WhoAmI", dev::now());
            let w = client.who_am_i(req).await.context("asking the server who this node is")?.into_inner();
            println!("{} of {}: {} on node {}, serial {}, until {}", w.kind, w.account, w.hostname, w.node_id, w.serial, w.not_after);
        }
    }
    Ok(())
}
