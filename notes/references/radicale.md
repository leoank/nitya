---
type: reference
schema_version: 1
citekey: radicale
title: "Radicale: a CalDAV and CardDAV server"
authors: Kozea
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

A Python CalDAV and CardDAV server (GPL-3) [V]: one `.ics` file per item in a folder tree,
sync-collection (RFC 6578) tokens from a history cache, pluggable auth and rights modules,
and a storage hook that can `git commit` every change by user, which is a free versioned
audit trail [V]. Optional iTIP over iMIP for scheduling email [V].

## What it means for this work
- Plain-text calendar objects plus history as an audit and recovery pattern for nitya's
  store (ADR-0003); a reference CalDAV server for [[EXP-20261123-bhfzfs]].
Informs [[PRJ-0001]].

## Doubts and follow-ups
- None yet.
