# Channels and sync

A channel is one provider account and one kind of data, for example the primary
calendar of a Google account. The server keeps each channel in sync on its own,
every minute, and you can make it sync at once.

> No provider can be connected yet: Google, Microsoft and CalDAV come next. This page
> describes the engine they plug into.

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
| `needs_auth` | the provider refused the sign-in; sign the account in again |
| `failed` | the last attempt failed; it is retried, waiting longer each time (at most an hour) |
| `no_driver` | this server has no driver for the provider |

## Syncing now

```sh
nitya-node sync now        # every channel of the account
nitya-node sync now 1      # channel 1
```

Each line says what the run did, for example `1	incremental	incremental sync: 2 upserted, 0 deleted`.
A run already in progress is not started twice. The server's poll interval is
`--poll-secs` (or `pollSeconds` in the [module](../deployment/modules.md)), one minute by
default.

## What a sync keeps

Events arrive exactly as the provider sent them, sealed under the channel's data key.
Each is bound to its channel and its id at the provider. An event deleted at the
provider is kept as a record that it was deleted, with no content. If the provider
forgets where a channel was, the server lists everything again; whatever the new
listing no longer contains is marked deleted.

Every sync that changed something, or failed, is in the [audit log](audit.md) as
`sync.run` by `system:sync-delegate`. A poll that found nothing new isn't logged.
