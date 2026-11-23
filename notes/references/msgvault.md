---
type: reference
schema_version: 1
citekey: msgvault
title: "msgvault: a local-first archive of email, chat, calendars and contacts"
authors: kenn-io
year: 2026
doi: null
status: skimmed
role: key-result
created: 2026-11-23
author: agent:claude
tags: []
---

## What it says
Surveyed by an agent on 2026-11-22/23 from the source and docs ([[AGS-20261123-bhdnxc]]); [V] checked in source or docs, [I] inferred.

A Go, MIT, local-first archive of email, chat, calendars and contacts; very active [V].
- SQLite (or Postgres) is the system of record; raw MIME zlib-compressed in a BLOB,
  attachments content-addressed by SHA-256; FTS5 for search, Parquet plus DuckDB for
  analytics, a rebuildable sqlite-vec index labelled by generation; raw evidence and
  checkpoints kept beside the parsed records [V].
- Gmail syncs by `history.list` from a stored cursor, IMAP by a cursor per folder, Graph too;
  calendars by Google `syncToken` with resumable checkpoints; Gmail sync is crash-safe
  (work record committed with the MIME, then attachments, then marked done) [V].
- OAuth tokens are owner-only JSON files or come from an external command (pass/vault);
  headless servers get tokens authorised on a laptop and uploaded with `export-token`; no
  encryption at rest [V].
- One daemon serialises writes and serves an MCP server and an HTTP API. Delegated agent
  tokens carry per-source grants that never imply each other (`search.read`,
  `message.read`, `draft.create`, `calendar.read`, `calendar.write`, `calendar.invite`).
  msgvault never sends: agents create drafts. Calendar writes have dry-run plans, re-check
  access live before each write, report an unknown outcome instead of replaying it, and
  are archived without advancing the sync cursor. Upstream deletion needs a staged
  manifest a person reviews [V].

## What it means for this work
- The baseline for nitya's agent model (ADR-0004) and its store (ADR-0003).
- nitya adds what msgvault lacks: encryption at rest under per-user keys (ADR-0005), RBAC
  (ADR-0006) and a signed audit log.
- Informs [[EXP-20261123-bhfz7e]] and [[EXP-20261123-bhfz49]] (the laptop-then-export
  token flow).
Informs [[PRJ-0001]].

## Doubts and follow-ups
- How it handles Gmail's 7-day `watch` renewal and a too-old `startHistoryId` was not read.
