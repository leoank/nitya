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
- [ ] 2. Enrollment and the device CA: Ed25519 device keys, short-lived certificates, signed requests, revocation
- [ ] 3. Accounts and the key hierarchy as [[EXP-20261123-bhfzeh]] left it; the sync delegate with consent
- [ ] 4. The store: encrypted items, channels and cursors (ADR-0003), migrations
- [ ] 5. Cedar policies and schema for users, devices and agents, validated in CI
- [ ] 6. The hash-chained, signed audit log with device checkpoints
- [ ] 7. NixOS module for the server and home-manager module for the node, with module tests; deployed on ghost001

## Notes
