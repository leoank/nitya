---
id: EXP-20261126-hvzed5
type: experiment
schema_version: 1
title: Does the tailnet give each device a stable identity the server can read from a connection and bind a device certificate to, one that survives restarts and that a new join cannot take over?
project: PRJ-0001
status: concluded         # planned | active | paused | concluded | abandoned
outcome: positive         # null | positive | negative | inconclusive | infra
influenced_by: []
created: 2026-11-26
author: agent:claude
protocols: []
materials: []
tags: []
review: pending
---

## Question
ADR-0005 binds each device certificate to the device's tailnet identity, and the owner
chose (2026-11-26) a one-time enrollment code over gRPC with nitya's own certificate
format. A certificate is only as good as what it is bound to. What does the server learn
about a caller from the connection alone (tsnet's status for the remote address), and
which of it is stable across the caller's restarts and unique to its join?

## Hypothesis and predictions
H1: each tailnet node has a stable node id (`ID`, the StableNodeID) and a node key
(`PublicKey`) in the server's status, found from the connection's remote address. The id
is the thing to bind; the hostname is not, since anyone can join with any hostname.

- P1. The server resolves a connection's remote address to a peer whose `ID` and
  `PublicKey` are non-empty. If not, binding needs another source (LocalAPI whois) or a
  different design.
- P2. The node restarted from the same state directory comes back with the same `ID`,
  `PublicKey` and address. If the id changes, a certificate would break on every restart
  and must not bind to it.
- P3. A second join with a fresh state directory and the same hostname gets a different
  `ID` and `PublicKey` (and the tailnet may rename it). If it got the same id, a stolen
  hostname would be enough to pass as the device.

## Plan
1. `spikes/tailnet-identity`: one binary, as server (listens, prints the status fields
   of each caller as JSON) or as node (joins and dials). Both ephemeral, on the leoank
   tailnet with the owner's existing reusable key (neusis `leoankTsAuthKey`).
2. The run script: server up; node A joins and dials; node A stops and starts again from
   the same state and dials; node B joins with a fresh state and A's hostname and dials.
   Each caller's `ID`, `PublicKey`, `HostName`, `DNSName` and address go to `out/`.
3. Compare the three calls; the conclusion says what the certificate binds.

## Success criteria
P1 and P2 hold: the certificate binds the node id (with the node key recorded). P3 decides
whether hostname may appear in it as a label only. A failed P1 or P2 sends the binding
back to the owner before item 2 builds on it.

## Runs
One line per run after its link, what was tried and what it showed; `yajna validate`
rejects a bare link here. Metric runs also land in results.tsv.
- [[RUN-20261126-hw6442]] four calls from two joins on rogue: the node id survives a restart and differs for a fresh join with the same hostname; the node key changes on every restart

## Conclusion
Positive, from [[RUN-20261126-hw6442]]. The server reads the caller's stable node id from
its own tsnet status by the connection's address (P1). The id survives a restart from the
same state (P2), and a fresh join gets a new one even with the same hostname (P3), which
only changes the DNS name. The node key does not survive a restart, at least for an
ephemeral node, so it cannot be bound.

For phase 1 item 2: a device certificate binds the account, the device's Ed25519 key and
the tailnet node id; the hostname goes in as a label only. The server accepts a request
when the connection's node id equals the certificate's. A device whose tailnet identity is
forgotten or removed comes back with a new id and enrolls again, which is what revoking
its tailnet access should mean anyway.
