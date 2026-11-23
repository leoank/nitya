---
type: reference
schema_version: 1
citekey: rallly
title: "Rallly: meeting polls"
authors: Vella, Luke
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

A meeting-poll app (TypeScript, Next.js, Prisma; AGPL-3.0) [V].
- A public poll link needs no account. Each response gets an edit token of at least 32
  random characters minted by the app (deliberately not a database default, so a forgotten
  mint fails loudly); each emailed invite has its own `PollInvite.token`, rotated on
  reactivation, with an explicit `revokedAt`; an email typed by a respondent is unverified
  and never claims an invite [V].
- Votes are yes, if-need-be, no; organisers can hide participants or scores [V].
- Finalising creates a `ScheduledEvent` and emails `invite.ics`; Google and Microsoft
  connections keep OAuth secrets under AES-256-GCM [V].

## What it means for this work
- Copy the capability-token design for [[PHS-20261123-bhensm]]: a token per invitee and per
  response, rotation, revocation, unverified email never trusted.
- Only these routes would face the public internet; how is still open on [[PRJ-0001]].
Informs [[PRJ-0001]].

## Doubts and follow-ups
- AGPL: learn from it, do not copy code into nitya.
