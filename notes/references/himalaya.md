---
type: reference
schema_version: 1
citekey: himalaya
title: "himalaya: CLI to manage emails"
authors: pimalaya
year: 2026
doi: null
status: skimmed
role: tool
created: 2026-11-23
author: agent:claude
tags: []
---

## What it says
Surveyed by an agent on 2026-11-22/23 from the source and docs ([[AGS-20261123-bhdnxc]]); [V] checked in source or docs, [I] inferred.

A Rust email CLI from the pimalaya project [V]: IMAP, SMTP, JMAP, Gmail, Graph, Maildir and
mbox backends; offline sync in a separate tool (Neverest); JSON output for every command
with a schema; an MCP server; secrets from `password.command` [V].

## What it means for this work
- The pimalaya crates are candidates for nitya's mail drivers in [[PHS-20261123-bhend2]];
  secrets fetched by command fit neusis's agenix setup.
Informs [[PRJ-0001]].

## Doubts and follow-ups
- Licence and crate boundaries to be checked before depending on them.
