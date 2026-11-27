---
id: AGS-20261127-jk0h1r
type: agent_session
schema_version: 1
title: "phase 2 item 1: the channel engine: drivers, opaque cursors, sync stages, stale-job recovery, polling"
model: "claude-opus-5-5"
harness: "claude-code"
started: 2026-11-27T16:55+00:00
finished: 2026-11-27T17:05+00:00
created_entities: []
edited_entities: [PHS-20261123-bhen36, PHS-20261123-bhen6w]
outcome: done             # done | failed | aborted | pending
created: 2026-11-27
author: agent:claude
---

## Task

"Close phase 1 and start phase 2": phase 1 closed (its goal's app enrollment waits for
phase 6, noted on its page), phase 2 started, and its item 1 built: the channel engine.

## What happened

- Asked the owner two questions; chosen: a channel syncs only while `tokens` and its class
  are consented to; poll every minute, with a command to sync now. Recorded as ADR-0017.
- Decided without asking, for the owner to check in ADR-0017: the driver interface (one
  `fetch` from a cursor, upserts with etags, deletions, rotated credentials, three kinds of
  error); the stages and the backoff (poll interval doubled per failure, at most an hour);
  a lease per channel in the store, stale after 10 minutes; a full sync tombstones what it
  did not see; quiet polls are not in the audit log (1,440 entries a day per channel
  otherwise), runs that changed something or failed are, as is entering
  `waiting_for_consent` once.
- Store migration 4 (credential, retry state, lease, tombstones, `sync_runs`; 1 test);
  `Delegate::consented`; the engine with 8 tests against a scripted fake driver; the
  `Channels` service (List, SyncNow) with `channels.read` and `channels.sync` in Cedar;
  `nitya-node channels` and `sync now [ID]`; `--poll-secs` and the module's `pollSeconds`.
- neusis: its nitya-server feature test checked the exact flag order, which the new
  `--poll-secs` breaks; made it check the parts separately (uncommitted, the owner's).
- On the leoank tailnet with a channel of a provider that has no driver: it waited for
  consent, then stopped at `no_driver`; an unknown channel was not found; an agent was
  refused `channels.sync`; the poller left the channel alone; all of it in the audit log.
- The guide: a *Channels and sync* page; *Where it stands* now says phase 2.
