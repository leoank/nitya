# Running nitya

nitya is a server and a node per machine, each on its own identity on your tailnet. Until
the NixOS and home-manager modules exist, you run them by hand from the flake. This page
gets a server up and a device enrolled. The reference lists every option for
[`nitya-server`](../reference/nitya-server.md) and [`nitya-node`](../reference/nitya-node.md).

## The server

```sh
nix build .#nitya-server .#nitya-node
./result/bin/nitya-server --state-dir ~/nitya-server --authkey-file ~/tailnet-authkey
```

It joins the tailnet as `nitya` (`--hostname` to change it) and serves its API on port
7300 of that identity, nowhere else. The auth key is needed only for the first join; the
identity is kept in `tailnet/` under the state directory.

The state directory, made on first use and readable only by its owner, holds:

| File | What it is |
|------|------------|
| `nitya.db` | the store: accounts, certificates, key sets, the audit log, channels and items |
| `ca.key` | the device CA's signing key |
| `audit.key` | the audit log's signing key |
| `delegate.key` | the sync-delegate key ([Your keys](keys.md#the-sync-delegate)) |
| `tailnet/` | the server's tailnet identity |

Keys are files beside the store, never in it. The admin commands below open the same
directory and work while the server runs.

## Enrolling a device

Every node, app install and agent job is a device with its own key and a short-lived
certificate from the server. To enroll one, make a one-time code on the server:

```sh
nitya-server --state-dir ~/nitya-server enroll-code --account ank --kind node
```

It prints a code valid for 15 minutes (`--minutes`), for one device of that account. On
the machine to enroll:

```sh
nitya-node --state-dir ~/.local/state/nitya-node --hostname nitya-node-rogue enroll CODE
nitya-node --state-dir ~/.local/state/nitya-node --hostname nitya-node-rogue whoami
```

`enroll` makes the device's keys, joins the tailnet as `nitya-node-rogue`, and gets a
certificate bound to this machine's tailnet node: a copy of the device's key and
certificate on another machine is refused. A code works once. Certificates last 24 hours;
the node renews its own once half has passed, and may do so up to 30 days after one ran
out, after which it must enroll again.

## Devices and revocation

```sh
nitya-server --state-dir ~/nitya-server devices
nitya-server --state-dir ~/nitya-server revoke SERIAL
```

`devices` lists each device's latest certificate: account, kind, hostname, tailnet node,
serial, expiry and whether it is revoked. `revoke` refuses that device everything from
then on (calls, renewal, enrolling again with the same key) and deletes its copy of the
account's keys; the account's next device to unlock rotates them
([Your keys](keys.md#revoking-a-device)).

Every enrollment, renewal, code and revocation is in the account's
[audit log](audit.md).
