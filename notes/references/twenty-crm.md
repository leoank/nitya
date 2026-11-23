---
type: reference
schema_version: 1
citekey: twenty-crm
title: "Twenty: an open-source CRM"
authors: Twenty
year: 2026
doi: null
status: skimmed
role: method
created: 2026-11-23
author: agent:claude
tags: []
---

## What it says
Surveyed by an agent on 2026-11-22/23 from the source and docs ([[AGS-20261123-bhdnxc]]); [V] checked in source or docs, [I] inferred.

A TypeScript (NestJS) CRM with the cleanest multi-provider calendar sync engine in the
survey [V]; licence reported as NOASSERTION, believed AGPL core plus commercial code [I].
- Separate import and create drivers per provider (Google, Microsoft, CalDAV); an opaque
  `syncCursor` string per channel: Google `nextSyncToken` with full-sync fallback on 410,
  Graph `/me/calendar/events/delta` with the deltaLink, CalDAV a JSON cursor [V].
- Cron jobs for list-fetch, import, stale-job recovery and relaunching failed channels,
  with indexed sync stages [V].

## What it means for this work
- The shape of nitya's sync engine (ADR-0003): one channel is a driver, an opaque cursor it
  owns and a sync-stage state machine with stale-job recovery; polling on cron is correct
  without public webhooks.
- [[EXP-20261123-bhfzfs]] tests the drivers' round trip before this engine is built.
Informs [[PRJ-0001]].

## Doubts and follow-ups
- None yet.
