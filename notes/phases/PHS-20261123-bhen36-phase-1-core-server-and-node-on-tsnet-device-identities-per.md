---
id: PHS-20261123-bhen36
type: phase
schema_version: 1
title: "Phase 1: core: server and node on tsnet, device identities, per-user keys, audit log, RBAC"
project: PRJ-0001
status: active
since: 95c7efc514814093bfc0ce7db7adf271ab4c435b
until: null               # the commit it finished at: yajna phase done
created: 2026-11-23
author: agent:claude
tags: []
---

## Goal
The secure skeleton everything else runs in: the server and the node on their own tailnet
identities, devices enrolled with certificates, the key hierarchy, Cedar policies and the
audit log (ADR-0001, ADR-0005, ADR-0006, ADR-0007). Done when a node and the app enroll, a
revoked device is refused, and every request appears in a chain a device can verify.

## Progress
One item per piece of work, ticked when it lands; add items as the plan grows and keep the
done ones. The project page's *Progress* lists this phase on one line.

- [x] 1. The tailnet crate from saarthi, copied as sandhi did; server and node each with a tsnet identity
- [x] 2. Enrollment and the device CA: Ed25519 device keys, short-lived certificates, signed requests, revocation ([[EXP-20261126-hvzed5]], ADR-0011); the app's enrollment, keys in a TPM or the Android Keystore, and revoking from a device come later
- [x] 3. Accounts and the key hierarchy as [[EXP-20261123-bhfzeh]] left it; the sync delegate with consent; ADR-0013: one key set per account written whole by its devices, new devices approved by fingerprint, rotation by the next device to unlock after a revocation; the delegate's audit entries come with item 6
- [x] 4. The store: encrypted items, channels and cursors (ADR-0003), migrations; SQLite (ADR-0012), done before item 3 at the owner's choice; items are stored as sealed bytes, the sealing comes with item 3
- [x] 5. Cedar policies and schema for users, devices and agents, validated in CI; ADR-0014: devices act as their user, agents hold only grants (a linked template each); the Keys service asks Cedar; grants are stored and linked in phase 5
- [ ] 6. The hash-chained, signed audit log with device checkpoints
- [ ] 7. NixOS module for the server and home-manager module for the node, with module tests; deployed on ghost001

## Notes

- 2026-11-26: item 4 before item 3, on the owner's choice (ADR-0012): the key hierarchy's
  wraps and data keys need the store, and the CA's state moved into it from files.
