# ADR-0005: Device keys with short-lived certificates, a per-user key hierarchy, and a consented sync delegate; no MLS

Status: accepted, amended by ADR-0010 (revocation)
Date: 2026-11-23

## Context
The owner requires, in phase 1, encryption at rest, per-user keys the server admin cannot
use for another account, mutual authentication per device, and a signed audit log. The
server must still sync and run agent jobs while the owner is offline. Wire's core-crypto
(@wire-corecrypto) keeps a SQLCipher keystore keyed from outside the store, gives each
device its own signature key and an X.509 certificate through DPoP and OIDC challenges,
and lets a server read only through legal hold, a consented, visible, removable virtual
device (@wire-legalhold). Stalwart (@stalwart) encrypts mail on arrival to the user's
public key so the server can append what it cannot read. cal.diy stores tokens with
encryption that fails open and CBC without authentication.

Alternatives considered: MLS (built for group messaging among parties who distrust the
server; nitya's server must read the data, so it adds epochs and commits for nothing);
shared bearer tokens per client, as saarthi uses (no per-device revocation or signing);
server-wide encryption with one key (the admin reads every account); linking Wire's code
(GPL-3.0-only).

## Decision
- **Device identity.** Every node, app install and agent job has its own Ed25519 key, in the
  Android Keystore or a TPM where there is one. nitya's server is a small CA issuing
  short-lived device certificates at enrollment, bound to the device's tailnet identity.
  Requests are signed (HTTP message signatures or a DPoP-style proof); revocation is a deny
  list plus short lifetimes.
- **Key hierarchy.** Each account has a user key (UK); each data class (mail, calendar,
  tasks, provider tokens, files) has a data key (DEK) wrapped by UK; UK is wrapped to each
  device's X25519 key (age or HPKE, e.g. @rage) and to a recovery passphrase (Argon2id).
  Revoking a device deletes its wrap and rotates UK; only DEKs are re-wrapped.
- **Sync delegate.** Background sync and agent jobs need some DEKs on the server. The owner
  consents per data class to a sync-delegate wrap sealed to a server key, unlocked at
  service start (TPM or systemd credentials), shown in the device list, removable, and its
  unlock and every use written to the audit log. New data can be sealed to the account's
  public key on arrival, so classes without a delegate are append-only for the server.
- **At rest.** Records under AEAD (XChaCha20-Poly1305 or AES-GCM) with versioned keys and
  associated data binding each to its account and record; keys never on disk beside the
  data; missing keys fail closed; secrets in memory zeroized.

## Consequences
- The hierarchy and the delegate are tested before phase 1 builds on them
  ([[EXP-20261123-bhfzeh]]).
- Losing every device and the recovery passphrase loses the account's data, by design.
- An admin who controls the server process while a delegate is unlocked can read the
  delegated classes; the audit log makes that visible, it does not prevent it.
