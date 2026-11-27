# ADR-0015: One audit chain per account, checked by its devices before every keys command

Status: accepted (2026-11-27; the owner chose one chain per account and checks before every keys command; the rest settled by agent:claude for the owner to check)
Date: 2026-11-27

## Context
ADR-0007 asks for an append-only log of every decision, hash-chained and signed by the
server, with device checkpoints so a rewritten history is detected; ADR-0006 puts each
decision's policy ids in its entry. [[EXP-20261123-bhfzeh]] showed a history the server
rewrites and signs again verifies on its own: only a device's checkpoint catches it.
Asked on 2026-11-27 ([[AGS-20261127-c2ef5w]]), the owner chose:
- one chain per account, so no account's devices see another's entries;
- nodes check the chain before every keys command, and on demand.

Alternatives considered:
- One chain for the server: one ordering, but every device would read, or at least hash,
  every account's entries.
- Checking only on demand (or on a timer): cheaper, but a rewrite goes unnoticed until
  someone looks.

## Decision
- **Entries** (`nitya.v1.AuditEntry`): a UUIDv7 id, seq, chain (the account), operation
  id, actor (`device:<key>`, `agent:<key>`, `system:ca`, `system:admin`), action,
  resource, decision, policy ids, time, the SHA-256 of a detail, and the previous entry's
  hash. An entry's hash is SHA-256 over a tag and its encoded bytes; the server signs it
  with `audit.key`, a key of its own beside `ca.key`. The detail (hostnames, reasons)
  sits outside the hash and can be redacted.
- **What is logged**: every Cedar decision, allowed or not, before the call acts (if the
  entry cannot be written the call is refused); the CA's enrollments and renewals; the
  admin's enrollment codes and revocations; every refusal of a request whose certificate
  is genuine, with its reason. Requests without a genuine certificate name no account
  and are not logged.
- **Store**: `audit` (chain, seq, entry, detail, hash, signature) and `checkpoints`, with
  no foreign keys; appends take SQLite's write lock, so the admin commands and the server
  chain in turn.
- **Devices** (`Audit/Since`, `Audit/Submit`): a node fetches its account's entries from
  its last checkpoint, checks hashes, signatures, links and details, compares other
  devices' signed checkpoints, saves the new head and hands it back signed. It pins the
  audit key it first saw. `nitya-node audit verify --all` checks from the first entry,
  which also catches an edited detail of an entry older than the checkpoint.
- Reading the log and handing back a checkpoint are themselves decisions, so each check
  adds entries the next check verifies.

## Consequences
- An admin who rewrites, truncates or re-signs history is caught by the next check of any
  device whose checkpoint is past the change; changes before every checkpoint are caught
  only by `--all`.
- Holding a copy of a genuine certificate is enough to add refusal entries to its
  account's chain; the tailnet limits who can try.
- The log grows with every call, checks included; retention stays an open question
  (ADR-0007). Admin actions not tied to an account would go to a system chain; none
  exist yet.
- The delegate's unwraps and decryptions are logged when sync uses the delegate (phase 2).
