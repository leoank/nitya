# Channels and sync

A channel is one provider account and one kind of data, for example one calendar of a
Google account. The server keeps each channel in sync on its own, every minute, and you
can make it sync at once. You read and change a channel's events from any of your
devices, and the server writes your changes back to the provider.

Google calendars can be connected now; Microsoft and CalDAV come next.

## What the server needs from you

To sync while your devices are offline, the server has to read two things: the
channel's sign-in token and the data it syncs. Both are under your keys, so a channel
syncs only after you have [consented](keys.md#the-sync-delegate) to both:

```sh
nitya-node keys consent tokens
nitya-node keys consent calendar
```

Until then the channel waits, in the stage `waiting_for_consent`. If you withdraw either
consent, it waits again.

## Connecting a Google account

The server needs Google's sign-in client (`--google-client`, or `googleClientFile` in the
[module](../deployment/modules.md)). Then, on a machine with a browser:

```sh
nitya-node keys consent tokens       # once; the server keeps the account's sign-in
nitya-node channels add google
```

The browser opens Google's sign-in page. Choose the account and allow nitya to use its
calendars; the tab then says nitya has the answer. Back in the terminal, the account's
calendars are listed. Choose by number, or `all`:

```text
  1  Ank (primary)	owner	ank@example.org
  2  Family	writer	family…@group.calendar.google.com
calendars to sync, by number (1,3) or all: 1
channel 1: google calendar ank@example.org (new)
```

Each calendar you choose becomes a channel, named by the calendar's id, and syncs within a
minute once the server also has consent to `calendar`. Your machine handles only the
sign-in's one-time code: the server trades it for the account's tokens and keeps them in
its store under your `tokens` key.

Without a browser on that machine, `--no-browser` prints the URL. Google sends the browser
back to the machine running `nitya-node`, so open the URL there, or forward the port the
URL names (`ssh -L <port>:127.0.0.1:<port>`).

If Google stops accepting the sign-in (you removed nitya from your Google account, or
changed its password), the channel waits in `needs_auth`. Run `channels add google` again
and choose the same calendar: it gets the new sign-in and syncs again.

## Seeing where each channel stands

```sh
nitya-node channels
```

```text
1	google	calendar	primary	incremental	synced 42s ago
```

| Stage | What it means |
|-------|---------------|
| `new`, `full_sync` | the first sync, or a resync, is under way |
| `incremental` | up to date; the next poll asks only for changes |
| `waiting_for_consent` | the server may not use `tokens` or the channel's class |
| `needs_auth` | the provider refused the sign-in; sign the account in again (`channels add`) |
| `failed` | the last attempt failed; it is retried, waiting longer each time (at most an hour) |
| `no_driver` | this server has no driver for the provider |

## Syncing now

```sh
nitya-node sync now        # every channel of the account
nitya-node sync now 1      # channel 1
```

Each line says what the run did, for example `1	incremental	incremental sync: 2 upserted, 0 deleted`,
and how many of your changes it wrote, if any.
A run already in progress is not started twice. The server's poll interval is
`--poll-secs` (or `pollSeconds` in the [module](../deployment/modules.md)), one minute by
default.

## Reading and changing events

```sh
nitya-node events 1                  # id, start, summary of each event
nitya-node events show 1 4kq…        # one event as iCalendar; an id prefix is enough
nitya-node events add 1 dentist.ics  # a new event from a file (- for stdin)
nitya-node events edit 1 4kq…        # in $EDITOR; or: events edit 1 4kq… changed.ics
nitya-node events delete 1 4kq…
```

Your device opens the events with your keys, and seals a change before sending it. The
server writes the change to the provider at once and says `written`. A series is listed
once, with `(series)`; an occurrence of it that was moved or changed is listed on its own,
with `(one occurrence)`.

Two kinds of change are refused rather than written:
- **What the provider can't hold** is refused before anything is sent. For Google: a time
  without a time zone (floating time), `RECURRENCE-ID;RANGE=THISANDFUTURE`, more than one
  event in a file (write each occurrence's change on its own), a time zone name that
  isn't an IANA one, `DURATION` instead of `DTEND`, an alarm after the start.
- **A change to an event that changed at the provider** since your device read it is
  refused as a conflict, so it never overwrites the other change. It stays listed under
  `events 1` as refused, with the reason; sync, then edit the event again.

If the provider can't be reached, the change waits (`queued`) and the next sync writes
it. Changes are written without sending invitations or updates to attendees.

## What a sync keeps

Each event is kept as iCalendar, sealed under the channel's data key and bound to its
channel and its id at the provider. For Google, the event is translated as
[[EXP-20261123-bhfzfs]] found Google keeps it: one event per Google event (a series, a
changed occurrence, a cancelled occurrence), times in their own zone, and fields Google
has no place for kept as Google's private extended properties. An event deleted at the
provider is kept as a record that it was deleted, with no content. If the provider
forgets where a channel was, the server lists everything again; whatever the new listing
no longer contains is marked deleted. Google does this now and then on its own.

Every sync that changed something, or failed, is in the [audit log](audit.md) as
`sync.run` by `system:sync-delegate`. A poll that found nothing new isn't logged.
Reading a channel's events is `calendar.read` and changing them `calendar.write`, by your
device; signing an account in is `channels.add`. Agents can't sign accounts in, and
read or change events only with a grant.
