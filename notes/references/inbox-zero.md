---
type: reference
schema_version: 1
citekey: inbox-zero
title: "Inbox Zero: AI email assistant"
authors: Elie Steinbock
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

An AI email assistant (Next.js, Postgres, Redis; AGPL-3.0 with an extra clause on
commercial use) [V]. Gmail and Outlook webhooks; tokens under versioned AES-256-GCM keys
[V]. Its rule executions once had `PENDING` and `REJECTED` statuses, now deprecated: it
moved from an approval queue to automatic rules that pre-draft replies [V].

## What it means for this work
- A warning for ADR-0004: approval queues are abandoned when they add friction. Drafts,
  not sends, are the safe default; approval must be one tap and batched.
Informs [[PRJ-0001]].

## Doubts and follow-ups
- None yet.
