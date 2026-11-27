# ADR-0017: The channel engine syncs a channel only with consent, every minute, with a sync-now

Status: accepted (2026-11-27; the owner chose consent-gated channels, a one-minute poll and a command to sync now; the engine's details settled by agent:claude for the owner to check)
Date: 2026-11-27

## Context
ADR-0003 makes each provider account a channel with a driver, an opaque cursor, a
sync-stage machine with resync and stale-job recovery, crash-safe writes, and polling.
ADR-0005 lets the server use an account's data keys only for classes the owner consented
to; a channel needs two: `tokens` for its credential (Microsoft rotates refresh tokens on
use, so the server must also write it) and the channel's class. Asked on 2026-11-27
([[AGS-20261127-jk0h1r]]), the owner chose:
- a channel syncs only while `tokens` and its class are consented to;
- each channel is polled every minute, and a device can sync now.

Alternatives considered:
- Pulling without consent to the class, storing changes sealed to the append key: the
  server could not read them, so two-way sync would wait for a device anyway.
- Polling every 5 or 15 minutes: fewer requests, slower to show changes.

## Decision
- **Drivers** implement one call: changes since a cursor (from the start without one),
  as upserts of raw provider data with etags and deletions, the next cursor, whether more
  pages follow, and a rotated credential if the provider issued one. They report a
  forgotten cursor, a refused credential, or a transient failure.
- **Stages**: `new`, `full_sync`, `incremental`; `waiting_for_consent`; `needs_auth` (the
  poller leaves it alone until someone signs in again); `failed` (retried after the poll
  interval doubled per failure, at most an hour); `no_driver`.
- **A run** takes the channel's lease in the store, so the poller and a sync-now never
  run one channel at once; a lease older than 10 minutes belongs to a run that died and is
  taken over. Each page's items and the new cursor are one transaction. A forgotten cursor
  starts a full sync once in the same run; a full sync tombstones the items it did not
  see. A rotated credential is sealed and stored before anything else.
- **Sealing**: items under the class's current key through the delegate, bound to their
  channel and provider id; the credential under the `tokens` key. Deleted items stay as
  tombstones with no data.
- **Evidence**: every run that changed something or failed is a row in `sync_runs` and an
  audit entry (`system:sync-delegate`, `sync.run`); entering `waiting_for_consent` is
  logged once. A quiet poll is not logged: at one a minute it would add 1,440 entries a
  day per channel for nothing the owner needs to see.
- **Sync now**: `nitya-node sync now [ID]` runs the account's channels at once through the
  `Channels` service (`channels.read`, `channels.sync` in Cedar; agents are refused).
  `--poll-secs` (module: `pollSeconds`) sets the interval, 60 by default.

## Consequences
- With polling every minute a change at a provider shows in nitya within about a minute.
- ADR-0005's "every use written to the audit log" holds for runs that did something;
  the delegate's unwraps in quiet polls are not logged, by the reasoning above.
- The engine has no driver yet; Google, Graph and CalDAV come with phase 2 items 2 to 4,
  as does signing an account in to make a channel.
