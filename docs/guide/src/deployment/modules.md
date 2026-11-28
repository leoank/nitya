# The server and node modules

nitya's flake exports two modules: `nixosModules.server` runs the server on a NixOS
machine, and `homeModules.node` sets up the node in each user's home on Linux and macOS.
This page shows them wired into a machine flake as neusis does. The secrets are agenix
files there.

## The server

```nix
{ inputs, config, ... }:
{
  imports = [ inputs.nitya.nixosModules.server ];

  services.nitya-server = {
    enable = true;
    # hostname = "nitya"; port = 7300;      # its name and port on the tailnet
    authKeyFile = config.age.secrets.nityaServerTsAuthKey.path;
    delegateKeyFile = config.age.secrets.nityaDelegateKey.path;
    googleClientFile = config.age.secrets.nityaGoogleClient.path;
    # pollSeconds = 60;                     # how often each channel syncs
  };
  age.secrets.nityaServerTsAuthKey.rekeyFile = ./secrets/tailnet-authkey.age;
  age.secrets.nityaDelegateKey.rekeyFile = ./secrets/nityaDelegateKey.age;
  age.secrets.nityaGoogleClient.rekeyFile = ./secrets/nityaGoogleClient.age;
}
```

- The server runs as the system user `nitya`, with its state in `/var/lib/nitya`
  (mode 0700): the tailnet identity, the store, the CA and audit keys
  ([Running nitya](../using/running.md#the-server)). It listens on its tailnet identity
  only and opens no port on the host.
- **The tailnet auth key** is needed for the first join, and again if the tailnet logs the
  identity out.
- **The delegate key** opens the classes you let the server sync while you are offline
  ([Your keys](../using/keys.md#the-sync-delegate)). Make it once with `age-keygen` and
  keep it as an agenix secret, so it survives a rebuild of the machine:

  ```sh
  # agenix-rekey's `edit -i` wants a regular file: make the key in a private temporary
  # directory and remove it once it is encrypted.
  d=$(mktemp -d) && age-keygen -o "$d/key" && agenix edit -i "$d/key" secrets/nityaDelegateKey.age
  rm -rf "$d"
  agenix rekey
  ```

  Without `delegateKeyFile`, the server makes the key in its state directory.
- **The Google client** is the Desktop OAuth client's JSON from
  [Provider sign-in apps](provider-apps.md). With it the server syncs Google calendars,
  and your nodes sign Google accounts in ([Channels and sync](../using/sync.md#connecting-a-google-account)).
  The accounts' tokens are kept in the server's store, not in agenix.
- The secrets reach the service as systemd credentials. The service user can't read
  them where agenix keeps them, and their paths never appear on its command line.

Admin commands run as the service user through sudo:

```sh
nitya-admin enroll-code --account ank
nitya-admin devices
nitya-admin revoke SERIAL
```

## The node

```nix
{ inputs, config, ... }:
{
  imports = [ inputs.nitya.homeModules.node ];

  services.nitya-node = {
    enable = true;
    hostname = "nitya-node-rogue";           # one per machine
    # server = "nitya:7300";
    authKeyFile = config.age.secrets.nityaNodeTsAuthKey.path;
  };
  age.secrets.nityaNodeTsAuthKey = {
    rekeyFile = ./secrets/tailnet-authkey.age;
    path = "${config.home.homeDirectory}/.config/nitya/tailnet-authkey"; # fixed, for launchd
  };
}
```

This puts `nitya-node` on your PATH with the machine's settings filled in, so the commands
in [Running nitya](../using/running.md) need only the command itself:

```sh
nitya-node enroll CODE          # a code from `nitya-admin enroll-code` on the server
nitya-node keys status
```

Every hour a user service checks the account's [audit log](../using/audit.md) against
this device's checkpoint. It is a systemd timer on Linux and a launchd agent on macOS,
logging to `~/Library/Logs/nitya-node-audit.log`. The same check renews the device's
certificate, so a machine that sits idle doesn't fall out of enrollment. Until the node
is enrolled, the check fails and says so in its log. `check.enable = false` turns it off.

A failed check is only written to that log for now; nothing alerts you yet.
