# ADR-0012: The store is one SQLite database in the server's state directory, built before the key hierarchy

Status: accepted
Date: 2026-11-26

## Context
ADR-0003 makes nitya's own store the record (raw items, channels with opaque cursors and
a sync stage) but names no database. Phase 1 planned the key hierarchy (item 3) before
the store (item 4), yet the hierarchy's wraps, the data keys and the items all have to
live somewhere, and ADR-0011's CA keeps its state in files only until a store exists.
Asked on 2026-11-26 ([[AGS-20261126-jhdpp1]]), the owner chose the store first, on SQLite.

Alternatives considered:
- PostgreSQL, run by NixOS on ghost001: another service to run, back up and upgrade, for
  one server process with one writer; worth it only with several writers.
- Keeping the plan's order with files for the keys: state written twice, once to files
  and again when moved into the store.

## Decision
- **SQLite**, through rusqlite with SQLite built in (`bundled`), one file `nitya.db` in
  the server's state directory; WAL mode, foreign keys on, a busy timeout so the admin
  commands can write while the server runs.
- **Migrations** are SQL in the binary, applied in order at open, the version in SQLite's
  `user_version`; a database newer than the binary is refused, not touched.
- **Contents now**: accounts; the CA's enrollment codes (hashes), issued certificates and
  revoked device keys, moved from files; channels (provider, data class, opaque cursor,
  sync stage) and items (provider id, etag, key version, sealed bytes), as ADR-0003 has
  them. Item 3's wraps and data keys come as a later migration.
- **Keys stay out of it.** The CA's signing key stays a file beside the database, never
  in it (ADR-0005: keys never on disk beside the data they protect, here the certificates
  they sign); items are stored sealed, and the store never sees a data key.
- Multi-step writes go in one transaction; claiming an enrollment code is one
  `DELETE … RETURNING`.

## Consequences
- Phase 1 builds item 4 before item 3.
- Backups are one file (with SQLite's online backup); restoring an old copy brings back
  revoked devices as not revoked, so a restore must be followed by re-revoking (noted for
  item 7's deployment).
- One server process writes; scaling out would mean another ADR.
