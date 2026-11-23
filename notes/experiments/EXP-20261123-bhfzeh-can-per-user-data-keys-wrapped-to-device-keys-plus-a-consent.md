---
id: EXP-20261123-bhfzeh
type: experiment
schema_version: 1
title: Can per-user data keys wrapped to device keys, plus a consented sync-delegate wrap, let the nitya server sync and run jobs while the user is offline, with every unwrap in the signed audit log and device revocation losing nothing?
project: PRJ-0001
status: planned           # planned | active | paused | concluded | abandoned
outcome: null             # null | positive | negative | inconclusive | infra
influenced_by: []
created: 2026-11-23
author: agent:claude
protocols: []
materials: []
tags: []
review: pending
---

## Question
Does the key hierarchy of ADR-0005 work as designed: can per-user data keys, wrapped to
device keys and to a consented sync-delegate key, let the server sync and run agent jobs
while the owner is offline, with every unwrap in the signed audit log (ADR-0007), and can a
device be revoked without losing data or leaving it able to read new data?

## Hypothesis and predictions
H1: a hierarchy of user key, data keys per class and device wraps, with a delegate wrap
per class, gives all three properties at a cost small enough to ignore.

1. With the delegate unlocked for calendar only, the server syncs calendar items while no
   device is online, and cannot decrypt mail items (append-only: new mail sealed to the
   account's public key is stored but unreadable to it).
2. Every delegate unwrap and every decryption it serves appears in the hash-chained log,
   and a modified or deleted entry is detected by a device's checkpoint.
3. Revoking a device (delete its wrap, rotate UK, re-wrap DEKs) takes under a second for
   one account, loses nothing, and the revoked device's saved wrap opens nothing new.
4. Encrypting and decrypting an item adds under 1 ms per item on ghost001.

If 1 fails, background agent jobs need the owner online, and the phase plan changes.

## Plan
1. A Rust spike: X25519 device keys with age (@rage) or HPKE wraps, XChaCha20-Poly1305
   items with associated data (account, class, item id), an Argon2id recovery wrap, a
   delegate key from systemd credentials (or a file in the spike), and a minimal
   hash-chained log signed with Ed25519.
2. Scenarios as tests: offline sync with a calendar-only delegate; mail append while
   delegate-less; revocation and rotation; tampering with the log; recovery from the
   passphrase after every device is lost.
3. Time encryption and revocation over 100k synthetic items.
4. Review the threat model against ADR-0005's consequences with the owner.

## Success criteria
- All five scenarios pass as tests; the timings are within the predictions.
- The ADR-0005 design either stands or is superseded with what the spike showed.
- A written list of what the server admin can and cannot read in each state.

## Runs
One line per run after its link, what was tried and what it showed; `yajna validate`
rejects a bare link here. Metric runs also land in results.tsv.

## Conclusion
Written once, by a human, when status becomes `concluded`.
