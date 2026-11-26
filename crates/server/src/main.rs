//! The nitya server (ADR-0001): its own tsnet identity, serving on the tailnet.
//!
//! Phase 1 item 1 only: it joins the tailnet and answers each connection with who the
//! tailnet says the caller is, the placeholder of nitya's API until that is chosen.
//!
//!   nitya-server --tailnet-dir DIR [--hostname nitya] [--authkey-file FILE] [--port 7300]
//!                [--ephemeral]

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use nitya_tailnet::{AuthKey, Options, Tailnet};
use tokio::io::AsyncWriteExt;
use tokio_stream::StreamExt;

/// The port nitya's server listens on, on its tailnet identity.
pub const PORT: u16 = 7300;

struct Args {
    dir: PathBuf,
    hostname: String,
    authkey: Option<PathBuf>,
    port: u16,
    ephemeral: bool,
}

fn args() -> Result<Option<Args>> {
    let mut a = std::env::args().skip(1);
    let (mut dir, mut hostname, mut authkey, mut port, mut ephemeral) = (None, "nitya".to_string(), None, PORT, false);
    while let Some(x) = a.next() {
        let mut val = || a.next().with_context(|| format!("{x} needs a value"));
        match x.as_str() {
            "--tailnet-dir" => dir = Some(PathBuf::from(val()?)),
            "--hostname" => hostname = val()?,
            "--authkey-file" => authkey = Some(PathBuf::from(val()?)),
            "--port" => port = val()?.parse().context("--port")?,
            "--ephemeral" => ephemeral = true,
            "--version" => return Ok(None),
            _ => bail!("unknown argument {x}; usage: nitya-server --tailnet-dir DIR [--hostname H] [--authkey-file F] [--port P] [--ephemeral]"),
        }
    }
    Ok(Some(Args { dir: dir.context("--tailnet-dir is required")?, hostname, authkey, port, ephemeral }))
}

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<()> {
    let Some(a) = args()? else {
        println!("nitya-server {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    };
    let hostname = a.hostname.clone();
    let tailnet = Tailnet::start(Options {
        name: "nitya".into(),
        dir: a.dir,
        hostname: a.hostname,
        authkey: a.authkey.map(AuthKey::File),
        ephemeral: a.ephemeral,
        give_up_after: None,
    })
    .await?;
    eprintln!("nitya-server: on tailnet as {hostname} ({}), port {}", tailnet.ips(), a.port);
    serve(tailnet, a.port).await
}

async fn serve(tailnet: Arc<Tailnet>, port: u16) -> Result<()> {
    let mut incoming = tailnet.incoming(port)?;
    while let Some(conn) = incoming.next().await {
        let mut conn = conn.context("accepting on the tailnet")?;
        tokio::spawn(async move {
            let who = match conn.remote().peer().await {
                Some(p) => p.to_string(),
                None => format!("an unknown peer at {}", conn.remote().addr),
            };
            eprintln!("nitya-server: connection from {who}");
            let _ = conn.write_all(format!("nitya-server {}; you are {who}\n", env!("CARGO_PKG_VERSION")).as_bytes()).await;
            let _ = conn.shutdown().await;
        });
    }
    bail!("the tailnet stopped accepting connections")
}
