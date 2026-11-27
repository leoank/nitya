---
id: AGS-20261126-jhdpp1
type: agent_session
schema_version: 1
title: "Phase 1 item 4 before item 3: the store on SQLite, with the CA moved into it"
model: "claude-opus-5-5"
harness: "claude-code"
started: 2026-11-26T16:52+00:00
finished: 2026-11-26T17:11+00:00
created_entities: []
edited_entities: [PHS-20261123-bhen36, PHS-20261123-bhenjn, PRJ-0001]
outcome: done             # done | failed | aborted | pending
created: 2026-11-26
author: agent:claude
---

## Task

"Move to the next item" after item 2. Item 3 (accounts and the key hierarchy) needed the
owner's word first; the owner accepted ADR-0010, chose a separate X25519 wrap key per
device, and chose to build the store (item 4) first, on SQLite. This session is item 4.

## What happened

- ADR-0010 marked accepted, ADR-0005 amended by it, with the project page and phase 0
  lines; the experiment's conclusion itself stays a draft for `yajna approve`.
- ADR-0012: one SQLite database, migrations in the binary, keys kept out of it.
- `nitya-store` (rusqlite, bundled): accounts, the CA's codes, certificates and revoked
  keys, channels with cursor and stage, items as sealed bytes; a sync batch writes items
  and cursor in one transaction. 5 tests.
- The CA moved from files to the store (`ca.key` stays a file); its 8 tests pass
  unchanged. Decided without asking: an enrollment code creates its account row if
  missing; a store error while checking revocation counts as revoked.
- Surprise: under nix the link asks for `-lsqlite3` despite `bundled`; nixpkgs' sqlite is
  now a build input.
- On the leoank tailnet: the same enroll, reuse, copied-key and revocation checks pass
  with the admin commands writing the database while the server runs. A first attempt
  timed out because the previous run's ephemeral server still held its hostname, so the
  name went to the old machine; the check now uses fresh hostnames.
- The wrap key (X25519 per device) is for item 3.
