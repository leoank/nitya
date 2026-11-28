# ADR-0018: Google calendars are signed in from a node, one channel per calendar, and written back from the node's CLI

Status: accepted (2026-11-27; the owner chose sign-in from a node, a channel per calendar and minimal CLI edits; the driver's details settled by agent:claude for the owner to check)
Date: 2026-11-27

## Context
Phase 2 item 2 is the Google Calendar driver, two-way, on ADR-0017's engine and with
[[EXP-20261123-bhfzfs]]'s contract. Three things were open, and the owner settled them
on 2026-11-27 ([[AGS-20261127-meqexs]]):
- How a Google account reaches the server. `nitya-consent` signs an account in and
  writes the refresh token for agenix, which the token probe on ghost001 reads; a channel
  instead keeps its credential in the store, sealed under `tokens` (ADR-0017).
- What one channel is: a calendar, or a whole account with a cursor per calendar.
- How events change in nitya, so there is something to write back: no API for it existed.

Alternatives considered:
- Importing the agenix token into a channel once: no new sign-in flow, but the token
  would also stay decrypted in `/run/agenix` on the server.
- One channel per account: fewer rows, but one failing calendar holds up the others, and
  ADR-0017's one cursor per channel would hold many.
- Read-only now, writing back once the app or an editing API exists.

[[EXP-20261127-mqzh92]] checked the parts of Google's API the driver rests on that the
round trip had not: sync tokens, 410, `If-Match`.

## Decision
- **Sign-in from a node.** `nitya-node channels add google` runs the browser sign-in on
  the owner's machine (RFC 8252: a loopback redirect and PKCE). The server holds the
  Desktop OAuth client (`--google-client`, an agenix secret), builds the sign-in URL,
  trades the one-time code for the tokens, and lists the account's calendars. The refresh
  token never reaches the node, and waits in the server's memory, not on disk, for ten
  minutes until calendars are chosen. Signing in again replaces a channel's credential and
  lets a channel waiting in `needs_auth` run again. Cedar: `channels.add`; agents are
  refused.
- **One channel per calendar**: provider `google`, class `calendar`, named by the
  calendar's id. Its credential is `{refresh_token, calendar_id}` under `tokens`; access
  tokens are only in memory.
- **Reading**: `events.list` with `singleEvents=false`; the cursor is the sync token, or a
  page token while a listing is paged. One item per Google event, as iCalendar with one
  VEVENT: a master with its RRULE, RDATE and EXDATE lines as Google holds them, an
  exception with its RECURRENCE-ID, a cancelled occurrence as a STATUS:CANCELLED override.
  A cancelled event with no series is a delete, as is a cancelled occurrence of a series
  that is gone. Times keep their zone; a time without one is UTC. Google re-sends a whole
  series when one occurrence changes, so the engine skips an item whose ETag it already
  has.
- **Writing**: changes come from devices, sealed under the channel's class, through
  `Channels.Write`. The server opens the change to check it against what Google can hold
  (no floating time, no RANGE=THISANDFUTURE, one VEVENT, IANA zones, DTEND rather than
  DURATION, alarms before the start, one value per property Google has no field for) and
  refuses what it cannot before queueing. A queued change waits in the store's outbox; a
  run writes the outbox first, then fetches. Updates and deletes carry `If-Match` with the
  ETag the device read, so a change made at Google meanwhile is refused as a conflict and
  stays in the outbox with the reason, never overwriting it. New events are `insert`ed
  with the UID given; a new override of a series is written to the occurrence it names.
  Writes send no invitations (`sendUpdates=none`): inviting is the separate
  `calendar.invite` grant (ADR-0004).
- **The node's CLI**: `nitya-node events CH` lists, `show`, `add` from a file, `edit` from
  a file or `$EDITOR`, `delete`. The node opens and seals items with the account's keys;
  Cedar decides `calendar.read` and `calendar.write` on the channel.
- **Mapping**: as the round trip's driver did. Properties Google has no field for (X-,
  CATEGORIES, PRIORITY) are private extended properties; no alarm means the calendar's
  default reminders.

## Consequences
- Google calendars sync both ways from the deployed server once it has
  `googleClientFile`; the agenix refresh token stays the token probe's only.
- Sign-in needs a browser on the node's machine, or a forwarded loopback port with
  `--no-browser`.
- A change refused as a conflict waits in the outbox until the owner edits the event
  again, which replaces it; nothing retries it alone.
- An event whose reminders were explicitly none at Google comes back with the default
  reminders if nitya writes it: iCalendar has no way to say "no alarms, not the default".
- Google answers a fresh sync token with 410 now and then (once in six driver test runs
  on 2026-11-27), so the engine's full sync is an ordinary path.
- Invitations, free/busy and Microsoft and CalDAV drivers are later items.
