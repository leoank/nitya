---
id: EXP-20261123-bhfzeh
type: experiment
schema_version: 1
title: Can per-user data keys wrapped to device keys, plus a consented sync-delegate wrap, let the nitya server sync and run jobs while the user is offline, with every unwrap in the signed audit log and device revocation losing nothing?
project: PRJ-0001
status: concluded         # planned | active | paused | concluded | abandoned
outcome: positive         # null | positive | negative | inconclusive | infra
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
- [[RUN-20261126-fexqm6]] the spike: offline sync with a calendar-only delegate, the signed log
  with checkpoints, recovery and timings (~6 µs an item, ~70 ms a revocation) hold;
  revocation by re-wrapping only leaves new data readable to the revoked device, rotating
  the DEKs fixes it

## Conclusion
*Draft by agent:claude for the owner's review.*

H1 holds with one correction ([[RUN-20261126-fexqm6]]). The hierarchy lets the server sync
and run agent jobs for the classes the owner delegates while every device is offline,
keeps undelegated classes append-only for it, logs every use, and recovers from the
passphrase alone; the cost is negligible (~6 µs an item, ~70 ms a revocation).

The correction: revocation as ADR-0005 writes it, rotating UK and re-wrapping the same
DEKs, does not revoke. A device that kept the account's wraps reads data written after
its revocation. Rotating the DEKs (and the append key) at revocation, with items naming
their key version, fixes it at no measurable cost; proposed as ADR-0010.

Also shown: a log the admin rewrites and re-signs with the server's own key verifies on
its own; only a device's checkpoint catches it. ADR-0007's checkpoints are therefore
required, not optional, and devices must compare them on every sync.

What the server's admin can read in each state is listed in the run's notes. Not done:
step 4 of the plan, reviewing the threat model with the owner, and timings on ghost001.
