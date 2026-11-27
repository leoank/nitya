# Your keys

Everything nitya stores for an account is encrypted under keys the server does not hold.
The account's user key lives on your devices; the server keeps only copies of it wrapped
to each device, and data keys wrapped by it. This page is how you make, share, recover
and delegate them, with `nitya-node keys`.

## The first device

On the account's first enrolled device:

```sh
nitya-node --state-dir … --hostname … keys init
```

It asks twice for a **recovery passphrase** and makes the account's keys: the user key,
a data key per class (mail, calendar, tasks, tokens, files), and an append key. The
passphrase is the only way back in if you lose every device; nitya cannot reset it.

## Adding a device

A new device, once [enrolled](running.md#enrolling-a-device), has a certificate but no
keys. Give it them from a device that has them, after checking it is the device you
think:

```sh
# on the new device
nitya-node … keys fingerprint
# 1a2b-3c4d-5e6f-7a8b-9c0d

# on a device that has the keys
nitya-node … keys status                   # find the new device's serial
nitya-node … keys approve SERIAL 1a2b-3c4d-5e6f-7a8b-9c0d
```

The approval goes through only if the fingerprint matches, so the server (or whoever
runs it) cannot slip in a key of its own: enrolling a device of theirs into your account
gets them a certificate, never your keys. Each device also checks, whenever it unlocks,
that every device and recovery key in the account's key set was approved by one of your
devices.

`keys status` lists the account's devices, which of them hold the keys, which are
revoked, and which classes the server may use.

## Recovering

On a newly enrolled device, after losing the others:

```sh
nitya-node … keys recover
```

It asks for the recovery passphrase, unlocks the keys, and gives this device its own copy.

## The sync delegate

The server syncs your calendars and mail while every device is offline, but only the
classes you allow:

```sh
nitya-node … keys consent calendar
nitya-node … keys withdraw calendar
```

`consent` hands the server that class's data key, wrapped to its sync-delegate key.
`withdraw` deletes every copy it has. For a class without consent the server still
stores what arrives, sealed to the account so it cannot read it; your devices open it
when they next connect.

Whoever controls the running server can read the classes you consented to. Each use is
meant to go into the [audit log](audit.md), which shows it but does not prevent it. The
classes you did not consent to it cannot read at all.

## Revoking a device

When a device is [revoked](running.md#devices-and-revocation), its copy of the keys is
deleted and the account is marked for rotation. The next device of yours to run a keys
command rotates first: a new user key, new data keys for everything written from then on,
a new append key. Nothing already stored is lost or re-encrypted. A revoked device
reads nothing written after the rotation. What it could read before, it may have kept.

Until a device rotates, new data is still written under the old keys; reading it would
need both the revoked device's copy of the keys and the server's database.
