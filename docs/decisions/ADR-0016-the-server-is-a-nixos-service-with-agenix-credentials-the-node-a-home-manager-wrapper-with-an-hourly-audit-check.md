# ADR-0016: The server is a NixOS service fed agenix secrets as credentials; the node is a home-manager wrapper with an hourly audit check

Status: accepted (2026-11-27; the owner chose the delegate key as an agenix secret and the hourly check; the module details settled by agent:claude for the owner to check)
Date: 2026-11-27

## Context
Phase 1 ends with nitya deployed by modules from this flake: the server on ghost001,
the node in each user's home, as saarthi's hub and node are. ADR-0013 left the server's
sync-delegate key in a file beside the store "until the NixOS module passes it as a
systemd credential". Asked on 2026-11-27 ([[AGS-20261127-crjc71]]), the owner chose:
- the delegate key as an agenix secret in neusis, given to the unit as a systemd
  credential;
- a node module that checks the audit log every hour;
- the modules here, wired into neusis by the agent, committed and deployed by the owner.

Alternatives considered:
- systemd-creds (`LoadCredentialEncrypted`): on a host without a TPM it is encrypted
  under `/var/lib/systemd/credential.secret`, on the same disk, so it protects against
  the same things agenix does (a copied state directory or backup) and not against a
  copy of the whole disk, while losing the key with the machine. With a TPM it would be
  stronger; ghost001 is not known to have one.
- The delegate key as a file the server makes: nothing to provision, but it sits beside
  the data it opens.
- A node module with only the configured command: a rewritten log would go unnoticed
  until someone runs a check, and an unused certificate expires for good after 30 days.

## Decision
- **`nixosModules.server`** (`services.nitya-server`): a system user `nitya`, state in
  `/var/lib/nitya` (0700), the server on its own tailnet identity with no port on the
  host. The tailnet auth key and the delegate key are `LoadCredential`s, so the paths of
  the secrets never reach the command line and the service user cannot read them where
  agenix keeps them. `nitya-admin` runs the admin commands as `nitya` through sudo.
  The server takes `--delegate-key FILE` (an `age-keygen` identity) and makes no key of
  its own when given one.
- **`homeModules.node`** (`services.nitya-node`): `nitya-node` on PATH with the
  machine's state directory, hostname, server and auth key given; every hour a systemd
  user timer or launchd agent runs `nitya-node audit verify`, which also renews the
  certificate.
- **Tests**: `checks.modules` evaluates the server in a NixOS system and the node, on
  Linux and Darwin, against stubs of home-manager's options, as saarthi's module tests
  do.
- The CA and audit keys stay files in `/var/lib/nitya`, made by the server.

## Consequences
- ADR-0013's delegate key is provided by neusis on ghost001; a lost machine keeps it.
- A failed hourly check is written to the user service's log; nothing tells the owner
  yet. Notifications are the node's job in a later phase.
- An interactive `nitya-node` command and the hourly check started at the same moment
  share the node's tailnet state; a collision fails one of them, and the next hour
  retries.
