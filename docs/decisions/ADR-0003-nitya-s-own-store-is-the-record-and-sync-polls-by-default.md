# ADR-0003: nitya's own store is the record; indexes are derived; sync polls by default

Status: accepted
Date: 2026-11-23

## Context
Prior art splits on storage (survey in [[AGS-20261123-bhdnxc]]): msgvault (@msgvault) keeps
its own SQLite as the record with raw MIME and cursors and rebuildable indexes; Stalwart
(@stalwart) is a full JMAP and CalDAV server nitya could wrap; cal.diy (@calcom-caldiy)
mostly queries providers live. Twenty (@twenty-crm) has the clearest multi-provider calendar
sync engine. nitya must sync Google, Microsoft 365 and plain IMAP/CalDAV accounts and
answer agents quickly, under per-user encryption (ADR-0005).

Alternatives considered: wrapping Stalwart (less to build, but AGPL with enterprise-gated
multi-tenancy, and nitya's crypto and policy model would sit outside its store); querying
providers live (no offline, no search, every agent call a provider call); spiking both
first (the owner chose not to).

## Decision
- nitya's own database is the system of record: raw MIME and raw iCalendar as received,
  with provider ids, etags and cursors, and raw evidence of each sync. Search (full text,
  later vectors) and other indexes are derived, versioned by generation, and rebuildable.
- Each provider account is a channel: a driver, an opaque cursor the driver owns (Google
  `syncToken` and Gmail `historyId`, Graph delta links, CalDAV sync-collection tokens, IMAP
  CONDSTORE/QRESYNC state), and a sync-stage state machine with resync on 410 or
  `syncStateNotFound` and recovery of stale jobs. Multi-step writes are crash-safe.
- Sync polls on a schedule. Push (provider webhooks) needs a public endpoint and is an
  optimisation, never required for correctness.

## Consequences
- nitya owns iCalendar fidelity: recurrence, exceptions and time zones must round-trip,
  which [[EXP-20261123-bhfzfs]] tests before the engine is built.
- Polling adds latency; acceptable for one person, revisited if it is not.
