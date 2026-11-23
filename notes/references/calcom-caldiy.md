---
type: reference
schema_version: 1
citekey: calcom-caldiy
title: "cal.diy: the MIT community edition of Cal.com"
authors: Cal.com
year: 2026
doi: null
status: skimmed
role: background
created: 2026-11-23
author: agent:claude
tags: []
---

## What it says
Surveyed by an agent on 2026-11-22/23 from the source and docs ([[AGS-20261123-bhdnxc]]); [V] checked in source or docs, [I] inferred.

The MIT fork of Cal.com with the enterprise code removed (TypeScript, Next.js, tRPC, Prisma,
Postgres). Its README calls it for personal, non-production use [V].
- Busy times come from live freebusy queries; an opt-in calendar-subscription feature adds a
  local cache fed by Google `events.watch` channels with a per-calendar `syncToken` (first
  sync today to +3 months, `singleEvents=true`) and Microsoft Graph `/subscriptions` with a
  `clientState` secret plus delta links [V]. No handling of an expired syncToken (410) was
  found [V].
- CalDAV passwords are AES-256-CBC without an authentication tag; Google OAuth tokens sit in
  a plaintext JSON column, double-written to an `encryptedKey` column that is silently
  skipped when no keyring is configured [V].
- A public booking page (no account) writes the event to a destination calendar and emails
  `.ics` [V].
- `PERMISSIONS.md` maps checks to `resource.action` strings; booking audit records have an
  actor type (USER, GUEST, ATTENDEE, SYSTEM, APP), UUIDv7 ids, an `operationId` correlating
  one action across tables, no foreign key to the booking (they outlive it), and actor PII
  that can be anonymised [V].

## What it means for this work
- Adopt the audit actor model (user, guest, system, agent, with UUIDv7 and `operationId`)
  for nitya's signed log; adopt one subscription/sync adapter per provider.
- Avoid credential encryption that fails open or is half-migrated, and unauthenticated CBC
  (see *Lessons* on [[PRJ-0001]]).
- Its public booking page is the shape of [[PHS-20261123-bhensm]]'s poll page; how outsiders
  reach it is an open question.
Informs [[PRJ-0001]].

## Doubts and follow-ups
- Webhook push needs a public HTTPS endpoint; nitya behind a tailnet polls instead.
