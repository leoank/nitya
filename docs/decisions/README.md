# Decisions

One architecture decision record per file, `ADR-0001-short-title.md`, numbered in order
and never renumbered. A decision that changes an earlier one supersedes it; both stay.

| ADR | Decision | Status |
|-----|----------|--------|
| [ADR-0001](ADR-0001-a-rust-server-and-a-node-per-machine-each-on-its-own-tailnet-identity.md) | A Rust server on ghost001 and a node per machine, each on its own tailnet identity, independent of saarthi | accepted |
| [ADR-0002](ADR-0002-the-android-client-is-the-nitya-workspace-in-saarthi-s-app.md) | The Android client is the Nitya workspace in saarthi's app | accepted |
| [ADR-0003](ADR-0003-nitya-s-own-store-is-the-record-and-sync-polls-by-default.md) | nitya's own store is the record; indexes are derived; sync polls by default | accepted |
| [ADR-0004](ADR-0004-agents-get-scoped-grants-draft-not-send-and-act-through-approved-plans.md) | Agents get scoped grants, draft rather than send, and act through approved plans; nitya spawns saarthi sessions first | accepted |
| [ADR-0005](ADR-0005-device-keys-a-per-user-key-hierarchy-and-a-consented-sync-delegate.md) | Device keys with short-lived certificates, a per-user key hierarchy, and a consented sync delegate; no MLS | accepted, amended by ADR-0010 |
| [ADR-0006](ADR-0006-cedar-is-the-policy-engine.md) | Cedar is the policy engine | accepted |
| [ADR-0007](ADR-0007-a-signed-hash-chained-audit-log.md) | A signed, hash-chained audit log of every read and write | accepted |
| [ADR-0008](ADR-0008-poll-respondents-reach-a-cloudflare-worker-not-nitya.md) | Poll respondents reach a Cloudflare Worker that holds only poll data, never nitya | accepted |
| [ADR-0009](ADR-0009-provider-oauth-apps-are-a-terranix-stack-here-their-secrets-agenix-in-neusis.md) | Provider OAuth apps are a terranix stack in this repo; their secrets are agenix files in neusis | accepted |
| [ADR-0010](ADR-0010-revoking-a-device-rotates-the-data-keys-and-the-append-key-not-only-the-user-key.md) | Revoking a device rotates the data keys and the append key, not only the user key | accepted |
| [ADR-0011](ADR-0011-grpc-api-one-time-enrollment-codes-and-nitya-s-own-device-certificates-bound-to-the-tailnet-node-id.md) | The API is gRPC; devices enroll with a one-time code and get nitya's own short-lived certificates, bound to their tailnet node id | accepted |

Each record:

```markdown
# ADR-0001: <the decision, as a sentence>

Status: proposed | accepted | superseded by ADR-xxxx
Date: YYYY-MM-DD

## Context
What forced the decision, and the alternatives considered.

## Decision
What was chosen.

## Consequences
What follows, good and bad.
```
