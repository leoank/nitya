---
id: AGS-20261127-meqexs
type: agent_session
schema_version: 1
title: "phase 2 item 2: the Google Calendar driver, two-way, with the contract from EXP-20261123-bhfzfs"
model: claude-opus-5-5
harness: claude-code
started: 2026-11-27T18:37+00:00
finished: null
created_entities: [EXP-20261127-mqzh92, RUN-20261127-mr3rsk]
edited_entities: [PHS-20261123-bhen6w, PRJ-0001]
outcome: pending          # done | failed | aborted | pending
created: 2026-11-27
author: agent:claude
---

## Task
Phase 2 item 2 of [[PHS-20261123-bhen6w]]: the Google Calendar driver, two-way, on the
channel engine (ADR-0017) with [[EXP-20261123-bhfzfs]]'s contract; and the way a Google
account reaches the server and events change in nitya, which the owner settled first.

## What happened
- Asked the owner three questions; chosen: sign-in from a node (the server holds the
  OAuth client and keeps the tokens), one channel per calendar, minimal CLI edits on the
  node (ADR-0018).
- Before building, [[EXP-20261127-mqzh92]] ([[RUN-20261127-mr3rsk]]) checked Google's sync
  tokens, 410 and If-Match on a throwaway calendar: all held, except that a changed
  occurrence re-sends its whole series, so the engine skips unchanged ETags.
- Built: `nitya-ical` (reader and writer), the Google driver (iCalendar both ways, sync
  token cursor, insert and If-Match writes, refusals before queueing), the store's outbox
  (migration 5), the engine writing the outbox before fetching, `Channels` RPCs for
  sign-in, items and writes (Cedar `channels.add`, `calendar.read`, `calendar.write`),
  `nitya-node channels add google` and `nitya-node events`, `--google-client` and the
  module's `googleClientFile`, ADR-0018, the guide.
- Decided without asking (ADR-0018, for the owner to check): channels named by calendar
  id; sign-in tokens held in memory for 10 minutes; writes send no invitations; refused
  and conflicting changes stay in the outbox, not retried; no alarm means the default
  reminders; a node lists, shows, adds from a file, edits in a file or `$EDITOR`, deletes.
- Surprise: the driver's test against Google once got 410 for a sync token minutes old
  (one in six runs); the engine's full sync covers it.
- Checked: unit tests; the driver against Google on a throwaway calendar (ignored test,
  run by hand: insert, edit, stale edit refused, series, moved occurrence across the clock
  change, incremental, delete); on the leoank tailnet everything but the browser sign-in,
  which the owner runs.
