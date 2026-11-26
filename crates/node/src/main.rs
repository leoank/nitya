//! The nitya node (ADR-0001): its own tsnet identity on each machine, whatever tailnet the
//! machine itself is on.
//!
//! Phase 1 item 1 only: it joins the tailnet, dials the server and prints its answer, the
//! placeholder of nitya's API until that is chosen.
//!
//!   nitya-node --tailnet-dir DIR --hostname NAME [--authkey-file FILE]
//!              [--server nitya:7300] [--ephemeral]

use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use nitya_tailnet::{AuthKey, Options, Tailnet};
use tokio::io::AsyncReadExt;

struct Args {
    dir: PathBuf,
    hostname: String,
    authkey: Option<PathBuf>,
    server: String,
    ephemeral: bool,
}

fn args() -> Result<Option<Args>> {
    let mut a = std::env::args().skip(1);
    let (mut dir, mut hostname, mut authkey, mut server, mut ephemeral) = (None, None, None, "nitya:7300".to_string(), false);
    while let Some(x) = a.next() {
        let mut val = || a.next().with_context(|| format!("{x} needs a value"));
        match x.as_str() {
            "--tailnet-dir" => dir = Some(PathBuf::from(val()?)),
            "--hostname" => hostname = Some(val()?),
            "--authkey-file" => authkey = Some(PathBuf::from(val()?)),
            "--server" => server = val()?,
            "--ephemeral" => ephemeral = true,
            "--version" => return Ok(None),
            _ => bail!("unknown argument {x}; usage: nitya-node --tailnet-dir DIR --hostname NAME [--authkey-file F] [--server HOST:PORT] [--ephemeral]"),
        }
    }
    Ok(Some(Args {
        dir: dir.context("--tailnet-dir is required")?,
        hostname: hostname.context("--hostname is required (one per machine, e.g. nitya-node-rogue)")?,
        authkey,
        server,
        ephemeral,
    }))
}

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<()> {
    let Some(a) = args()? else {
        println!("nitya-node {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    };
    let tailnet = Tailnet::start(Options {
        name: "nitya".into(),
        dir: a.dir,
        hostname: a.hostname,
        authkey: a.authkey.map(AuthKey::File),
        ephemeral: a.ephemeral,
        give_up_after: Some(std::time::Duration::from_secs(60)),
    })
    .await?;
    let mut conn = tailnet.dial(&a.server).await.with_context(|| format!("dialling {} on the tailnet", a.server))?;
    let mut answer = String::new();
    conn.read_to_string(&mut answer).await.context("reading the server's answer")?;
    print!("{answer}");
    Ok(())
}
