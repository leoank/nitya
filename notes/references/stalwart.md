---
type: reference
schema_version: 1
citekey: stalwart
title: "Stalwart: mail and collaboration server"
authors: Stalwart Labs
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

A Rust mail and collaboration server (AGPL-3.0 plus a proprietary licence for multi-tenancy
and other enterprise features) [V].
- JMAP Mail (RFC 8621), JMAP Calendars (draft) and Contacts, CalDAV with scheduling
  (RFC 6638), WebDAV ACL, JMAP Sharing [V].
- Pluggable store, blob and search backends (RocksDB, FoundationDB, Postgres, SQLite, S3) [V].
- Encryption at rest per mailbox with the user's own OpenPGP or S/MIME key, so the server
  can append new mail it cannot read [V]; roles with nested subroles and tenants that cap
  their members' permissions [V].

## What it means for this work
- Considered and not chosen as nitya's store (ADR-0003); kept as a model for separating
  store, blob and search, and for write-only ingest under a user's public key (ADR-0005).
- nitya might later expose JMAP or CalDAV so ordinary clients keep working [I].
Informs [[PRJ-0001]].

## Doubts and follow-ups
- Could nitya speak CalDAV outward to phone calendar apps instead of building every view?
