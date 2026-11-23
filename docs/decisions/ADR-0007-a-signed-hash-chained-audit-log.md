# ADR-0007: A signed, hash-chained audit log of every read and write

Status: accepted
Date: 2026-11-23

## Context
Access must be controlled and auditable. cal.diy's booking audit (@calcom-caldiy) records
an actor type, time-sortable ids and an operation id correlating one action across
tables, and outlives what it describes; Radicale (@radicale) can commit every change to
git. Wire has no signed audit log.

Alternatives considered: plain log tables (editable by whoever holds the database);
git history of objects only (covers writes, not reads or decisions).

## Decision
An append-only log, each entry `{id (UUIDv7), operation id, actor (user, device
certificate, agent grant, system, guest), action, resource, decision, policy ids, time}`,
hash-chained to the previous entry and signed by the server; devices keep periodic
checkpoints of the chain head so a rewritten history is detected. Entries carry no
foreign keys and survive the records they describe; personal data in them can be
redacted without breaking the chain (hash the redactable part separately).

## Consequences
- Every read by an agent is logged, which makes the log large; retention and compaction of
  read entries is an open question.
