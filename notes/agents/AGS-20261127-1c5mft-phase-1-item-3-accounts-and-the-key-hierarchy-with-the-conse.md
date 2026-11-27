---
id: AGS-20261127-1c5mft
type: agent_session
schema_version: 1
title: "phase 1 item 3: accounts and the key hierarchy with the consented sync delegate"
model: "claude-opus-5-5"
harness: "claude-code"
started: 2026-11-27T01:15+00:00
finished: 2026-11-27T01:30+00:00
created_entities: []
edited_entities: [PHS-20261123-bhen36]
outcome: done             # done | failed | aborted | pending
created: 2026-11-27
author: agent:claude
---

## Task

"Commit and push, then move to the next item": phase 1 item 3, accounts and the key
hierarchy as [[EXP-20261123-bhfzeh]] left it with ADR-0010, the per-device X25519 wrap
key the owner chose, and the sync delegate with consent.

## What happened

- Asked the owner three questions first; chosen: a new device gets UK only by approval
  on an unlocked device after comparing its fingerprint; after a revocation the account
  is flagged and the next device to unlock rotates; the recovery passphrase is required.
  Recorded with the rest as ADR-0013.
- Decided without asking, for the owner to check in ADR-0013:
  - the account's keys are one protobuf key set, written whole by devices with a
    compare-and-swap on its revision, not tables per wrap;
  - recovery is an age X25519 key sealed under Argon2id of the passphrase, so a rotation
    can wrap the new UK to it without asking for the passphrase;
  - tags under UK over each device's wrap key and over the recovery key, checked at every
    unlock, so the server cannot slip in a key of its own;
  - the wrap key goes into the enrollment request, its proof and the certificate;
  - the delegate key is a file beside the store until item 7's systemd credential.
- `nitya-keys`: create, unlock, recover, approve, consent, withdraw, rotate, sealed items
  and the server's delegate; 6 tests repeat the spike's scenarios (offline delegate,
  append-only classes, rotation shutting a revoked device out of new data, recovery) and
  add swapped keys being caught.
- Server: the `Keys` service (Get, Put) with its write checks (2 tests), revocation
  deleting the wrap and flagging the account (1 test), `key_sets` in the store
  (migration 2, 1 test).
- Node: `keys fingerprint | init | status | approve | recover | consent | withdraw`.
- On the leoank tailnet with three nodes: init; a second init refused; approval refused
  on a wrong fingerprint and done on the right one; consent to calendar; after revoking
  b the next command on a rotated to UK version 2 before consenting to mail; b refused;
  c recovered with the passphrase (a wrong one refused) and holds the keys.
