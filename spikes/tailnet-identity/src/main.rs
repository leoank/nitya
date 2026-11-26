//! EXP-20261126-hvzed5: what the server learns about a caller from its tailnet connection.
//!
//!   tailnet-identity server DIR HOSTNAME AUTHKEY_FILE PORT   # one JSON line per caller
//!   tailnet-identity node DIR HOSTNAME AUTHKEY_FILE HOST:PORT
//!
//! Both join as ephemeral nodes.

use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use nitya_tailnet::{AuthKey, Options, Tailnet};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_stream::StreamExt;

async fn join(dir: &str, hostname: &str, key: &str) -> Result<std::sync::Arc<Tailnet>> {
    Tailnet::start(Options {
        name: "idtest".into(),
        dir: PathBuf::from(dir),
        hostname: hostname.into(),
        authkey: Some(AuthKey::File(PathBuf::from(key))),
        ephemeral: true,
        give_up_after: Some(std::time::Duration::from_secs(90)),
    })
    .await
}

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<()> {
    let a: Vec<String> = std::env::args().skip(1).collect();
    match a.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        ["server", dir, host, key, port] => {
            let t = join(dir, host, key).await?;
            eprintln!("server up as {host} ({})", t.ips());
            let mut incoming = t.incoming(port.parse()?)?;
            while let Some(conn) = incoming.next().await {
                let mut conn = conn?;
                let addr = conn.remote().addr.clone();
                let p = conn.remote().peer().await;
                let line = serde_json::json!({
                    "addr": addr,
                    "host": p.as_ref().map(|p| p.host.clone()),
                    "login": p.as_ref().map(|p| p.login.clone()),
                    "node_id": p.as_ref().map(|p| p.node_id.clone()),
                    "node_key": p.as_ref().map(|p| p.node_key.clone()),
                    "dns_name": p.as_ref().map(|p| p.dns_name.clone()),
                });
                println!("{line}");
                conn.write_all(b"ok\n").await?;
                conn.shutdown().await?;
            }
            bail!("stopped accepting")
        }
        ["node", dir, host, key, server] => {
            let t = join(dir, host, key).await?;
            let mut c = t.dial(server).await.with_context(|| format!("dialling {server}"))?;
            let mut s = String::new();
            c.read_to_string(&mut s).await?;
            eprintln!("node {host} ({}): server said {}", t.ips(), s.trim());
            Ok(())
        }
        _ => bail!("usage: tailnet-identity server DIR HOSTNAME AUTHKEY PORT | node DIR HOSTNAME AUTHKEY HOST:PORT"),
    }
}
