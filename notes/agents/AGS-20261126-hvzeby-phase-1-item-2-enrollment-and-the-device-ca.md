---
id: AGS-20261126-hvzeby
type: agent_session
schema_version: 1
title: "Phase 1 item 2: enrollment and the device CA"
model: "claude-opus-5-5"
harness: "claude-code"
started: 2026-11-26T16:16+00:00
finished: 2026-11-26T16:27+00:00
created_entities: [EXP-20261126-hvzed5, RUN-20261126-hw6442]
edited_entities: [PHS-20261123-bhen36, PRJ-0001]
outcome: done             # done | failed | aborted | pending
created: 2026-11-26
author: agent:claude
---

## Task

Phase 1 item 2: enrollment and the device CA (Ed25519 device keys, short-lived
certificates, signed requests, revocation), after the owner committed the probe wiring.

## What happened

- Asked the owner what ADR-0005 left open: the API transport (gRPC with tonic), first
  enrollment (a one-time code) and the certificate format (nitya's own). ADR-0011.
- What "bound to the tailnet identity" means was unchecked, so [[EXP-20261126-hvzed5]]
  ran first ([[RUN-20261126-hw6442]]): the node id is stable and unique to a join; the
  node key changes on every restart, which nobody expected; the hostname is a label.
  Conclusion drafted, positive, review pending.
- Decided without asking, recorded in ADR-0011: 24-hour certificates renewable up to 30
  days after their end, 15-minute codes, a 60-second clock window with a nonce cache,
  the body hash over the prost encoding, revocation by device key, the CA's state as
  files until the store, the device key as a file until TPM/Keystore work.
- Built: tonic's connector and `Connected` back in the tailnet crate, and the peer's node
  id; `nitya-proto` (`nitya.v1.Devices`: Enroll, Renew, WhoAmI); `nitya-device` (keys,
  certificates, enrollment proof, request signing); the CA and service in
  `nitya-server` with `enroll-code`, `devices` and `revoke`; `nitya-node enroll` and
  `whoami` (renewing at half-life). protoc added to the flake. Item 1's text answer is
  replaced by `WhoAmI`, and `--tailnet-dir` by `--state-dir` in both binaries.
- Tests: 4 in nitya-device, 8 in the CA (every refusal, renewal grace, revocation,
  restart). On the leoank tailnet, all ephemeral: enroll and whoami pass; a reused code,
  A's key and certificate on another node with A's hostname, and A after revocation are
  refused. Renewal on the wire was not exercised (it needs 12 hours); the CA test covers it.
- The three log lines were written together at 16:27, after the work they describe.
- Not committed: awaiting the owner. The run needs `yajna run record` with the commit
  that holds the spike.
