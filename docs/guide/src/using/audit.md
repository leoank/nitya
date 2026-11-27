# The audit log

Every decision nitya makes about your account is in a log the server signs and your
devices check. That covers policy decisions, enrollments, renewals, revocations, and
refused requests with their reasons. Each account has its own chain. Each entry holds
the hash of the one before it, so an entry can't be changed, removed or inserted
without breaking the chain.

## What an entry says

Who acted (a device or agent by its key, the CA, the admin), what they did, to what, the
decision, the policies that made it, and when. Personal details, such as a device's
hostname or the reason for a refusal, sit beside the entry with only their hash inside
it. They can be redacted later without breaking the chain.

## Checking it

```sh
nitya-node … audit verify
nitya-node … audit verify --all
```

`audit verify` fetches the entries added since this device last checked. It checks
their signatures and links, compares the points other devices checked up to, and prints
what is new:

```text
nitya-node: audit log of ank verified to entry 15; 1 other device checkpoint(s) agree
12	device:59d09f88	audit.checkpoint	audit	allow	devices-act-as-their-user
13	device:e33ae222	audit.read	audit	allow	devices-act-as-their-user
```

It then hands the server a signed checkpoint, the point it verified, for your other
devices to compare against. Every keys command runs the same check first and stops if
the check fails.

## What it catches

The server signs its own log, so whoever runs the server could rewrite history and sign
it again. Only your devices' checkpoints catch that. A device refuses to go on when:

- an entry it checked earlier has changed, or the log is shorter than its checkpoint;
- another device's checkpoint names an entry that is no longer there;
- the server's audit key differs from the one the device saw first.

An ordinary check reads only entries after the device's checkpoint. `--all` reads from
the first entry, and also catches an edited detail in an older entry. A detail that was
redacted rather than edited passes.

Checking the log is itself logged, so the log grows with every check. How long to keep
old entries is still open.
