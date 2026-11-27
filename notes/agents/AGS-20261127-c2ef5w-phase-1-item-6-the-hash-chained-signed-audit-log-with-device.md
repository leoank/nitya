---
id: AGS-20261127-c2ef5w
type: agent_session
schema_version: 1
title: "phase 1 item 6: the hash-chained, signed audit log with device checkpoints"
model: "claude-opus-5-5"
harness: "claude-code"
started: 2026-11-27T10:59+00:00
finished: 2026-11-27T11:09+00:00
created_entities: []
edited_entities: [PHS-20261123-bhen36]
outcome: done             # done | failed | aborted | pending
created: 2026-11-27
author: agent:claude
---

## Task

"Commit and push. build next item": phase 1 item 6, the hash-chained, signed audit log
with device checkpoints (ADR-0007).

## What happened

- Asked the owner two questions; chosen: one chain per account, and nodes checking the
  chain before every keys command as well as on demand. Recorded as ADR-0015.
- Decided without asking, for the owner to check in ADR-0015: a separate `audit.key`;
  what is logged (every Cedar decision before acting, CA and admin events, refusals of
  genuine certificates, not requests without one); log reads and checkpoints are logged
  too; the node pins the audit key it first saw; checkpoints signed by device keys and
  compared across devices.
- `nitya-audit`: sealing, chain verification from a checkpoint, signed checkpoints; 5
  tests, including the spike's finding that a history rewritten and re-signed is caught
  only by a checkpoint.
- Store migration 3 (`audit`, `checkpoints`; 1 test); the CA records its events and
  refusals (1 test checks the chain); a guard puts every Cedar decision in the log before
  the call acts; the `Audit` service (Since, Submit); `nitya-node audit verify [--all]`,
  run before every keys command.
- Surprise on the tailnet: an edited detail of an entry older than a device's checkpoint
  passes an incremental check, since the device never re-reads it; `--all` was added to
  check from the start. Then: an agent's read refused and logged, two devices'
  checkpoints agreeing, the edit caught by `--all`, a redaction accepted, deleted entries
  caught through the other device's checkpoint.
